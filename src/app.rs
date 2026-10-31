//! Dashboard UI.

use std::collections::{HashMap, VecDeque};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use eframe::egui::{self, Align, Color32, CornerRadius, Frame, Layout, Margin, RichText, Sense, Stroke, Ui, vec2};
use egui_extras::{Column, Size, StripBuilder, TableBuilder};
use egui_plot::{Corner, HLine, Legend, Line, LineStyle, Plot, PlotPoints};
use rust_i18n::t;

use crate::activity::Status;
use crate::bridge::{Bridge, BrowserTabs, Discard};
use crate::classify::{AppUsage, Category};
use crate::close::{self, Closing, Outcome, Refusal};
use crate::i18n::Lang;
use crate::integration::{self, Integration};
use crate::pagefile;
use crate::priority::Governor;
use crate::process::Instance;
use crate::trim;
use crate::sampler::{self, Snapshot};
use crate::theme::{
    self, AMBER, BLUE, CARD, CARD_BORDER, CRITICAL_PCT, CYAN, GREEN, MUTED, RED, SLATE, TRACK, VIOLET,
};

const SAMPLE_INTERVAL: Duration = Duration::from_secs(2);
/// 10 minutes of history at the sample interval.
const HISTORY_LEN: usize = 300;
const TOP_N: usize = 6;
const PALETTE: [Color32; 4] = [BLUE, VIOLET, GREEN, AMBER];
/// Wait after the extension reports a discard before measuring the browser
/// again, so exiting renderer processes are gone from the sample.
const DISCARD_SETTLE: Duration = Duration::from_secs(5);
const LANG_KEY: &str = "lang";

const MB: f64 = 1024.0 * 1024.0;
const GB: f64 = MB * 1024.0;

pub struct App {
    rx: Receiver<Snapshot>,
    latest: Option<Snapshot>,
    started: Instant,
    /// `[seconds since start, physical %, commit %]`
    history: VecDeque<[f64; 3]>,
    filter: Filter,
    lang: Lang,
    governor: Arc<Mutex<Governor>>,
    /// `None` when the pipe could not be created (another memwarden is running).
    bridge: Option<Bridge>,
    browser_switch: BrowserSwitch,
    controls: Controls,
}

/// The "browser integration" switch in the header.
struct BrowserSwitch {
    integration: Integration,
    /// Read at start and after every change, not every frame.
    state: integration::State,
    error: Option<String>,
}

/// State of the priority and trim controls.
#[derive(Default)]
struct Controls {
    /// Bulk action waiting for confirmation.
    confirm: Option<Bulk>,
    /// Running trim, if any.
    trimming: Option<Receiver<trim::Report>>,
    last_trim: Option<trim::Report>,
    last_discard: Option<DiscardRun>,
    /// Close request waiting for the app to exit; one at a time.
    closing: Option<Closing>,
    last_close: Option<CloseResult>,
    /// Why a suggested app cannot be closed, refreshed with every sample.
    close_blockers: HashMap<String, Refusal>,
}

/// How the latest close request ended.
struct CloseResult {
    name: String,
    /// Private bytes of the app when closing was requested.
    private: u64,
    result: Result<Outcome, Refusal>,
}

/// Latest tab discard request and its measured effect.
struct DiscardRun {
    browser: Instance,
    /// Private bytes of the browser's app group when the request was sent.
    before: u64,
    /// `before` minus private bytes after the discard settled; negative if it grew.
    change: Option<i64>,
}

#[derive(Clone)]
enum Bulk {
    Lower,
    Trim,
    Discard(Instance),
    Close(String),
}

/// User actions, applied after the frame is drawn.
enum Action {
    Lower(String),
    Restore(String),
    LowerAllIdle,
    RestoreAll,
    Trim(String),
    TrimLongIdle,
    DiscardTabs(Instance),
    Close(String),
}

fn lock(governor: &Mutex<Governor>) -> MutexGuard<'_, Governor> {
    // A panic elsewhere must not prevent restoring priorities.
    governor.lock().unwrap_or_else(|e| e.into_inner())
}

/// Row filter of the applications table.
#[derive(Default)]
struct Filter {
    text: String,
    category: Option<Category>,
    idle_only: bool,
    close_only: bool,
}

impl Filter {
    fn matches(&self, app: &AppUsage, needle: &str) -> bool {
        (needle.is_empty() || app.name.to_lowercase().contains(needle))
            && self.category.is_none_or(|c| c == app.category)
            && (!self.idle_only || is_idle_candidate(app))
            && (!self.close_only || close::suggested(app))
    }
}

fn is_idle_candidate(app: &AppUsage) -> bool {
    app.category.idle_candidate() && matches!(app.status, Status::Idle(_))
}

