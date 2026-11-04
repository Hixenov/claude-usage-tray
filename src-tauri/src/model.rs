use serde::Serialize;
use time::{Duration, OffsetDateTime};

/// Length class of a quota window, derived from its API key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Period {
    FiveHour,
    SevenDay,
    Unknown,
}

impl Period {
    pub fn from_key(key: &str) -> Period {
        if key.starts_with("five_hour") {
            Period::FiveHour
        } else if key.starts_with("seven_day") {
            Period::SevenDay
        } else {
            Period::Unknown
        }
    }

    pub fn duration(self) -> Option<Duration> {
        match self {
            Period::FiveHour => Some(Duration::hours(5)),
            Period::SevenDay => Some(Duration::days(7)),
            Period::Unknown => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Quota {
    pub key: String,
    /// Percent used, as reported by the API (normally 0–100).
    pub utilization: f64,
    pub resets_at: Option<OffsetDateTime>,
    pub period: Period,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Profile {
    pub email: Option<String>,
    pub plan: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    pub quotas: Vec<Quota>,
    pub fetched_at: OffsetDateTime,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ApiError {
    NoToken,
    AuthExpired,
    RateLimited { retry_after: Option<u64> },
    Server(u16),
    Network(String),
    Http(u16),
    Malformed,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct AppState {
    pub snapshot: Option<Snapshot>,
    pub profile: Option<Profile>,
    pub last_error: Option<ApiError>,
    pub last_change: Option<OffsetDateTime>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::Duration;

    #[test]
    fn period_from_key() {
        assert_eq!(Period::from_key("five_hour"), Period::FiveHour);
        assert_eq!(Period::from_key("seven_day_opus"), Period::SevenDay);
        assert_eq!(Period::from_key("monthly"), Period::Unknown);
        assert_eq!(Period::FiveHour.duration(), Some(Duration::hours(5)));
        assert_eq!(Period::SevenDay.duration(), Some(Duration::days(7)));
        assert_eq!(Period::Unknown.duration(), None);
    }
}
