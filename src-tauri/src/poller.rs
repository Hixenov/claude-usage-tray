//! Poll scheduling: pure timing rules (tested) plus the async polling loop.

use crate::model::{ApiError, Snapshot};
use crate::settings::Settings;
use time::{Duration, OffsetDateTime};

const MIN_DELAY: Duration = Duration::seconds(30);
const POPUP_DELAY: Duration = Duration::seconds(60);
const BACKOFF_START: Duration = Duration::seconds(60);
const BACKOFF_MAX: Duration = Duration::seconds(900);
const ACTIVE_WINDOW: Duration = Duration::minutes(15);
const AFTER_RESET: Duration = Duration::seconds(30);

#[derive(Clone, Debug, PartialEq)]
pub struct PollCtx {
    pub now: OffsetDateTime,
    pub last_change: Option<OffsetDateTime>,
    pub next_reset: Option<OffsetDateTime>,
    pub popup_open: bool,
    pub backoff: Option<Duration>,
}

/// How long to wait before the next usage request.
pub fn next_delay(ctx: &PollCtx, s: &Settings) -> Duration {
    let delay = match ctx.backoff {
        Some(b) => b,
        None => {
            let base = if ctx.popup_open {
                POPUP_DELAY
            } else if ctx.last_change.is_some_and(|c| ctx.now - c <= ACTIVE_WINDOW) {
                Duration::seconds(s.poll_active_secs as i64)
            } else {
                Duration::seconds(s.poll_idle_secs as i64)
            };
            match ctx.next_reset {
                Some(r) => base.min(r + AFTER_RESET - ctx.now),
                None => base,
            }
        }
    };
    delay.max(MIN_DELAY)
}

/// Retry delay after an error; `None` means "use the regular schedule".
pub fn next_backoff(prev: Option<Duration>, err: &ApiError) -> Option<Duration> {
    match err {
        ApiError::AuthExpired => Some(BACKOFF_START),
        ApiError::RateLimited { retry_after: Some(n) } => {
            let secs = i64::try_from(*n).unwrap_or(i64::MAX / 2);
            Some(Duration::seconds(secs).clamp(MIN_DELAY, BACKOFF_MAX))
        }
        ApiError::RateLimited { retry_after: None } | ApiError::Server(_) | ApiError::Network(_) => {
            Some(prev.map_or(BACKOFF_START, |p: Duration| (p * 2_i32).clamp(BACKOFF_START, BACKOFF_MAX)))
        }
        ApiError::NoToken | ApiError::Http(_) | ApiError::Malformed => None,
    }
}

/// Earliest reset time still in the future.
pub fn next_reset(snap: &Snapshot, now: OffsetDateTime) -> Option<OffsetDateTime> {
    snap.quotas.iter().filter_map(|q| q.resets_at).filter(|r| *r > now).min()
}