/// The application group a browser process instance belongs to.
fn app_of(apps: &[AppUsage], browser: Instance) -> Option<&AppUsage> {
    apps.iter().find(|a| a.instances.contains(&browser))
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Priority: MEMWARDEN_LANG (for translators) > saved choice > Windows language.
        let lang = std::env::var("MEMWARDEN_LANG")
            .ok()
            .or_else(|| cc.storage.and_then(|s| s.get_string(LANG_KEY)))
            .and_then(|code| Lang::from_code(&code))
            .unwrap_or_else(Lang::detect);
        lang.activate();
        theme::apply(&cc.egui_ctx);
        theme::set_fonts(&cc.egui_ctx, lang);
        let governor = Arc::new(Mutex::new(Governor::new()));
        Self {
            rx: sampler::spawn(cc.egui_ctx.clone(), SAMPLE_INTERVAL, governor.clone()),
            latest: None,
            started: Instant::now(),
            history: VecDeque::with_capacity(HISTORY_LEN + 1),
            filter: Filter::default(),
            lang,
            governor,
            bridge: Bridge::start(cc.egui_ctx.clone()),
            browser_switch: {
                let integration = Integration::new();
                BrowserSwitch { state: integration.state(), integration, error: None }
            },
            controls: Controls::default(),
        }
    }

    /// Returns whether a new snapshot arrived.
    fn drain_samples(&mut self) -> bool {
        let mut new = false;
        while let Ok(snap) = self.rx.try_recv() {
            new = true;
            let t = snap.taken.duration_since(self.started).as_secs_f64();
            self.history.push_back([t, snap.sys.phys_pct(), snap.sys.commit_pct()]);
            if self.history.len() > HISTORY_LEN {
                self.history.pop_front();
            }
            self.latest = Some(snap);
        }
        new
    }

    /// Follows a running close request and finds suggested apps that cannot
    /// be closed, once per sample rather than every frame.
    fn update_close(&mut self) {
        let Some(snap) = self.latest.as_ref() else { return };
        let c = &mut self.controls;
        c.close_blockers = snap
            .apps
            .iter()
            .filter(|a| close::suggested(a))
            .filter_map(|a| close::blocker(&a.instances).map(|r| (a.name.clone(), r)))
            .collect();
        let done = c.closing.as_mut().and_then(|closing| {
            closing.poll(Instant::now()).map(|outcome| CloseResult {
                name: closing.name.clone(),
                private: closing.private,
                result: Ok(outcome),
            })
        });
        if done.is_some() {
            c.last_close = done;
            c.closing = None;
        }
    }

    fn apply(&mut self, actions: Vec<Action>) {
        let Some(snap) = self.latest.as_mut() else { return };
        let mut gov = lock(&self.governor);
        for action in actions {
            match action {
                Action::Lower(name) => {
                    if let Some(app) = snap.apps.iter().find(|a| a.name == name) {
                        gov.lower(app);
                    }
                }
                Action::Restore(name) => gov.restore(&name),
                Action::LowerAllIdle => {
                    let failed: usize = snap
                        .apps
                        .iter()
                        .filter(|a| is_idle_candidate(a) && !a.lowered)
                        .map(|a| gov.lower(a))
                        .sum();
                    gov.failed = failed;
                }
                Action::RestoreAll => gov.restore_all(),
                Action::Trim(name) => {
                    let targets: Vec<_> = snap.apps.iter().filter(|a| a.name == name).cloned().collect();
                    start_trim(&mut self.controls, targets);
                }
                Action::TrimLongIdle => {
                    let targets: Vec<_> = snap.apps.iter().filter(|a| trim::eligible(a)).cloned().collect();
                    start_trim(&mut self.controls, targets);
                }
                Action::DiscardTabs(browser) => {
                    let before = app_of(&snap.apps, browser).map(|a| a.private);
                    if let (Some(bridge), Some(before)) = (&self.bridge, before)
                        && bridge.discard_idle(browser)
                    {
                        self.controls.last_discard = Some(DiscardRun { browser, before, change: None });
                    }
                }
                Action::Close(name) => {
                    if self.controls.closing.is_none()
                        && let Some(app) = snap.apps.iter().find(|a| a.name == name)
                    {
                        let private = app.private;
                        match close::request(app, Instant::now()) {
                            Ok(closing) => {
                                self.controls.last_close = None;
                                self.controls.closing = Some(closing);
                            }
                            Err(refusal) => {
                                self.controls.last_close = Some(CloseResult { name, private, result: Err(refusal) });
                            }
                        }
                    }
                }
            }
        }
        // Show the change now instead of waiting for the next sample.
        for app in &mut snap.apps {
            app.lowered = gov.is_lowered(&app.name);
        }
    }
}

