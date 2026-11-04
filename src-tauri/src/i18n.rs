//! Russian/English UI strings and shared text formatting.

use std::collections::HashMap;
use time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    Ru,
    En,
}

/// Explicit `ru`/`en` wins; `auto` follows the system locale (`ru*` → Russian).
pub fn resolve_lang(setting: &str, system: Option<&str>) -> Lang {
    match setting {
        "ru" => Lang::Ru,
        "en" => Lang::En,
        _ if system.is_some_and(|s| s.to_ascii_lowercase().starts_with("ru")) => Lang::Ru,
        _ => Lang::En,
    }
}

pub struct Strings {
    lang: Lang,
    map: HashMap<String, String>,
}

impl Strings {
    pub fn new(lang: Lang) -> Strings {
        let raw = match lang {
            Lang::Ru => include_str!("../locales/ru.json"),
            Lang::En => include_str!("../locales/en.json"),
        };
        let map = serde_json::from_str(raw).expect("bundled locale file is valid JSON");
        Strings { lang, map }
    }

    /// The string for `key`, or the key itself when missing.
    pub fn t<'a>(&'a self, key: &'a str) -> &'a str {
        self.map.get(key).map(String::as_str).unwrap_or(key)
    }

    pub fn lang(&self) -> Lang {
        self.lang
    }

    pub fn all(&self) -> &HashMap<String, String> {
        &self.map
    }
}

/// "14 мин" / "2h 14m" / "3 д 4 ч"; zero or negative → "скоро" / "soon".
/// Partial minutes round up so a pending reset never shows "0".
pub fn format_duration(d: Duration, lang: Lang) -> String {
    if d <= Duration::ZERO {
        return match lang {
            Lang::Ru => "скоро".into(),
            Lang::En => "soon".into(),
        };
    }
    let mins = (d.whole_seconds() + 59) / 60;
    let (days, hours, m) = (mins / 1440, mins % 1440 / 60, mins % 60);
    match (lang, days, hours) {
        (Lang::Ru, 0, 0) => format!("{m} мин"),
        (Lang::En, 0, 0) => format!("{m}m"),
        (Lang::Ru, 0, h) => format!("{h} ч {m} мин"),
        (Lang::En, 0, h) => format!("{h}h {m}m"),
        (Lang::Ru, dd, h) => format!("{dd} д {h} ч"),
        (Lang::En, dd, h) => format!("{dd}d {h}h"),
    }
}

fn capitalize(s: &str) -> String {
    let s = s.replace('_', " ");
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// Full quota name for the popup and notifications.
pub fn quota_name(key: &str, lang: Lang) -> String {
    let (five, seven) = match lang {
        Lang::Ru => ("5 часов", "7 дней"),
        Lang::En => ("5 hours", "7 days"),
    };
    match key {
        "five_hour" => five.into(),
        "seven_day" => seven.into(),
        _ => match key.strip_prefix("seven_day_") {
            Some(rest) => format!("{seven} · {}", capitalize(rest)),
            None => capitalize(key),
        },
    }
}

/// Compact quota name for the tray tooltip.
pub fn quota_short(key: &str, lang: Lang) -> String {
    match (key, lang) {
        ("five_hour", Lang::Ru) => "5ч".into(),
        ("five_hour", Lang::En) => "5h".into(),
        ("seven_day", Lang::Ru) => "7д".into(),
        ("seven_day", Lang::En) => "7d".into(),
        _ => quota_name(key, lang),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use time::Duration;

    #[test]
    fn lang_resolution() {
        assert_eq!(resolve_lang("auto", Some("ru-RU")), Lang::Ru);
        assert_eq!(resolve_lang("auto", Some("de-DE")), Lang::En);
        assert_eq!(resolve_lang("auto", None), Lang::En);
        assert_eq!(resolve_lang("ru", Some("en-US")), Lang::Ru);
        assert_eq!(resolve_lang("en", Some("ru-RU")), Lang::En);
    }

    #[test]
    fn format_durations() {
        assert_eq!(format_duration(Duration::minutes(14), Lang::Ru), "14 мин");
        assert_eq!(format_duration(Duration::minutes(134), Lang::En), "2h 14m");
        assert_eq!(format_duration(Duration::minutes(134), Lang::Ru), "2 ч 14 мин");
        assert_eq!(format_duration(Duration::hours(76), Lang::Ru), "3 д 4 ч");
        assert_eq!(format_duration(Duration::hours(76), Lang::En), "3d 4h");
        assert_eq!(format_duration(Duration::seconds(30), Lang::En), "1m");
    }

    #[test]
    fn format_negative_is_soon() {
        assert_eq!(format_duration(Duration::minutes(-3), Lang::Ru), "скоро");
        assert_eq!(format_duration(Duration::ZERO, Lang::En), "soon");
    }

    #[test]
    fn quota_names() {
        assert_eq!(quota_name("five_hour", Lang::Ru), "5 часов");
        assert_eq!(quota_name("seven_day", Lang::En), "7 days");
        assert_eq!(quota_name("seven_day_opus", Lang::Ru), "7 дней · Opus");
        assert_eq!(quota_name("seven_day_oauth_apps", Lang::En), "7 days · Oauth apps");
        assert_eq!(quota_name("monthly_cap", Lang::En), "Monthly cap");
        assert_eq!(quota_short("five_hour", Lang::Ru), "5ч");
        assert_eq!(quota_short("seven_day", Lang::En), "7d");
        assert_eq!(quota_short("seven_day_opus", Lang::En), "7 days · Opus");
    }

    #[test]
    fn locales_have_same_keys() {
        let keys = |lang| Strings::new(lang).all().keys().cloned().collect::<BTreeSet<_>>();
        let ru = keys(Lang::Ru);
        assert_eq!(ru, keys(Lang::En));
        for k in [
            "menu_open", "menu_refresh", "menu_autostart", "menu_quit", "tooltip_updated_ago",
            "err_no_token", "err_auth_expired", "err_rate_limited", "err_network", "err_server",
            "err_http", "err_settings", "popup_updated", "popup_refresh", "popup_stale",
            "popup_resets_in", "alert_threshold", "alert_reset", "app_name",
        ] {
            assert!(ru.contains(k), "missing key {k}");
        }
    }

    #[test]
    fn missing_key_falls_back_to_key() {
        assert_eq!(Strings::new(Lang::En).t("no_such_key"), "no_such_key");
        assert_eq!(Strings::new(Lang::Ru).t("menu_quit"), "Выход");
    }
}
