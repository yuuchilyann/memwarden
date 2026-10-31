//! Dark dashboard theme and fonts.

use std::sync::Arc;

use eframe::egui::{self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, Stroke, Visuals};

use crate::i18n::Lang;

pub const BG: Color32 = Color32::from_rgb(0x0f, 0x13, 0x1a);
pub const CARD: Color32 = Color32::from_rgb(0x18, 0x1e, 0x28);
pub const CARD_BORDER: Color32 = Color32::from_rgb(0x26, 0x2e, 0x3b);
pub const TEXT: Color32 = Color32::from_rgb(0xe6, 0xea, 0xf0);
pub const MUTED: Color32 = Color32::from_rgb(0x8a, 0x94, 0xa6);
pub const TRACK: Color32 = Color32::from_rgb(0x26, 0x2e, 0x3b);

pub const BLUE: Color32 = Color32::from_rgb(0x4c, 0x9a, 0xff);
pub const VIOLET: Color32 = Color32::from_rgb(0xa7, 0x8b, 0xfa);
pub const GREEN: Color32 = Color32::from_rgb(0x34, 0xd3, 0x99);
pub const AMBER: Color32 = Color32::from_rgb(0xfb, 0xbf, 0x24);
pub const RED: Color32 = Color32::from_rgb(0xf8, 0x71, 0x71);

/// Usage at or above this percentage is shown as critical.
pub const CRITICAL_PCT: f64 = 85.0;
const WARN_PCT: f64 = 70.0;

pub fn level_color(pct: f64) -> Color32 {
    if pct >= CRITICAL_PCT {
        RED
    } else if pct >= WARN_PCT {
        AMBER
    } else {
        GREEN
    }
}

pub fn apply(ctx: &egui::Context) {
    let mut v = Visuals::dark();
    v.panel_fill = BG;
    v.window_fill = CARD;
    v.extreme_bg_color = Color32::from_rgb(0x12, 0x17, 0x1f);
    v.faint_bg_color = Color32::from_rgb(0x15, 0x1b, 0x24);
    v.override_text_color = Some(TEXT);
    v.selection.bg_fill = BLUE.gamma_multiply(0.35);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, CARD_BORDER);
    for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active] {
        w.corner_radius = CornerRadius::same(6);
    }
    // Always dark, regardless of the Windows app theme.
    ctx.set_theme(egui::ThemePreference::Dark);
    ctx.style_mut_of(egui::Theme::Dark, |s| {
        s.visuals = v;
        s.spacing.item_spacing = egui::vec2(10.0, 10.0);
    });
}

const JHENGHEI: &str = r"C:\Windows\Fonts\msjh.ttc";
const YU_GOTHIC: &str = r"C:\Windows\Fonts\YuGothM.ttc";
const MALGUN: &str = r"C:\Windows\Fonts\malgun.ttf";

/// Installs Windows system fonts as fallbacks after egui's Latin fonts.
/// The language's own font goes first so shared Han characters use the right
/// regional glyphs; a second font covers the scripts in the language picker.
/// Fonts are read from the local Windows installation, not bundled.
pub fn set_fonts(ctx: &egui::Context, lang: Lang) {
    let chain: &[&str] = match lang {
        Lang::ZhTw | Lang::En => &[JHENGHEI, MALGUN],
        Lang::Ja => &[YU_GOTHIC, MALGUN],
        Lang::Ko => &[MALGUN, JHENGHEI],
    };
    let mut fonts = FontDefinitions::default();
    for (i, path) in chain.iter().enumerate() {
        let Ok(bytes) = std::fs::read(path) else { continue };
        fonts.font_data.insert(path.to_string(), Arc::new(FontData::from_owned(bytes)));
        let list = fonts.families.entry(FontFamily::Proportional).or_default();
        // Yu Gothic's baseline differs from egui's Latin font, so mixed
        // Japanese/digit text looks uneven; let it render the Latin text too.
        if i == 0 && lang == Lang::Ja {
            list.insert(0, path.to_string());
        } else {
            list.push(path.to_string());
        }
        fonts.families.entry(FontFamily::Monospace).or_default().push(path.to_string());
    }
    ctx.set_fonts(fonts);
}