/// Measures the browser once the discard has settled.
fn measure_discard(run: &mut DiscardRun, browsers: &[BrowserTabs], snap: &Snapshot) {
    if run.change.is_some() {
        return;
    }
    let done_at = browsers.iter().find(|b| b.browser == run.browser).and_then(|b| match b.discard {
        Some(Discard::Done { at, .. }) => Some(at),
        _ => None,
    });
    if let Some(at) = done_at
        && snap.taken >= at + DISCARD_SETTLE
        && let Some(app) = app_of(&snap.apps, run.browser)
    {
        run.change = Some(run.before as i64 - app.private as i64);
    }
}

/// Starts a trim unless one is already running.
fn start_trim(controls: &mut Controls, targets: Vec<AppUsage>) {
    if controls.trimming.is_none() && !targets.is_empty() {
        controls.last_trim = None;
        controls.trimming = Some(trim::spawn(targets));
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        if self.drain_samples() {
            self.update_close();
        }
        if let Some(report) = self.controls.trimming.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.controls.last_trim = Some(report);
            self.controls.trimming = None;
        }
        // Keeps the "updated N s ago" label ticking between samples.
        ui.ctx().request_repaint_after(Duration::from_secs(1));

        let (lowered, failed) = {
            let gov = lock(&self.governor);
            (gov.lowered_count(), gov.failed)
        };
        let (browsers, incompatible) =
            self.bridge.as_ref().map_or((Vec::new(), false), |b| (b.browsers(), b.incompatible()));
        if let (Some(run), Some(snap)) = (self.controls.last_discard.as_mut(), self.latest.as_ref()) {
            measure_discard(run, &browsers, snap);
        }
        let Self { latest, history, filter, started, lang, controls, browser_switch, .. } = self;

        let picked = egui::Panel::top("header")
            .frame(Frame::new().fill(theme::BG).inner_margin(Margin::symmetric(20, 14)))
            .show_separator_line(false)
            .show(ui, |ui| header(ui, latest.as_ref(), *lang, lowered, browser_switch))
            .inner;
        if let Some(new_lang) = picked {
            *lang = new_lang;
            new_lang.activate();
            theme::set_fonts(ui.ctx(), new_lang);
        }

        let mut actions = Vec::new();
        egui::CentralPanel::default()
            .frame(Frame::new().fill(theme::BG).inner_margin(Margin { left: 20, right: 20, top: 0, bottom: 18 }))
            .show(ui, |ui| {
                let Some(snap) = latest.as_ref() else {
                    ui.centered_and_justified(|ui| ui.spinner());
                    return;
                };
                kpi_row(ui, snap);
                if snap.sys.commit_pct() >= CRITICAL_PCT {
                    commit_alert(ui, snap);
                }
                ui.allocate_ui(vec2(ui.available_width(), 280.0), |ui| {
                    StripBuilder::new(ui)
                        .size(Size::relative(0.62))
                        .size(Size::remainder())
                        .horizontal(|mut strip| {
                            strip.cell(|ui| history_card(ui, history, started.elapsed().as_secs_f64()));
                            strip.cell(|ui| top_card(ui, &snap.apps));
                        });
                });
                let tabs = (browsers.as_slice(), incompatible);
                actions = apps_card(ui, snap, filter, controls, (lowered, failed), tabs);
            });
        if !actions.is_empty() {
            self.apply(actions);
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        lock(&self.governor).restore_all();
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        storage.set_string(LANG_KEY, self.lang.code().to_owned());
    }
}

// ---------- building blocks ----------

fn card<R>(ui: &mut Ui, fill_height: bool, add: impl FnOnce(&mut Ui) -> R) -> R {
    Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1.0, CARD_BORDER))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::same(16))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if fill_height {
                ui.set_min_height(ui.available_height());
            }
            add(ui)
        })
        .inner
}

fn card_title(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(14.0).strong());
}

/// Thin rounded progress bar.
fn meter(ui: &mut Ui, frac: f64, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 6.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(3), TRACK);
    let mut fill = rect;
    fill.set_width(rect.width() * frac.clamp(0.0, 1.0) as f32);
    painter.rect_filled(fill, CornerRadius::same(3), color);
}

fn fmt_bytes(bytes: u64) -> String {
    let b = bytes as f64;
    if b >= GB { format!("{:.1} GB", b / GB) } else { format!("{:.0} MB", b / MB) }
}

/// A decrease as "−X", an increase as "+X".
fn fmt_change(decrease: i64) -> String {
    let sign = if decrease >= 0 { '−' } else { '+' };
    format!("{sign}{}", fmt_bytes(decrease.unsigned_abs()))
}

// ---------- sections ----------

