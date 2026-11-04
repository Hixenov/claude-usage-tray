use crate::model::Quota;
use time::OffsetDateTime;

/// Fraction (0.0–1.0) of the quota's window that has already elapsed.
/// `None` when the window length or reset time is unknown.
pub fn elapsed_fraction(q: &Quota, now: OffsetDateTime) -> Option<f64> {
    let len = q.period.duration()?;
    let start = q.resets_at? - len;
    let frac = (now - start).as_seconds_f64() / len.as_seconds_f64();
    Some(frac.clamp(0.0, 1.0))
}

/// True when usage runs ahead of elapsed time in the window.
pub fn is_ahead(q: &Quota, now: OffsetDateTime) -> bool {
    elapsed_fraction(q, now).is_some_and(|f| q.utilization / 100.0 > f)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Period, Quota};
    use time::{macros::datetime, Duration, OffsetDateTime};

    const NOW: OffsetDateTime = datetime!(2026-11-04 12:00 UTC);

    fn quota(key: &str, utilization: f64, resets_at: Option<OffsetDateTime>) -> Quota {
        Quota { key: key.into(), utilization, resets_at, period: Period::from_key(key) }
    }

    #[test]
    fn fraction_midway() {
        let q = quota("five_hour", 10.0, Some(NOW + Duration::minutes(150)));
        assert!((elapsed_fraction(&q, NOW).unwrap() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn fraction_clamped_after_reset() {
        let q = quota("five_hour", 10.0, Some(NOW - Duration::minutes(5)));
        assert_eq!(elapsed_fraction(&q, NOW), Some(1.0));
    }

    #[test]
    fn fraction_clamped_before_start() {
        let q = quota("five_hour", 10.0, Some(NOW + Duration::hours(9)));
        assert_eq!(elapsed_fraction(&q, NOW), Some(0.0));
    }

    #[test]
    fn no_fraction_without_reset_or_period() {
        assert_eq!(elapsed_fraction(&quota("five_hour", 10.0, None), NOW), None);
        assert_eq!(
            elapsed_fraction(&quota("monthly", 10.0, Some(NOW + Duration::hours(1))), NOW),
            None
        );
    }

    #[test]
    fn ahead_of_pace() {
        let q = quota("five_hour", 60.0, Some(NOW + Duration::minutes(150)));
        assert!(is_ahead(&q, NOW));
        assert!(!is_ahead(&quota("five_hour", 40.0, Some(NOW + Duration::minutes(150))), NOW));
        assert!(!is_ahead(&quota("five_hour", 99.0, None), NOW));
    }
}
