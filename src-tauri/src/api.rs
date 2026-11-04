//! Anthropic OAuth usage API: response parsing, status mapping, HTTP client.
//! The only host this app ever contacts is `BASE_URL`.

use crate::credentials;
use crate::model::{ApiError, Period, Profile, Quota};
use std::time::Duration;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

pub const BASE_URL: &str = "https://api.anthropic.com";
const BETA: &str = "oauth-2025-04-20";

/// Every top-level object holding both `utilization` and `resets_at` is a quota.
/// Order: `five_hour`, `seven_day`, then the rest alphabetically.
pub fn parse_usage(body: &str) -> Result<Vec<Quota>, ApiError> {
    let v: serde_json::Value = serde_json::from_str(body).map_err(|_| ApiError::Malformed)?;
    let obj = v.as_object().ok_or(ApiError::Malformed)?;
    let mut quotas: Vec<Quota> = obj
        .iter()
        .filter_map(|(key, val)| {
            let o = val.as_object()?;
            if !o.contains_key("resets_at") {
                return None;
            }
            let utilization = o.get("utilization")?.as_f64()?;
            let resets_at = o
                .get("resets_at")
                .and_then(|r| r.as_str())
                .and_then(|r| OffsetDateTime::parse(r, &Rfc3339).ok());
            Some(Quota {
                key: key.clone(),
                utilization,
                resets_at,
                period: Period::from_key(key),
            })
        })
        .collect();
    let rank = |k: &str| match k {
        "five_hour" => 0,
        "seven_day" => 1,
        _ => 2,
    };
    quotas.sort_by(|a, b| {
        rank(&a.key)
            .cmp(&rank(&b.key))
            .then_with(|| a.key.cmp(&b.key))
    });
    Ok(quotas)
}

pub fn parse_profile(body: &str) -> Option<Profile> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let account = v.get("account")?;
    let flag = |k: &str| account.get(k).and_then(|b| b.as_bool()) == Some(true);
    let plan = if flag("has_claude_max") {
        Some("Max".to_string())
    } else if flag("has_claude_pro") {
        Some("Pro".to_string())
    } else {
        None
    };
    Some(Profile {
        email: account
            .get("email")
            .and_then(|e| e.as_str())
            .map(String::from),
        plan,
    })
}

/// `None` for success (2xx); otherwise the error the status represents.
pub fn map_status(status: u16, retry_after: Option<&str>) -> Option<ApiError> {
    match status {
        200..=299 => None,
        401 => Some(ApiError::AuthExpired),
        429 => Some(ApiError::RateLimited {
            retry_after: retry_after.and_then(|v| v.trim().parse::<u64>().ok()),
        }),
        500..=599 => Some(ApiError::Server(status)),
        _ => Some(ApiError::Http(status)),
    }
}

pub struct Client {
    http: reqwest::Client,
}

impl Default for Client {
    fn default() -> Self {
        Self::new()
    }
}

impl Client {
    pub fn new() -> Client {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .user_agent(concat!("claude-usage-tray/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("HTTP client configuration is static");
        Client { http }
    }

    async fn get(&self, path: &str) -> Result<String, ApiError> {
        let token = credentials::load().map_err(|_| ApiError::NoToken)?;
        let resp = self
            .http
            .get(format!("{BASE_URL}{path}"))
            .bearer_auth(&token.access_token)
            .header("anthropic-beta", BETA)
            .send()
            .await
            .map_err(|e| ApiError::Network(e.without_url().to_string()))?;
        let retry_after = resp
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|h| h.to_str().ok())
            .map(String::from);
        if let Some(err) = map_status(resp.status().as_u16(), retry_after.as_deref()) {
            return Err(err);
        }
        resp.text()
            .await
            .map_err(|e| ApiError::Network(e.without_url().to_string()))
    }

    pub async fn fetch_usage(&self) -> Result<Vec<Quota>, ApiError> {
        parse_usage(&self.get("/api/oauth/usage").await?)
    }

    pub async fn fetch_profile(&self) -> Option<Profile> {
        parse_profile(&self.get("/api/oauth/profile").await.ok()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ApiError, Period};

    #[test]
    fn parses_full_usage() {
        let q = parse_usage(include_str!("../tests/fixtures/usage_full.json")).unwrap();
        assert_eq!(
            q.iter().map(|q| q.key.as_str()).collect::<Vec<_>>(),
            ["five_hour", "seven_day", "seven_day_opus"]
        );
        assert_eq!(q[0].utilization, 42.0);
        assert_eq!(q[0].period, Period::FiveHour);
        assert!(q[0].resets_at.is_some());
    }

    #[test]
    fn null_fields() {
        let q = parse_usage(
            r#"{"five_hour":{"utilization":10,"resets_at":null},"seven_day_sonnet":{"utilization":null,"resets_at":null}}"#,
        )
        .unwrap();
        assert_eq!(q.len(), 1);
        assert_eq!(q[0].resets_at, None);
    }

    #[test]
    fn ignores_non_quota_fields() {
        let q = parse_usage(include_str!("../tests/fixtures/usage_extra.json")).unwrap();
        assert_eq!(q.len(), 1);
        assert_eq!(q[0].key, "seven_day");
    }

    #[test]
    fn unknown_quota_key_kept() {
        let q = parse_usage(
            r#"{"seven_day_fable":{"utilization":3,"resets_at":"2026-11-10T00:00:00Z"}}"#,
        )
        .unwrap();
        assert_eq!(q[0].key, "seven_day_fable");
    }

    #[test]
    fn unparseable_reset_is_none() {
        let q = parse_usage(r#"{"five_hour":{"utilization":1,"resets_at":"tomorrow"}}"#).unwrap();
        assert_eq!(q[0].resets_at, None);
    }

    #[test]
    fn invalid_json_is_malformed() {
        assert_eq!(parse_usage("<html>"), Err(ApiError::Malformed));
        assert_eq!(parse_usage("[1,2]"), Err(ApiError::Malformed));
    }

    #[test]
    fn status_mapping() {
        assert_eq!(map_status(200, None), None);
        assert_eq!(map_status(401, None), Some(ApiError::AuthExpired));
        assert_eq!(
            map_status(429, Some("120")),
            Some(ApiError::RateLimited {
                retry_after: Some(120)
            })
        );
        assert_eq!(map_status(503, None), Some(ApiError::Server(503)));
        assert_eq!(map_status(404, None), Some(ApiError::Http(404)));
    }

    #[test]
    fn retry_after_garbage_is_none() {
        assert_eq!(
            map_status(429, Some("Wed, 21 Oct 2026 07:28:00 GMT")),
            Some(ApiError::RateLimited { retry_after: None })
        );
        assert_eq!(
            map_status(429, Some("-5")),
            Some(ApiError::RateLimited { retry_after: None })
        );
    }

    #[test]
    fn parses_profile() {
        let p = parse_profile(include_str!("../tests/fixtures/profile.json")).unwrap();
        assert_eq!(p.email.as_deref(), Some("user@example.com"));
        assert_eq!(p.plan.as_deref(), Some("Pro"));
        let max = parse_profile(
            r#"{"account":{"email":"a@b.c","has_claude_max":true,"has_claude_pro":true}}"#,
        )
        .unwrap();
        assert_eq!(max.plan.as_deref(), Some("Max"));
        assert_eq!(parse_profile("nope"), None);
    }

    #[tokio::test]
    #[ignore = "hits the live API with the local Claude Code token"]
    async fn live_usage() {
        let q = Client::new().fetch_usage().await.unwrap();
        println!("live quotas: {}", q.len());
        assert!(q.len() >= 2);
    }
}