/// Returns the newly picked language, if the user changed it.
fn header(ui: &mut Ui, latest: Option<&Snapshot>, lang: Lang, lowered: usize, switch: &mut BrowserSwitch) -> Option<Lang> {
    let mut picked = lang;
    ui.horizontal(|ui| {
        ui.label(RichText::new("memwarden").size(22.0).strong());
        ui.label(RichText::new(t!("app.subtitle")).size(14.0).color(MUTED));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            egui::ComboBox::from_id_salt("lang")
                .selected_text(lang.native_name())
                .width(110.0)
                .show_ui(ui, |ui| {
                    for l in Lang::ALL {
                        ui.selectable_value(&mut picked, l, l.native_name());
                    }
                });
            ui.add_space(8.0);
            browser_switch(ui, switch);
            ui.add_space(8.0);
            if lowered > 0 {
                ui.label(RichText::new(t!("status.lowered", count = lowered)).size(12.0).color(AMBER));
            } else {
                ui.label(RichText::new(t!("status.read_only")).size(12.0).color(MUTED));
            }
            ui.label(RichText::new("·").color(MUTED));
            match latest {
                Some(s) => {
                    let secs = s.taken.elapsed().as_secs();
                    ui.label(RichText::new(t!("status.updated", secs = secs)).size(12.0).color(MUTED));
                    ui.label(RichText::new(t!("status.live")).size(12.0).color(GREEN));
                }
                None => {
                    ui.label(RichText::new(t!("status.sampling")).size(12.0).color(MUTED));
                }
            }
        });
    });
    (picked != lang).then_some(picked)
}

/// Checkbox that registers memwarden-host with the browsers. Changes happen
/// only when the user clicks it; turning it off removes everything it wrote.
fn browser_switch(ui: &mut Ui, s: &mut BrowserSwitch) {
    let mut on = s.state == integration::State::Enabled;
    let path = s.integration.manifest_path().map_or_else(String::new, |p| p.display().to_string());
    let response = ui
        .add_enabled(s.integration.available(), egui::Checkbox::new(&mut on, t!("integration.toggle")))
        .on_hover_text(t!("integration.explain", path = path))
        .on_disabled_hover_text(t!("integration.unavailable"));
    if response.changed() {
        let result = if on { s.integration.enable() } else { s.integration.disable() };
        s.error = result.err().map(|e| e.to_string());
        s.state = s.integration.state();
    }
    if let Some(error) = &s.error {
        ui.label(RichText::new(t!("integration.failed", error = error)).size(12.0).color(RED));
    } else if s.state == integration::State::Elsewhere {
        ui.label(RichText::new(t!("integration.elsewhere")).size(12.0).color(AMBER));
    }
}

/// The whole card shows `help` on hover; the ⓘ after the title hints at it.
fn kpi(ui: &mut Ui, title: &str, help: &str, value: String, detail: &str, meter_value: Option<(f64, Color32)>) {
    card(ui, false, |ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.label(RichText::new(title).size(13.0).color(MUTED));
            ui.label(RichText::new("ⓘ").size(12.0).color(MUTED));
        });
        ui.label(RichText::new(value).size(28.0).strong());
        ui.label(RichText::new(detail).size(12.5).color(MUTED));
        match meter_value {
            Some((frac, color)) => meter(ui, frac, color),
            None => {
                ui.add_space(6.0);
            }
        }
    });
    // The column holds only this card, so its rect is the card's.
    ui.interact(ui.min_rect(), ui.scope_id().with("kpi_help"), Sense::hover()).on_hover_text(help);
}

fn kpi_row(ui: &mut Ui, snap: &Snapshot) {
    let s = &snap.sys;
    ui.columns(4, |cols| {
        let phys = s.phys_pct();
        kpi(
            &mut cols[0],
            &t!("kpi.physical.title"),
            &t!("kpi.physical.help"),
            fmt_bytes(s.phys_used()),
            &t!("kpi.physical.detail", total = fmt_bytes(s.phys_total), pct = format!("{phys:.0}")),
            Some((phys / 100.0, theme::level_color(phys))),
        );
        let commit = s.commit_pct();
        kpi(
            &mut cols[1],
            &t!("kpi.commit.title"),
            &commit_help(snap),
            fmt_bytes(s.commit_total),
            &t!("kpi.commit.detail", limit = fmt_bytes(s.commit_limit), pct = format!("{commit:.0}")),
            Some((commit / 100.0, theme::level_color(commit))),
        );
        kpi(
            &mut cols[2],
            &t!("kpi.available.title"),
            &t!("kpi.available.help"),
            fmt_bytes(s.phys_avail),
            &t!("kpi.available.detail", cache = fmt_bytes(s.system_cache)),
            Some((s.phys_avail as f64 / s.phys_total as f64, BLUE)),
        );
        kpi(
            &mut cols[3],
            &t!("kpi.kernel.title"),
            &t!("kpi.kernel.help"),
            fmt_bytes(s.kernel_paged + s.kernel_nonpaged),
            &t!("kpi.kernel.detail", paged = fmt_bytes(s.kernel_paged), nonpaged = fmt_bytes(s.kernel_nonpaged)),
            None,
        );
    });
}