/// True when any quota appeared, disappeared or changed utilization.
pub fn utilization_changed(prev: Option<&Snapshot>, next: &Snapshot) -> bool {
    let Some(prev) = prev else { return true };
    let pairs = |s: &Snapshot| s.quotas.iter().map(|q| (q.key.clone(), q.utilization)).collect::<Vec<_>>();
    pairs(prev) != pairs(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ApiError, Period, Quota, Snapshot};
    use crate::settings::Settings;
    use time::{macros::datetime, Duration, OffsetDateTime};

    const NOW: OffsetDateTime = datetime!(2026-11-04 12:00 UTC);

    fn ctx() -> PollCtx {
        PollCtx { now: NOW, last_change: None, next_reset: None, popup_open: false, backoff: None }
    }

    fn snap(items: &[(&str, f64, Option<OffsetDateTime>)]) -> Snapshot {
        Snapshot {
            quotas: items
                .iter()
                .map(|(k, u, r)| Quota {
                    key: k.to_string(),
                    utilization: *u,
                    resets_at: *r,
                    period: Period::from_key(k),
                })
                .collect(),
            fetched_at: NOW,
        }
    }

    #[test]
    fn idle_and_active_intervals() {
        let s = Settings::default();
        assert_eq!(next_delay(&ctx(), &s), Duration::seconds(600));
        let recent = PollCtx { last_change: Some(NOW - Duration::minutes(5)), ..ctx() };
        assert_eq!(next_delay(&recent, &s), Duration::seconds(180));
        let old = PollCtx { last_change: Some(NOW - Duration::minutes(20)), ..ctx() };
        assert_eq!(next_delay(&old, &s), Duration::seconds(600));
    }

    #[test]
    fn popup_open_is_one_minute() {
        let c = PollCtx { popup_open: true, ..ctx() };
        assert_eq!(next_delay(&c, &Settings::default()), Duration::seconds(60));
    }

    #[test]
    fn aligns_to_reset() {
        let c = PollCtx { next_reset: Some(NOW + Duration::minutes(2)), ..ctx() };
        assert_eq!(next_delay(&c, &Settings::default()), Duration::seconds(150));
    }

    #[test]
    fn never_below_30s() {
        let s = Settings::default();
        let past = PollCtx { next_reset: Some(NOW - Duration::minutes(1)), ..ctx() };
        assert_eq!(next_delay(&past, &s), Duration::seconds(30));
        let tiny = PollCtx { backoff: Some(Duration::seconds(5)), ..ctx() };
        assert_eq!(next_delay(&tiny, &s), Duration::seconds(30));
        let fast = Settings { poll_active_secs: 1, poll_idle_secs: 1, ..Settings::default() };
        assert_eq!(next_delay(&ctx(), &fast), Duration::seconds(30));
    }

    #[test]
    fn backoff_overrides_base() {
        let c = PollCtx { backoff: Some(Duration::seconds(240)), popup_open: true, ..ctx() };
        assert_eq!(next_delay(&c, &Settings::default()), Duration::seconds(240));
    }

    #[test]
    fn backoff_doubles_and_caps() {
        let e = ApiError::Network("x".into());
        assert_eq!(next_backoff(None, &e), Some(Duration::seconds(60)));
        assert_eq!(next_backoff(Some(Duration::seconds(60)), &e), Some(Duration::seconds(120)));
        assert_eq!(next_backoff(Some(Duration::seconds(600)), &e), Some(Duration::seconds(900)));
        assert_eq!(
            next_backoff(None, &ApiError::RateLimited { retry_after: None }),
            Some(Duration::seconds(60))
        );
        assert_eq!(next_backoff(None, &ApiError::Server(502)), Some(Duration::seconds(60)));
    }

    #[test]
    fn retry_after_respected() {
        let e = |n| ApiError::RateLimited { retry_after: Some(n) };
        assert_eq!(next_backoff(None, &e(120)), Some(Duration::seconds(120)));
        assert_eq!(next_backoff(None, &e(1)), Some(Duration::seconds(30)));
        assert_eq!(next_backoff(None, &e(99_999)), Some(Duration::seconds(900)));
    }

    #[test]
    fn auth_expired_retries_in_a_minute() {
        assert_eq!(
            next_backoff(Some(Duration::seconds(900)), &ApiError::AuthExpired),
            Some(Duration::seconds(60))
        );
    }

    #[test]
    fn no_backoff_for_http() {
        assert_eq!(next_backoff(None, &ApiError::Http(404)), None);
        assert_eq!(next_backoff(Some(Duration::seconds(120)), &ApiError::NoToken), None);
        assert_eq!(next_backoff(None, &ApiError::Malformed), None);
    }

    #[test]
    fn next_reset_is_nearest_future() {
        let s = snap(&[
            ("five_hour", 1.0, Some(NOW + Duration::hours(2))),
            ("seven_day", 1.0, Some(NOW + Duration::days(3))),
            ("seven_day_opus", 1.0, Some(NOW - Duration::hours(1))),
            ("x", 1.0, None),
        ]);
        assert_eq!(next_reset(&s, NOW), Some(NOW + Duration::hours(2)));
    }

    #[test]
    fn change_detection() {
        let a = snap(&[("five_hour", 42.0, None)]);
        assert!(utilization_changed(None, &a));
        assert!(!utilization_changed(Some(&a), &a.clone()));
        assert!(utilization_changed(Some(&a), &snap(&[("five_hour", 43.0, None)])));
        assert!(utilization_changed(Some(&a), &snap(&[("seven_day", 42.0, None)])));
    }
}
