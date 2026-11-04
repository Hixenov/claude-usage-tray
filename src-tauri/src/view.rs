//! Ready-to-display data for the popup and the tray tooltip (all text localized here).

use crate::i18n::{format_duration, quota_name, quota_short, Strings};
use crate::model::{ApiError, AppState, Quota};
use crate::pace::{elapsed_fraction, is_ahead};
use crate::settings::Settings;
use crate::tray_icon::{bar_color, Bar};
use serde::Serialize;
use time::{Duration, OffsetDateTime};

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct QuotaView {
    pub key: String,
    pub name: String,
    pub percent: u8,
    /// Fraction of the window already elapsed (0–1), for the time marker.
    pub elapsed: Option<f64>,
    pub color: &'static str,
    pub reset_text: Option<String>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct PopupView {
    pub email: Option<String>,
    pub plan: Option<String>,
    pub quotas: Vec<QuotaView>,
    pub updated_text: Option<String>,
    pub stale: bool,
    pub error_text: Option<String>,
}

/// Replaces `{name}` placeholders in a localized template.
pub fn fill(template: &str, vars: &[(&str, &str)]) -> String {
    vars.iter().fold(template.to_string(), |acc, (k, v)| acc.replace(&format!("{{{k}}}"), v))
}

fn visible<'a>(st: &'a AppState, s: &'a Settings) -> impl Iterator<Item = &'a Quota> {
    st.snapshot
        .iter()
        .flat_map(|snap| snap.quotas.iter())
        .filter(|q| !s.hidden_quotas.contains(&q.key))
}

fn age(st: &AppState, now: OffsetDateTime) -> Option<Duration> {
    st.snapshot.as_ref().map(|snap| now - snap.fetched_at)
}

fn is_stale(st: &AppState, s: &Settings, now: OffsetDateTime) -> bool {
    age(st, now).is_some_and(|a| a > Duration::seconds(2 * s.poll_idle_secs as i64))
}

pub fn error_text(e: &ApiError, strings: &Strings) -> String {
    match e {
        ApiError::NoToken => strings.t("err_no_token").to_string(),
        ApiError::AuthExpired => strings.t("err_auth_expired").to_string(),
        ApiError::RateLimited { .. } => strings.t("err_rate_limited").to_string(),
        ApiError::Network(_) => strings.t("err_network").to_string(),
        ApiError::Server(code) => fill(strings.t("err_server"), &[("code", &code.to_string())]),
        ApiError::Http(code) => fill(strings.t("err_http"), &[("code", &code.to_string())]),
        ApiError::Malformed => strings.t("err_malformed").to_string(),
    }
}

pub fn build_view(st: &AppState, s: &Settings, strings: &Strings, now: OffsetDateTime) -> PopupView {
    let lang = strings.lang();
    let quotas = visible(st, s)
        .map(|q| QuotaView {
            key: q.key.clone(),
            name: quota_name(&q.key, lang),
            percent: q.utilization.round().clamp(0.0, 100.0) as u8,
            elapsed: elapsed_fraction(q, now),
            color: bar_color(q.utilization, is_ahead(q, now)).css(),
            reset_text: q.resets_at.map(|r| {
                fill(strings.t("popup_resets_in"), &[("time", &format_duration(r - now, lang))])
            }),
        })
        .collect();
    let updated_text = age(st, now).map(|a| {
        if a < Duration::minutes(1) {
            strings.t("popup_updated_now").to_string()
        } else {
            fill(strings.t("popup_updated"), &[("time", &format_duration(a, lang))])
        }
    });
    PopupView {
        email: st.profile.as_ref().and_then(|p| p.email.clone()),
        plan: st.profile.as_ref().and_then(|p| p.plan.clone()),
        quotas,
        updated_text,
        stale: is_stale(st, s, now),
        error_text: st.last_error.as_ref().map(|e| error_text(e, strings)),
    }
}

pub fn tooltip_text(st: &AppState, s: &Settings, strings: &Strings, now: OffsetDateTime) -> String {
    let lang = strings.lang();
    let mut out = strings.t("app_name").to_string();
    let parts: Vec<String> = s
        .tray_bars
        .iter()
        .filter_map(|key| visible(st, s).find(|q| &q.key == key))
        .map(|q| format!("{}: {}%", quota_short(&q.key, lang), q.utilization.round().clamp(0.0, 100.0)))
        .collect();
    if !parts.is_empty() {
        out.push('\n');
        out.push_str(&parts.join(" · "));
        if is_stale(st, s, now) {
            let ago = format_duration(age(st, now).unwrap_or(Duration::ZERO), lang);
            out.push(' ');
            out.push_str(&fill(strings.t("tooltip_updated_ago"), &[("time", &ago)]));
        }
    }
    if let Some(e) = &st.last_error {
        out.push('\n');
        out.push_str(&error_text(e, strings));
    }
    out
}

/// Bars for the tray icon, in `settings.tray_bars` order (`None` = quota absent).
pub fn tray_bars(st: &AppState, s: &Settings, now: OffsetDateTime) -> Vec<Option<Bar>> {
    s.tray_bars
        .iter()
        .map(|key| {
            visible(st, s).find(|q| &q.key == key).map(|q| Bar {
                utilization: q.utilization,
                color: bar_color(q.utilization, is_ahead(q, now)),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::{Lang, Strings};
    use crate::model::{ApiError, AppState, Period, Profile, Quota, Snapshot};
    use crate::settings::Settings;
    use time::{macros::datetime, Duration, OffsetDateTime};

    const NOW: OffsetDateTime = datetime!(2026-11-04 12:00 UTC);

    fn q(key: &str, utilization: f64, resets_at: Option<OffsetDateTime>) -> Quota {
        Quota { key: key.into(), utilization, resets_at, period: Period::from_key(key) }
    }

    fn state(quotas: Vec<Quota>, fetched_ago_min: i64) -> AppState {
        AppState {
            snapshot: Some(Snapshot { quotas, fetched_at: NOW - Duration::minutes(fetched_ago_min) }),
            profile: Some(Profile { email: Some("user@example.com".into()), plan: Some("Pro".into()) }),
            ..AppState::default()
        }
    }

    fn default_state() -> AppState {
        state(
            vec![
                q("five_hour", 42.0, Some(NOW + Duration::minutes(134))),
                q("seven_day", 18.0, Some(NOW + Duration::days(3))),
            ],
            0,
        )
    }

    #[test]
    fn view_basic() {
        let v = build_view(&default_state(), &Settings::default(), &Strings::new(Lang::Ru), NOW);
        assert_eq!(v.email.as_deref(), Some("user@example.com"));
        assert_eq!(v.plan.as_deref(), Some("Pro"));
        assert_eq!(v.quotas.len(), 2);
        let five = &v.quotas[0];
        assert_eq!(five.name, "5 часов");
        assert_eq!(five.percent, 42);
        assert_eq!(five.reset_text.as_deref(), Some("сброс через 2 ч 14 мин"));
        assert_eq!(five.color, "blue");
        assert!(five.elapsed.is_some());
        assert!(!v.stale);
        assert_eq!(v.error_text, None);
    }

    #[test]
    fn percent_is_clamped() {
        let st = state(vec![q("five_hour", 130.0, None), q("seven_day", -2.0, None)], 0);
        let v = build_view(&st, &Settings::default(), &Strings::new(Lang::En), NOW);
        assert_eq!(v.quotas[0].percent, 100);
        assert_eq!(v.quotas[0].color, "red");
        assert_eq!(v.quotas[1].percent, 0);
    }

    #[test]
    fn no_reset_no_marker() {
        let st = state(vec![q("five_hour", 10.0, None)], 0);
        let v = build_view(&st, &Settings::default(), &Strings::new(Lang::En), NOW);
        assert_eq!(v.quotas[0].reset_text, None);
        assert_eq!(v.quotas[0].elapsed, None);
    }

    #[test]
    fn past_reset_says_soon() {
        let st = state(vec![q("five_hour", 10.0, Some(NOW - Duration::minutes(2)))], 0);
        let v = build_view(&st, &Settings::default(), &Strings::new(Lang::En), NOW);
        assert_eq!(v.quotas[0].reset_text.as_deref(), Some("resets in soon"));
        assert_eq!(v.quotas[0].elapsed, Some(1.0));
    }

    #[test]
    fn ahead_of_pace_is_red() {
        // 5h window, 90% left (reset in 4.5h) → only 10% elapsed, 50% used.
        let st = state(vec![q("five_hour", 50.0, Some(NOW + Duration::minutes(270)))], 0);
        let v = build_view(&st, &Settings::default(), &Strings::new(Lang::En), NOW);
        assert_eq!(v.quotas[0].color, "red");
    }

    #[test]
    fn hidden_quotas_excluded() {
        let s = Settings { hidden_quotas: vec!["seven_day".into()], ..Settings::default() };
        let v = build_view(&default_state(), &s, &Strings::new(Lang::En), NOW);
        assert_eq!(v.quotas.len(), 1);
    }

    #[test]
    fn error_keeps_last_snapshot() {
        let mut st = default_state();
        st.last_error = Some(ApiError::Network("boom".into()));
        let v = build_view(&st, &Settings::default(), &Strings::new(Lang::En), NOW);
        assert_eq!(v.quotas.len(), 2);
        assert_eq!(v.error_text.as_deref(), Some("Can't reach api.anthropic.com"));
    }

    #[test]
    fn stale_flag() {
        let s = Settings::default();
        let en = Strings::new(Lang::En);
        let mut st = default_state();
        st.snapshot.as_mut().unwrap().fetched_at = NOW - Duration::minutes(21);
        assert!(build_view(&st, &s, &en, NOW).stale);
        st.snapshot.as_mut().unwrap().fetched_at = NOW - Duration::minutes(5);
        let v = build_view(&st, &s, &en, NOW);
        assert!(!v.stale);
        assert_eq!(v.updated_text.as_deref(), Some("Updated 5m ago"));
    }

    #[test]
    fn updated_just_now() {
        let v = build_view(&default_state(), &Settings::default(), &Strings::new(Lang::Ru), NOW);
        assert_eq!(v.updated_text.as_deref(), Some("Обновлено только что"));
    }

    #[test]
    fn empty_state_has_no_quotas() {
        let v = build_view(&AppState::default(), &Settings::default(), &Strings::new(Lang::En), NOW);
        assert!(v.quotas.is_empty());
        assert_eq!(v.updated_text, None);
        assert!(!v.stale);
    }

    #[test]
    fn tooltip_format() {
        let t = tooltip_text(&default_state(), &Settings::default(), &Strings::new(Lang::Ru), NOW);
        assert!(t.starts_with("Claude Usage Monitor"));
        assert!(t.contains("5ч: 42% · 7д: 18%"), "{t}");
        assert!(!t.contains("обновлено"));
    }

    #[test]
    fn tooltip_stale_and_error() {
        let mut st = default_state();
        st.snapshot.as_mut().unwrap().fetched_at = NOW - Duration::minutes(25);
        st.last_error = Some(ApiError::Server(503));
        let t = tooltip_text(&st, &Settings::default(), &Strings::new(Lang::En), NOW);
        assert!(t.contains("5h: 42% · 7d: 18%"), "{t}");
        assert!(t.contains("(updated 25m ago)"), "{t}");
        assert!(t.contains("Server error (503)"), "{t}");
    }

    #[test]
    fn tooltip_no_token() {
        let st = AppState { last_error: Some(ApiError::NoToken), ..AppState::default() };
        let t = tooltip_text(&st, &Settings::default(), &Strings::new(Lang::En), NOW);
        assert!(t.contains("claude auth login"), "{t}");
    }

    #[test]
    fn error_texts() {
        let en = Strings::new(Lang::En);
        assert_eq!(error_text(&ApiError::Http(404), &en), "HTTP error 404");
        assert!(error_text(&ApiError::AuthExpired, &en).contains("claude auth login"));
        assert_eq!(error_text(&ApiError::Malformed, &en), "Unexpected response from api.anthropic.com");
    }

    #[test]
    fn tray_bars_follow_settings() {
        let s = Settings { tray_bars: vec!["seven_day".into(), "missing".into()], ..Settings::default() };
        let bars = tray_bars(&default_state(), &s, NOW);
        assert_eq!(bars.len(), 2);
        assert_eq!(bars[0].as_ref().map(|b| b.utilization), Some(18.0));
        assert!(bars[1].is_none());
    }
}