/// Commit card explanation plus what the page file settings allow.
fn commit_help(snap: &Snapshot) -> String {
    let help = t!("kpi.commit.help");
    let Some(pf) = &snap.pagefile else { return help.into_owned() };
    let (current, max) = (fmt_bytes(pf.current()), fmt_bytes(pf.max()));
    let limit = fmt_bytes(pf.potential_limit(snap.sys.commit_limit));
    let note = if pf.0.is_empty() {
        t!("pagefile.none")
    } else if pf.system_managed() {
        t!("pagefile.system", current = current, max = max, limit = limit)
    } else {
        t!("pagefile.custom", current = current, max = max, limit = limit)
    };
    format!("{help}\n\n{note}")
}

fn commit_alert(ui: &mut Ui, snap: &Snapshot) {
    let pct = format!("{:.0}", snap.sys.commit_pct());
    // Windows grows a page file it manages on its own; otherwise only the user can.
    let growable = snap.pagefile.as_ref().filter(|pf| pf.system_managed() && pf.max() > pf.current());
    Frame::new()
        .fill(RED.gamma_multiply(0.12))
        .stroke(Stroke::new(1.0, RED.gamma_multiply(0.6)))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if let Some(pf) = growable {
                let limit = fmt_bytes(pf.potential_limit(snap.sys.commit_limit));
                ui.label(RichText::new(t!("alert.commit_auto", pct = pct, limit = limit)).color(RED));
                return;
            }
            ui.label(RichText::new(t!("alert.commit", pct = pct)).color(RED));
            if ui.button(t!("pagefile.open")).on_hover_text(t!("pagefile.open_help")).clicked() {
                // The dialog is part of Windows; if it cannot start there is nothing more to offer.
                let _ = pagefile::open_settings();
            }
        });
}

fn history_card(ui: &mut Ui, history: &VecDeque<[f64; 3]>, now: f64) {
    card(ui, true, |ui| {
        card_title(ui, &t!("history.title"));
        // x axis: minutes relative to now (negative = past)
        let series = |idx: usize| -> PlotPoints {
            history.iter().map(|h| [(h[0] - now) / 60.0, h[idx]]).collect()
        };
        Plot::new("history")
            .legend(Legend::default().position(Corner::LeftTop))
            .show_grid([false, true])
            .include_x(-(HISTORY_LEN as f64 * SAMPLE_INTERVAL.as_secs_f64()) / 60.0)
            .include_x(0.0)
            .include_y(0.0)
            .include_y(100.0)
            .allow_drag(false)
            .allow_zoom(false)
            .allow_scroll(false)
            .allow_boxed_zoom(false)
            .show_background(false)
            .x_axis_formatter(|mark, _| t!("history.minutes", n = format!("{:.0}", mark.value)).into_owned())
            .y_axis_formatter(|mark, _| format!("{:.0}%", mark.value))
            .show(ui, |plot| {
                plot.line(Line::new(t!("history.physical"), series(1)).color(BLUE).width(2.0).fill(0.0));
                plot.line(Line::new(t!("history.commit"), series(2)).color(VIOLET).width(2.0));
                plot.hline(
                    HLine::new(t!("history.critical"), CRITICAL_PCT)
                        .color(RED.gamma_multiply(0.7))
                        .style(LineStyle::dashed_loose()),
                );
            });
    });
}

fn top_card(ui: &mut Ui, apps: &[AppUsage]) {
    card(ui, true, |ui| {
        card_title(ui, &t!("top.title"));
        ui.spacing_mut().item_spacing.y = 4.0;
        let max = apps.first().map_or(1, |a| a.private.max(1)) as f64;
        for (i, a) in apps.iter().take(TOP_N).enumerate() {
            ui.horizontal(|ui| {
                ui.label(RichText::new(&a.name).size(12.5));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new(fmt_bytes(a.private)).size(12.5).color(MUTED));
                });
            });
            meter(ui, a.private as f64 / max, PALETTE[i % PALETTE.len()]);
        }
    });
}

fn category_color(c: Category) -> Color32 {
    match c {
        Category::Application => AMBER,
        Category::WebApp => CYAN,
        Category::Browser => BLUE,
        Category::Service => SLATE,
        Category::VirtualMachine => VIOLET,
        Category::Security => GREEN,
        Category::System => MUTED,
    }
}

/// Small rounded label with a tinted background.
fn chip(ui: &mut Ui, text: &str, color: Color32) -> egui::Response {
    Frame::new()
        .fill(color.gamma_multiply(0.16))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::symmetric(8, 2))
        .show(ui, |ui| ui.label(RichText::new(text).size(11.5).color(color)))
        .response
}

