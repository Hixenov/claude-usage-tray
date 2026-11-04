//! Locating the Claude Code OAuth token. The token is re-read on every call
//! and is never stored, logged or printed.

use std::fmt;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub enum CredError {
    NotFound,
    Malformed,
    Io(String),
}

#[derive(Clone, PartialEq)]
pub struct Credentials {
    pub access_token: String,
    pub expires_at_ms: Option<i64>,
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credentials")
            .field("access_token", &"<redacted>")
            .field("expires_at_ms", &self.expires_at_ms)
            .finish()
    }
}

/// Source of the credentials JSON stored in an OS secret store.
pub trait Keychain {
    fn read(&self) -> Option<String>;
}

/// macOS login Keychain via `/usr/bin/security`; always `None` elsewhere.
pub struct SystemKeychain;

impl Keychain for SystemKeychain {
    #[cfg(target_os = "macos")]
    fn read(&self) -> Option<String> {
        let out = std::process::Command::new("/usr/bin/security")
            .args(["find-generic-password", "-s", "Claude Code-credentials", "-w"])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let s = String::from_utf8(out.stdout).ok()?.trim().to_string();
        (!s.is_empty()).then_some(s)
    }

    #[cfg(not(target_os = "macos"))]
    fn read(&self) -> Option<String> {
        None
    }
}

pub fn parse_credentials_json(s: &str) -> Result<Credentials, CredError> {
    let v: serde_json::Value = serde_json::from_str(s).map_err(|_| CredError::Malformed)?;
    let oauth = v.get("claudeAiOauth").ok_or(CredError::Malformed)?;
    let token = oauth
        .get("accessToken")
        .and_then(|t| t.as_str())
        .filter(|t| !t.is_empty())
        .ok_or(CredError::Malformed)?;
    Ok(Credentials {
        access_token: token.to_string(),
        expires_at_ms: oauth.get("expiresAt").and_then(|e| e.as_i64()),
    })
}

/// `$CLAUDE_CONFIG_DIR`, otherwise `~/.claude`.
pub fn config_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("CLAUDE_CONFIG_DIR").filter(|d| !d.is_empty()) {
        return PathBuf::from(d);
    }
    home_dir().join(".claude")
}

pub(crate) fn home_dir() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_default()
}

pub fn find_credentials(
    env_token: Option<String>,
    keychain: &dyn Keychain,
    config_dir: &Path,
) -> Result<Credentials, CredError> {
    if let Some(t) = env_token.map(|t| t.trim().to_string()).filter(|t| !t.is_empty()) {
        return Ok(Credentials { access_token: t, expires_at_ms: None });
    }
    if let Some(json) = keychain.read() {
        return parse_credentials_json(&json);
    }
    match std::fs::read_to_string(config_dir.join(".credentials.json")) {
        Ok(s) => parse_credentials_json(&s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(CredError::NotFound),
        Err(e) => Err(CredError::Io(e.to_string())),
    }
}

pub fn load() -> Result<Credentials, CredError> {
    find_credentials(std::env::var("CLAUDE_CODE_OAUTH_TOKEN").ok(), &SystemKeychain, &config_dir())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    struct FakeKeychain(Option<String>);
    impl Keychain for FakeKeychain {
        fn read(&self) -> Option<String> {
            self.0.clone()
        }
    }

    fn cred_json(token: &str) -> String {
        format!(r#"{{"claudeAiOauth":{{"accessToken":"{token}","expiresAt":1767225600000}}}}"#)
    }

    fn temp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cum-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn dir_with_file(name: &str, token: &str) -> PathBuf {
        let d = temp_dir(name);
        fs::write(d.join(".credentials.json"), cred_json(token)).unwrap();
        d
    }

    #[test]
    fn parses_token_and_expiry() {
        let c = parse_credentials_json(
            r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat-x","expiresAt":1767225600000}}"#,
        )
        .unwrap();
        assert_eq!(c.access_token, "sk-ant-oat-x");
        assert_eq!(c.expires_at_ms, Some(1767225600000));
    }

    #[test]
    fn missing_or_empty_token_is_malformed() {
        assert!(matches!(parse_credentials_json(r#"{"claudeAiOauth":{}}"#), Err(CredError::Malformed)));
        assert!(matches!(
            parse_credentials_json(r#"{"claudeAiOauth":{"accessToken":""}}"#),
            Err(CredError::Malformed)
        ));
        assert!(matches!(parse_credentials_json(r#"{"other":1}"#), Err(CredError::Malformed)));
    }

    #[test]
    fn truncated_json_is_malformed() {
        assert!(matches!(
            parse_credentials_json(r#"{"claudeAiOauth":{"accessTo"#),
            Err(CredError::Malformed)
        ));
    }

    #[test]
    fn env_wins_over_keychain_and_file() {
        let d = dir_with_file("env", "f");
        let c = find_credentials(Some("e".into()), &FakeKeychain(Some(cred_json("k"))), &d).unwrap();
        assert_eq!(c.access_token, "e");
    }

    #[test]
    fn empty_env_is_ignored() {
        let d = dir_with_file("emptyenv", "f");
        let c = find_credentials(Some("  ".into()), &FakeKeychain(None), &d).unwrap();
        assert_eq!(c.access_token, "f");
    }

    #[test]
    fn keychain_wins_over_file() {
        let d = dir_with_file("kc", "f");
        let c = find_credentials(None, &FakeKeychain(Some(cred_json("k"))), &d).unwrap();
        assert_eq!(c.access_token, "k");
    }

    #[test]
    fn falls_back_to_file() {
        let d = dir_with_file("file", "f");
        let c = find_credentials(None, &FakeKeychain(None), &d).unwrap();
        assert_eq!(c.access_token, "f");
    }

    #[test]
    fn nothing_found() {
        let d = temp_dir("none");
        assert!(matches!(find_credentials(None, &FakeKeychain(None), &d), Err(CredError::NotFound)));
    }

    #[test]
    fn debug_hides_token() {
        let c = Credentials { access_token: "secret".into(), expires_at_ms: None };
        assert!(!format!("{c:?}").contains("secret"));
    }
}
