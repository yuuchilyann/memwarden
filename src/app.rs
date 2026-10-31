//! Dashboard UI.

use std::collections::VecDeque;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use eframe::egui::{self, Align, Color32, CornerRadius, Frame, Layout, Margin, RichText, Sense, Stroke, Ui, vec2};
use egui_extras::{Column, Size, StripBuilder, TableBuilder};
use egui_plot::{Corner, HLine, Legend, Line, LineStyle, Plot, PlotPoints};
use rust_i18n::t;

use crate::i18n::Lang;
use crate::process::AppUsage;
use crate::sampler::{self, Snapshot};
use crate::theme::{self, AMBER, BLUE, CARD, CARD_BORDER, CRITICAL_PCT, GREEN, MUTED, RED, TRACK, VIOLET};

const SAMPLE_INTERVAL: Duration = Duration::from_secs(2);
/// 10 minutes of history at the sample interval.
const HISTORY_LEN: usize = 300;
const TOP_N: usize = 8;
const PALETTE: [Color32; 4] = [BLUE, VIOLET, GREEN, AMBER];
const LANG_KEY: &str = "lang";

const MB: f64 = 1024.0 * 1024.0;
const GB: f64 = MB * 1024.0;

pub struct App {
    rx: Receiver<Snapshot>,
    latest: Option<Snapshot>,
    started: Instant,
    /// `[seconds since start, physical %, commit %]`
    history: VecDeque<[f64; 3]>,
    filter: String,
    lang: Lang,
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
        Self {
            rx: sampler::spawn(cc.egui_ctx.clone(), SAMPLE_INTERVAL),
            latest: None,
            started: Instant::now(),
            history: VecDeque::with_capacity(HISTORY_LEN + 1),
            filter: String::new(),
            lang,
        }
    }

    fn drain_samples(&mut self) {
        while let Ok(snap) = self.rx.try_recv() {
            let t = snap.taken.duration_since(self.started).as_secs_f64();
            self.history.push_back([t, snap.sys.phys_pct(), snap.sys.commit_pct()]);
            if self.history.len() > HISTORY_LEN {
                self.history.pop_front();
            }
            self.latest = Some(snap);
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.drain_samples();
        // Keeps the "updated N s ago" label ticking between samples.
        ui.ctx().request_repaint_after(Duration::from_secs(1));

        let Self { latest, history, filter, started, lang, .. } = self;

        let picked = egui::Panel::top("header")
            .frame(Frame::new().fill(theme::BG).inner_margin(Margin::symmetric(20, 14)))
            .show_separator_line(false)
            .show(ui, |ui| header(ui, latest.as_ref(), *lang))
            .inner;
        if let Some(new_lang) = picked {
            *lang = new_lang;
            new_lang.activate();
            theme::set_fonts(ui.ctx(), new_lang);
        }

        egui::CentralPanel::default()
            .frame(Frame::new().fill(theme::BG).inner_margin(Margin { left: 20, right: 20, top: 0, bottom: 18 }))
            .show(ui, |ui| {
                let Some(snap) = latest.as_ref() else {
                    ui.centered_and_justified(|ui| ui.spinner());
                    return;
                };
                kpi_row(ui, snap);
                if snap.sys.commit_pct() >= CRITICAL_PCT {
                    commit_alert(ui, snap.sys.commit_pct());
                }
                ui.allocate_ui(vec2(ui.available_width(), 340.0), |ui| {
                    StripBuilder::new(ui)
                        .size(Size::relative(0.62))
                        .size(Size::remainder())
                        .horizontal(|mut strip| {
                            strip.cell(|ui| history_card(ui, history, started.elapsed().as_secs_f64()));
                            strip.cell(|ui| top_card(ui, &snap.apps));
                        });
                });
                apps_card(ui, snap, filter);
            });
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

// ---------- sections ----------

/// Returns the newly picked language, if the user changed it.
fn header(ui: &mut Ui, latest: Option<&Snapshot>, lang: Lang) -> Option<Lang> {
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
            ui.label(RichText::new(t!("status.read_only")).size(12.0).color(MUTED));
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

fn kpi(ui: &mut Ui, title: &str, value: String, detail: &str, meter_value: Option<(f64, Color32)>) {
    card(ui, false, |ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.label(RichText::new(title).size(13.0).color(MUTED));
        ui.label(RichText::new(value).size(28.0).strong());
        ui.label(RichText::new(detail).size(12.5).color(MUTED));
        match meter_value {
            Some((frac, color)) => meter(ui, frac, color),
            None => {
                ui.add_space(6.0);
            }
        }
    });
}

fn kpi_row(ui: &mut Ui, snap: &Snapshot) {
    let s = &snap.sys;
    ui.columns(4, |cols| {
        let phys = s.phys_pct();
        kpi(
            &mut cols[0],
            &t!("kpi.physical.title"),
            fmt_bytes(s.phys_used()),
            &t!("kpi.physical.detail", total = fmt_bytes(s.phys_total), pct = format!("{phys:.0}")),
            Some((phys / 100.0, theme::level_color(phys))),
        );
        let commit = s.commit_pct();
        kpi(
            &mut cols[1],
            &t!("kpi.commit.title"),
            fmt_bytes(s.commit_total),
            &t!("kpi.commit.detail", limit = fmt_bytes(s.commit_limit), pct = format!("{commit:.0}")),
            Some((commit / 100.0, theme::level_color(commit))),
        );
        kpi(
            &mut cols[2],
            &t!("kpi.available.title"),
            fmt_bytes(s.phys_avail),
            &t!("kpi.available.detail", cache = fmt_bytes(s.system_cache)),
            Some((s.phys_avail as f64 / s.phys_total as f64, BLUE)),
        );
        kpi(
            &mut cols[3],
            &t!("kpi.kernel.title"),
            fmt_bytes(s.kernel_paged + s.kernel_nonpaged),
            &t!("kpi.kernel.detail", paged = fmt_bytes(s.kernel_paged), nonpaged = fmt_bytes(s.kernel_nonpaged)),
            None,
        );
    });
}

fn commit_alert(ui: &mut Ui, pct: f64) {
    Frame::new()
        .fill(RED.gamma_multiply(0.12))
        .stroke(Stroke::new(1.0, RED.gamma_multiply(0.6)))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(t!("alert.commit", pct = format!("{pct:.0}"))).color(RED));
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

fn apps_card(ui: &mut Ui, snap: &Snapshot, filter: &mut String) {
    card(ui, true, |ui| {
        ui.horizontal(|ui| {
            card_title(ui, &t!("apps.title"));
            ui.label(RichText::new(t!("apps.entries", count = snap.apps.len())).size(12.0).color(MUTED));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add(egui::TextEdit::singleline(filter).hint_text(t!("apps.filter")).desired_width(220.0));
                if snap.denied > 0 {
                    ui.label(RichText::new(t!("apps.unreadable", count = snap.denied)).size(12.0).color(MUTED));
                }
            });
        });

        let needle = filter.to_lowercase();
        let rows: Vec<&AppUsage> =
            snap.apps.iter().filter(|a| needle.is_empty() || a.name.to_lowercase().contains(&needle)).collect();
        let total = snap.sys.phys_total as f64;
        // Bars are scaled to the largest application so small differences stay visible.
        let max = snap.apps.first().map_or(1, |a| a.private.max(1)) as f64;

        let right = |ui: &mut Ui, text: String| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| ui.label(text));
        };
        TableBuilder::new(ui)
            .striped(true)
            .auto_shrink([false, false])
            .cell_layout(Layout::left_to_right(Align::Center))
            .column(Column::remainder().at_least(220.0))
            .column(Column::exact(90.0))
            .column(Column::exact(110.0))
            .column(Column::exact(120.0))
            .column(Column::exact(210.0))
            .header(26.0, |mut h| {
                h.col(|ui| {
                    ui.label(RichText::new(t!("col.name")).color(MUTED));
                });
                for key in ["col.processes", "col.private", "col.working_set"] {
                    h.col(|ui| {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(RichText::new(t!(key)).color(MUTED))
                        });
                    });
                }
                h.col(|ui| {
                    ui.add_space(12.0);
                    ui.label(RichText::new(t!("col.share")).color(MUTED));
                });
            })
            .body(|body| {
                body.rows(28.0, rows.len(), |mut row| {
                    let a = rows[row.index()];
                    row.col(|ui| {
                        ui.label(&a.name);
                    });
                    row.col(|ui| right(ui, a.count.to_string()));
                    row.col(|ui| right(ui, fmt_bytes(a.private)));
                    row.col(|ui| right(ui, fmt_bytes(a.working_set)));
                    row.col(|ui| {
                        let share = a.private as f64 / total;
                        ui.add_space(12.0);
                        ui.label(RichText::new(format!("{:>4.1}%", share * 100.0)).size(12.0).color(MUTED));
                        meter(ui, a.private as f64 / max, BLUE);
                    });
                });
            });
    });
}