fn status_text(app: &AppUsage) -> RichText {
    if !app.category.idle_candidate() {
        return RichText::new("—").color(MUTED);
    }
    let (text, color) = match app.status {
        _ if close::suggested(app) => (t!("close.status"), RED),
        Status::Active => (t!("activity.active"), GREEN),
        Status::Inactive => (t!("activity.inactive"), MUTED),
        Status::Idle(quiet) => (t!("activity.idle", min = quiet.as_secs() / 60), AMBER),
    };
    // A note marks apps with an active audio session; an arrow marks apps
    // whose memory priority memwarden lowered.
    let text = if app.audio { format!("♪ {text}") } else { text.into_owned() };
    if app.lowered { RichText::new(format!("↓ {text}")).color(VIOLET) } else { RichText::new(text).color(color) }
}

/// `priority` is `(lowered app count, processes that failed in the last action)`;
/// `tabs` is `(browsers connected through the extension, incompatible extension seen)`.
fn apps_card(
    ui: &mut Ui,
    snap: &Snapshot,
    filter: &mut Filter,
    controls: &mut Controls,
    priority: (usize, usize),
    tabs: (&[BrowserTabs], bool),
) -> Vec<Action> {
    let mut actions = Vec::new();
    card(ui, true, |ui| {
        let (idle_count, idle_bytes) = snap
            .apps
            .iter()
            .filter(|a| is_idle_candidate(a))
            .fold((0usize, 0u64), |acc, a| (acc.0 + 1, acc.1 + a.private));
        let (trim_count, trim_bytes) = snap
            .apps
            .iter()
            .filter(|a| trim::eligible(a))
            .fold((0usize, 0u64), |acc, a| (acc.0 + 1, acc.1 + a.working_set));
        let (close_count, close_bytes) = snap
            .apps
            .iter()
            .filter(|a| close::suggested(a))
            .fold((0usize, 0u64), |acc, a| (acc.0 + 1, acc.1 + a.private));
        let (lowered, failed) = priority;
        let trimming = controls.trimming.is_some();

        ui.horizontal(|ui| {
            card_title(ui, &t!("apps.title"));
            ui.label(RichText::new(t!("apps.entries", count = snap.apps.len())).size(12.0).color(MUTED));
            if idle_count > 0 {
                chip(ui, &t!("apps.idle_summary", count = idle_count, size = fmt_bytes(idle_bytes)), AMBER);
            }
            if close_count > 0 {
                let help = t!(
                    "close.summary_help",
                    min = close::SUGGEST_AFTER.as_secs() / 60,
                    mb = close::SUGGEST_MIN_PRIVATE / (1024 * 1024)
                );
                // Apps with only a tray icon count too, but the user has to quit them there.
                let tray = controls.close_blockers.values().filter(|&&r| r == Refusal::NoWindow).count();
                let size = fmt_bytes(close_bytes);
                let summary = if tray > 0 {
                    t!("close.summary_tray", count = close_count, size = size, tray = tray)
                } else {
                    t!("close.summary", count = close_count, size = size)
                };
                chip(ui, &summary, RED).on_hover_text(help);
            }
            if let Some(r) = &controls.last_trim {
                chip(ui, &t!("trim.result", count = r.apps, size = fmt_bytes(r.freed)), GREEN);
                if r.failed > 0 {
                    ui.label(RichText::new(t!("priority.failed", count = r.failed)).size(12.0).color(MUTED));
                }
            }
            if snap.denied > 0 {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new(t!("apps.unreadable", count = snap.denied)).size(12.0).color(MUTED));
                });
            }
        });
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut filter.text).hint_text(t!("apps.filter")).desired_width(220.0));
            let selected = filter.category.map_or_else(|| t!("category.all"), |c| t!(c.label_key()));
            egui::ComboBox::from_id_salt("category")
                .selected_text(selected)
                .width(170.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut filter.category, None, t!("category.all"));
                    for c in Category::ALL {
                        ui.selectable_value(&mut filter.category, Some(c), t!(c.label_key()));
                    }
                });
            ui.checkbox(&mut filter.idle_only, t!("apps.idle_only"));
            ui.checkbox(&mut filter.close_only, t!("apps.close_only"));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if let Some(bulk) = controls.confirm.clone() {
                    if ui.button(t!("priority.cancel")).clicked() {
                        controls.confirm = None;
                    }
                    let (yes, question, action) = match bulk {
                        Bulk::Lower => (
                            t!("priority.confirm_yes"),
                            t!("priority.confirm", count = idle_count, size = fmt_bytes(idle_bytes)),
                            Action::LowerAllIdle,
                        ),
                        Bulk::Trim => (
                            t!("trim.confirm_yes"),
                            t!(
                                "trim.confirm",
                                count = trim_count,
                                min = trim::TRIM_AFTER.as_secs() / 60,
                                size = fmt_bytes(trim_bytes)
                            ),
                            Action::TrimLongIdle,
                        ),
                        Bulk::Discard(browser) => {
                            let name = app_of(&snap.apps, browser).map_or("", |a| a.name.as_str());
                            let count = tabs.0.iter().find(|b| b.browser == browser).map_or(0, |b| b.tabs.candidates);
                            (
                                t!("tabs.confirm_yes"),
                                t!("tabs.confirm", name = name, count = count),
                                Action::DiscardTabs(browser),
                            )
                        }
                        Bulk::Close(name) => (
                            t!("close.confirm_yes"),
                            t!("close.confirm", name = name.as_str()),
                            Action::Close(name),
                        ),
                    };
                    if ui.button(RichText::new(yes).color(AMBER)).clicked() {
                        actions.push(action);
                        controls.confirm = None;
                    }
                    ui.label(question);
                    return;
                }
                let trim_all = ui
                    .add_enabled(
                        trim_count > 0 && !trimming,
                        egui::Button::new(t!("trim.all", count = trim_count)),
                    )
                    .on_hover_text(t!("trim.explain", min = trim::TRIM_AFTER.as_secs() / 60));
                if trim_all.clicked() {
                    controls.confirm = Some(Bulk::Trim);
                }
                if lowered > 0 && ui.button(t!("priority.restore_all", count = lowered)).clicked() {
                    actions.push(Action::RestoreAll);
                }
                let lower_all = ui
                    .add_enabled(idle_count > 0, egui::Button::new(t!("priority.lower_all", count = idle_count)))
                    .on_hover_text(t!("priority.explain"));
                if lower_all.clicked() {
                    controls.confirm = Some(Bulk::Lower);
                }
                if trimming {
                    ui.label(RichText::new(t!("trim.running")).size(12.0).color(MUTED));
                    ui.spinner();
                }
                if failed > 0 {
                    ui.label(RichText::new(t!("priority.failed", count = failed)).size(12.0).color(MUTED));
                }
            });
        });

        tabs_rows(ui, snap, tabs, controls);
        close_row(ui, controls);

        let needle = filter.text.to_lowercase();
        let rows: Vec<&AppUsage> = snap.apps.iter().filter(|a| filter.matches(a, &needle)).collect();
        let total = snap.sys.phys_total as f64;
        // Bars are scaled to the largest application so small differences stay visible.
        let max = snap.apps.first().map_or(1, |a| a.private.max(1)) as f64;

        let right = |ui: &mut Ui, text: RichText| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| ui.label(text));
        };
        TableBuilder::new(ui)
            .striped(true)
            .auto_shrink([false, false])
            .cell_layout(Layout::left_to_right(Align::Center))
            .column(Column::remainder().at_least(140.0))
            .column(Column::exact(140.0))
            .column(Column::exact(70.0))
            .column(Column::exact(64.0))
            .column(Column::exact(96.0))
            .column(Column::exact(100.0))
            .column(Column::exact(124.0))
            .column(Column::exact(210.0))
            .column(Column::exact(110.0))
            .header(26.0, |mut h| {
                for (key, align_right) in [
                    ("col.name", false),
                    ("col.category", false),
                    ("col.processes", true),
                    ("col.cpu", true),
                    ("col.private", true),
                    ("col.working_set", true),
                ] {
                    h.col(|ui| {
                        let text = RichText::new(t!(key)).color(MUTED);
                        if align_right {
                            right(ui, text);
                        } else {
                            ui.label(text);
                        }
                    });
                }
                h.col(|ui| {
                    ui.add_space(16.0);
                    ui.label(RichText::new(t!("col.status")).color(MUTED));
                });
                h.col(|ui| {
                    ui.label(RichText::new(t!("col.action")).color(MUTED));
                });
                h.col(|ui| {
                    ui.label(RichText::new(t!("col.share")).color(MUTED));
                });
            })
            .body(|body| {
                body.rows(30.0, rows.len(), |mut row| {
                    let a = rows[row.index()];
                    row.col(|ui| {
                        ui.label(&a.name);
                    });
                    row.col(|ui| {
                        chip(ui, &t!(a.category.label_key()), category_color(a.category));
                    });
                    row.col(|ui| right(ui, RichText::new(a.count.to_string())));
                    row.col(|ui| right(ui, RichText::new(format!("{:.1}%", a.cpu_pct))));
                    row.col(|ui| right(ui, RichText::new(fmt_bytes(a.private))));
                    row.col(|ui| right(ui, RichText::new(fmt_bytes(a.working_set))));
                    row.col(|ui| {
                        ui.add_space(16.0);
                        let status = ui.label(status_text(a));
                        if let (true, Status::Idle(quiet)) = (close::suggested(a), a.status) {
                            let size = fmt_bytes(a.private);
                            status.on_hover_text(t!("close.status_help", min = quiet.as_secs() / 60, size = size));
                        }
                    });
                    row.col(|ui| {
                        if a.lowered {
                            if ui.small_button(t!("priority.restore")).clicked() {
                                actions.push(Action::Restore(a.name.clone()));
                            }
                        } else if a.category.idle_candidate() && a.status != Status::Active {
                            let button = ui.small_button(t!("priority.lower")).on_hover_text(t!("priority.explain"));
                            if button.clicked() {
                                actions.push(Action::Lower(a.name.clone()));
                            }
                        }
                        if trim::eligible(a) {
                            let button = ui
                                .add_enabled(!trimming, egui::Button::new(t!("trim.button")).small())
                                .on_hover_text(t!("trim.explain", min = trim::TRIM_AFTER.as_secs() / 60));
                            if button.clicked() {
                                actions.push(Action::Trim(a.name.clone()));
                            }
                        }
                        if close::suggested(a) {
                            let blocker = controls.close_blockers.get(&a.name).copied();
                            let enabled = blocker.is_none() && controls.closing.is_none() && controls.confirm.is_none();
                            let mut button = ui
                                .add_enabled(enabled, egui::Button::new(t!("close.button")).small())
                                .on_hover_text(t!("close.explain"));
                            if let Some(refusal) = blocker {
                                button = button.on_disabled_hover_text(t!(refusal_key(refusal)));
                            }
                            if button.clicked() {
                                controls.confirm = Some(Bulk::Close(a.name.clone()));
                            }
                        }
                    });
                    row.col(|ui| {
                        let share = a.private as f64 / total;
                        ui.label(RichText::new(format!("{:>4.1}%", share * 100.0)).size(12.0).color(MUTED));
                        meter(ui, a.private as f64 / max, BLUE);
                    });
                });
            });
    });
    actions
}

