//! Optional user settings file. The app never creates or edits it.

use serde::Deserialize;
use std::path::PathBuf;

pub const FILE_NAME: &str = "usage-monitor-settings.json";

#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub language: String,
    pub thresholds: Vec<u8>,
    pub notify_reset: bool,
    pub pace_alerts: bool,
    pub poll_active_secs: u64,
    pub poll_idle_secs: u64,
    pub hidden_quotas: Vec<String>,
    pub tray_bars: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            language: "auto".into(),
            thresholds: vec![80, 95],
            notify_reset: true,
            pace_alerts: false,
            poll_active_secs: 180,
            poll_idle_secs: 600,
            hidden_quotas: vec![],
            tray_bars: vec!["five_hour".into(), "seven_day".into()],
        }
    }
}

/// `$CLAUDE_CONFIG_DIR`, the executable's folder, `~/.claude` — in that order.
pub fn search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(d) = std::env::var_os("CLAUDE_CONFIG_DIR").filter(|d| !d.is_empty()) {
        dirs.push(PathBuf::from(d));
    }
    if let Some(d) = std::env::current_exe().ok().and_then(|p| p.parent().map(PathBuf::from)) {
        dirs.push(d);
    }
    dirs.push(crate::credentials::home_dir().join(".claude"));
    dirs
}

/// Uses the first settings file found. A broken file yields the defaults plus
/// an error message for the user.
pub fn load_from(dirs: &[PathBuf]) -> (Settings, Option<String>) {
    for d in dirs {
        let path = d.join(FILE_NAME);
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return (Settings::default(), Some(format!("{}: {e}", path.display()))),
        };
        return match serde_json::from_str::<Settings>(&text) {
            Ok(s) => (s, None),
            Err(e) => (Settings::default(), Some(format!("{}: {e}", path.display()))),
        };
    }
    (Settings::default(), None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn dir(name: &str, content: Option<&str>) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cum-settings-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        if let Some(c) = content {
            fs::write(d.join(FILE_NAME), c).unwrap();
        }
        d
    }

    #[test]
    fn defaults_match_spec() {
        let s = Settings::default();
        assert_eq!(s.language, "auto");
        assert_eq!(s.thresholds, vec![80, 95]);
        assert!(s.notify_reset);
        assert!(!s.pace_alerts);
        assert_eq!((s.poll_active_secs, s.poll_idle_secs), (180, 600));
        assert!(s.hidden_quotas.is_empty());
        assert_eq!(s.tray_bars, vec!["five_hour".to_string(), "seven_day".to_string()]);
    }

    #[test]
    fn defaults_when_no_file() {
        assert_eq!(load_from(&[dir("none", None)]), (Settings::default(), None));
    }

    #[test]
    fn partial_file_overrides_only_given_keys() {
        let (s, err) = load_from(&[dir("partial", Some(r#"{"thresholds":[50]}"#))]);
        assert_eq!(err, None);
        assert_eq!(s.thresholds, vec![50]);
        assert_eq!(s.poll_idle_secs, 600);
    }

    #[test]
    fn broken_file_gives_default_and_error() {
        let (s, err) = load_from(&[dir("broken", Some("{oops"))]);
        assert_eq!(s, Settings::default());
        assert!(err.is_some());
    }

    #[test]
    fn unknown_key_is_error() {
        let (s, err) = load_from(&[dir("unknown", Some(r#"{"treshold":[1]}"#))]);
        assert_eq!(s, Settings::default());
        assert!(err.is_some());
    }

    #[test]
    fn first_dir_wins() {
        let a = dir("first-a", Some(r#"{"language":"ru"}"#));
        let b = dir("first-b", Some(r#"{"language":"en"}"#));
        assert_eq!(load_from(&[dir("first-empty", None), a, b]).0.language, "ru");
    }
}
