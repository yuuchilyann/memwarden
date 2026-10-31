//! UI language selection. Translations live in `locales/<code>.yml`.

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    ZhTw,
    En,
    Ja,
    Ko,
}

impl Lang {
    pub const ALL: [Lang; 4] = [Lang::ZhTw, Lang::En, Lang::Ja, Lang::Ko];

    /// Locale code, matching the file name in `locales/`.
    pub fn code(self) -> &'static str {
        match self {
            Lang::ZhTw => "zh-TW",
            Lang::En => "en",
            Lang::Ja => "ja",
            Lang::Ko => "ko",
        }
    }

    /// Name shown in the language picker, always written in that language.
    pub fn native_name(self) -> &'static str {
        match self {
            Lang::ZhTw => "繁體中文",
            Lang::En => "English",
            Lang::Ja => "日本語",
            Lang::Ko => "한국어",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|l| l.code().eq_ignore_ascii_case(code))
    }

    /// Picks the closest supported language from the Windows display language.
    pub fn detect() -> Self {
        let locale = sys_locale::get_locale().unwrap_or_default().to_lowercase();
        match locale.split(['-', '_']).next() {
            Some("zh") => Lang::ZhTw,
            Some("ja") => Lang::Ja,
            Some("ko") => Lang::Ko,
            _ => Lang::En,
        }
    }

    pub fn activate(self) {
        rust_i18n::set_locale(self.code());
    }
}