/// One line per browser connected through the extension: tab counts, the
/// discard button and the last result. Nothing is shown without the extension.
fn tabs_rows(ui: &mut Ui, snap: &Snapshot, (browsers, incompatible): (&[BrowserTabs], bool), controls: &mut Controls) {
    if incompatible {
        ui.label(RichText::new(t!("tabs.incompatible")).size(12.0).color(MUTED));
    }
    for b in browsers {
        let Some(app) = app_of(&snap.apps, b.browser) else { continue };
        let t = b.tabs;
        ui.horizontal(|ui| {
            let summary =
                t!("tabs.summary", name = app.name, total = t.total, discarded = t.discarded, candidates = t.candidates);
            chip(ui, &summary, BLUE);
            let running = b.discard == Some(Discard::Running);
            let button = ui
                .add_enabled(
                    t.candidates > 0 && !running && controls.confirm.is_none(),
                    egui::Button::new(t!("tabs.discard", count = t.candidates)).small(),
                )
                .on_hover_text(t!("tabs.explain"));
            if button.clicked() {
                controls.confirm = Some(Bulk::Discard(b.browser));
            }
            let run = controls.last_discard.as_ref().filter(|r| r.browser == b.browser);
            match (b.discard, run.map(|r| r.change)) {
                (Some(Discard::Running), _) => {
                    ui.spinner();
                    ui.label(RichText::new(t!("tabs.running")).size(12.0).color(MUTED));
                }
                (Some(Discard::Done { discarded, .. }), Some(Some(change))) => {
                    chip(ui, &t!("tabs.result", count = discarded, change = fmt_change(change)), GREEN);
                }
                (Some(Discard::Done { discarded, .. }), Some(None)) => {
                    ui.label(RichText::new(t!("tabs.measuring", count = discarded)).size(12.0).color(MUTED));
                }
                _ => {}
            }
        });
    }
}

/// Progress or result of the latest close request.
fn close_row(ui: &mut Ui, controls: &Controls) {
    if let Some(closing) = &controls.closing {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(RichText::new(t!("close.running", name = closing.name)).size(12.0).color(MUTED));
        });
    } else if let Some(r) = &controls.last_close {
        let name = r.name.as_str();
        ui.horizontal(|ui| {
            let note = match r.result {
                Ok(Outcome::Closed) => {
                    chip(ui, &t!("close.closed", name = name, size = fmt_bytes(r.private)), GREEN);
                    return;
                }
                Ok(Outcome::InBackground) => t!("close.in_background", name = name),
                Ok(Outcome::StillOpen) => t!("close.still_open", name = name),
                Err(refusal) => t!("close.refused", name = name, reason = t!(refusal_key(refusal))),
            };
            ui.label(RichText::new(note).size(12.0).color(AMBER));
        });
    }
}

fn refusal_key(refusal: Refusal) -> &'static str {
    match refusal {
        Refusal::NoWindow => "close.no_window",
        Refusal::NotResponding => "close.not_responding",
        Refusal::Denied => "close.denied",
    }
}
