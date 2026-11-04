//! Decides which notifications a new usage snapshot should trigger.

use crate::model::Snapshot;
use crate::pace::is_ahead;
use crate::settings::Settings;
use std::collections::HashSet;
use time::OffsetDateTime;

#[derive(Clone, Debug, PartialEq)]
pub enum Alert {
    Threshold {
        key: String,
        threshold: u8,
        utilization: f64,
        resets_at: Option<OffsetDateTime>,
    },
    Reset {
        key: String,
    },
}

/// Thresholds already handled, per quota window: `(key, threshold, resets_at unix)`.
#[derive(Default, Debug)]
pub struct FiredSet(HashSet<(String, u8, Option<i64>)>);

/// Utilization at which a quota counts as "nearly exhausted" for reset alerts.
const RESET_ALERT_MIN: f64 = 80.0;

pub fn evaluate(
    prev: Option<&Snapshot>,
    next: &Snapshot,
    fired: &mut FiredSet,
    s: &Settings,
    now: OffsetDateTime,
) -> Vec<Alert> {
    let mut thresholds = s.thresholds.clone();
    thresholds.sort_unstable();
    let mut alerts = Vec::new();
    for q in next
        .quotas
        .iter()
        .filter(|q| !s.hidden_quotas.contains(&q.key))
    {
        let old = prev.and_then(|p| p.quotas.iter().find(|o| o.key == q.key));
        if let Some(old) = old {
            if s.notify_reset && old.resets_at != q.resets_at && old.utilization >= RESET_ALERT_MIN
            {
                alerts.push(Alert::Reset { key: q.key.clone() });
            }
        }
        for &t in &thresholds {
            if q.utilization < f64::from(t) {
                continue;
            }
            let id = (q.key.clone(), t, q.resets_at.map(|r| r.unix_timestamp()));
            if !fired.0.insert(id) {
                continue;
            }
            if prev.is_some() && (!s.pace_alerts || is_ahead(q, now)) {
                alerts.push(Alert::Threshold {
                    key: q.key.clone(),
                    threshold: t,
                    utilization: q.utilization,
                    resets_at: q.resets_at,
                });
            }
        }
    }
    alerts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Period, Quota, Snapshot};
    use crate::settings::Settings;
    use time::{macros::datetime, Duration, OffsetDateTime};

    const NOW: OffsetDateTime = datetime!(2026-11-04 12:00 UTC);
    const R1: OffsetDateTime = datetime!(2026-11-04 14:00 UTC);
    const R2: OffsetDateTime = datetime!(2026-11-04 19:00 UTC);

    fn snap(items: &[(&str, f64, OffsetDateTime)]) -> Snapshot {
        Snapshot {
            quotas: items
                .iter()
                .map(|(k, u, r)| Quota {
                    key: k.to_string(),
                    utilization: *u,
                    resets_at: Some(*r),
                    period: Period::from_key(k),
                })
                .collect(),
            fetched_at: NOW,
        }
    }

    fn thresholds(alerts: &[Alert]) -> Vec<u8> {
        alerts
            .iter()
            .filter_map(|a| match a {
                Alert::Threshold { threshold, .. } => Some(*threshold),
                _ => None,
            })
            .collect()
    }

    /// Feeds snapshots in order and returns the alerts of the last step.
    fn run(steps: &[Snapshot], s: &Settings) -> Vec<Alert> {
        let mut fired = FiredSet::default();
        let mut prev: Option<&Snapshot> = None;
        let mut last = vec![];
        for next in steps {
            last = evaluate(prev, next, &mut fired, s, NOW);
            prev = Some(next);
        }
        last
    }

    #[test]
    fn first_snapshot_is_silent_but_marks_fired() {
        let s = Settings::default();
        let mut f = FiredSet::default();
        let a = snap(&[("five_hour", 85.0, R1)]);
        assert!(evaluate(None, &a, &mut f, &s, NOW).is_empty());
        let b = snap(&[("five_hour", 86.0, R1)]);
        assert!(evaluate(Some(&a), &b, &mut f, &s, NOW).is_empty());
    }

    #[test]
    fn crossing_threshold_fires_once() {
        let s = Settings::default();
        let mut f = FiredSet::default();
        let (a, b, c, d) = (
            snap(&[("five_hour", 70.0, R1)]),
            snap(&[("five_hour", 82.0, R1)]),
            snap(&[("five_hour", 84.0, R1)]),
            snap(&[("five_hour", 96.0, R1)]),
        );
        evaluate(None, &a, &mut f, &s, NOW);
        assert_eq!(
            thresholds(&evaluate(Some(&a), &b, &mut f, &s, NOW)),
            vec![80]
        );
        assert!(evaluate(Some(&b), &c, &mut f, &s, NOW).is_empty());
        assert_eq!(
            thresholds(&evaluate(Some(&c), &d, &mut f, &s, NOW)),
            vec![95]
        );
    }

    #[test]
    fn jump_over_two_thresholds_fires_both() {
        let steps = [
            snap(&[("seven_day", 70.0, R1)]),
            snap(&[("seven_day", 97.0, R1)]),
        ];
        assert_eq!(thresholds(&run(&steps, &Settings::default())), vec![80, 95]);
    }

    #[test]
    fn rearms_after_reset() {
        let steps = [
            snap(&[("five_hour", 85.0, R1)]),
            snap(&[("five_hour", 10.0, R2)]),
            snap(&[("five_hour", 85.0, R2)]),
        ];
        assert_eq!(thresholds(&run(&steps, &Settings::default())), vec![80]);
    }

    #[test]
    fn reset_alert_only_if_was_high() {
        let s = Settings::default();
        let high = run(
            &[
                snap(&[("five_hour", 90.0, R1)]),
                snap(&[("five_hour", 2.0, R2)]),
            ],
            &s,
        );
        assert_eq!(
            high,
            vec![Alert::Reset {
                key: "five_hour".into()
            }]
        );
        let low = run(
            &[
                snap(&[("five_hour", 50.0, R1)]),
                snap(&[("five_hour", 2.0, R2)]),
            ],
            &s,
        );
        assert!(low.is_empty());
        let off = Settings {
            notify_reset: false,
            ..Settings::default()
        };
        assert!(run(
            &[
                snap(&[("five_hour", 90.0, R1)]),
                snap(&[("five_hour", 2.0, R2)])
            ],
            &off
        )
        .is_empty());
    }

    #[test]
    fn pace_mode_suppresses_when_not_ahead() {
        let s = Settings {
            pace_alerts: true,
            ..Settings::default()
        };
        // 5h window resetting in 30 min → 90% of time elapsed; 82% used is behind pace.
        let r = NOW + Duration::minutes(30);
        let behind = run(
            &[
                snap(&[("five_hour", 70.0, r)]),
                snap(&[("five_hour", 82.0, r)]),
            ],
            &s,
        );
        assert!(behind.is_empty());
        // Resetting in 4.5h → 10% elapsed; 82% used is ahead of pace.
        let r = NOW + Duration::minutes(270);
        let ahead = run(
            &[
                snap(&[("five_hour", 70.0, r)]),
                snap(&[("five_hour", 82.0, r)]),
            ],
            &s,
        );
        assert_eq!(thresholds(&ahead), vec![80]);
    }

    #[test]
    fn hidden_quota_ignored() {
        let s = Settings {
            hidden_quotas: vec!["five_hour".into()],
            ..Settings::default()
        };
        let steps = [
            snap(&[("five_hour", 70.0, R1)]),
            snap(&[("five_hour", 99.0, R1)]),
        ];
        assert!(run(&steps, &s).is_empty());
    }
}
