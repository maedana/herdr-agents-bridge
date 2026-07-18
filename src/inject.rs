use std::process::Command;
use std::sync::Mutex;

const ALLOWED_KEYS: &[&str] = &["Escape", "Return", "Tab", "BackSpace", "Delete"];

pub trait TextInjector: Send + Sync {
    fn inject(&self, pane_id: &str, text: &str) -> Result<(), String>;
    fn send_key(&self, pane_id: &str, key: &str) -> Result<(), String>;
}

pub struct HerdrInjector;

static INJECT_LOCK: Mutex<()> = Mutex::new(());

pub fn is_allowed_key(key: &str) -> bool {
    ALLOWED_KEYS.iter().any(|k| k.eq_ignore_ascii_case(key))
}

fn normalize_key(key: &str) -> Option<&'static str> {
    ALLOWED_KEYS
        .iter()
        .find(|k| k.eq_ignore_ascii_case(key))
        .copied()
}

fn is_valid_pane_id(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with('-')
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == ':' || c == '_')
}

impl TextInjector for HerdrInjector {
    fn inject(&self, pane_id: &str, text: &str) -> Result<(), String> {
        if !is_valid_pane_id(pane_id) {
            return Err(format!("invalid pane_id: {pane_id}"));
        }
        let _lock = INJECT_LOCK.lock().unwrap();
        let output = Command::new("herdr")
            .args(["pane", "run", pane_id, text])
            .output()
            .map_err(|e| format!("herdr pane run failed: {e}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("herdr pane run failed: {stderr}"));
        }
        Ok(())
    }

    fn send_key(&self, pane_id: &str, key: &str) -> Result<(), String> {
        if !is_valid_pane_id(pane_id) {
            return Err(format!("invalid pane_id: {pane_id}"));
        }
        let key = normalize_key(key).ok_or_else(|| format!("key not allowed: {key}"))?;
        let _lock = INJECT_LOCK.lock().unwrap();
        let output = Command::new("herdr")
            .args(["pane", "send-keys", pane_id, key])
            .output()
            .map_err(|e| format!("herdr pane send-keys failed: {e}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("herdr pane send-keys failed: {stderr}"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_allowed_key_escape() {
        assert!(is_allowed_key("Escape"));
        assert!(is_allowed_key("escape"));
    }

    #[test]
    fn test_is_allowed_key_rejects_unknown() {
        assert!(!is_allowed_key("F1"));
        assert!(!is_allowed_key("ctrl+c"));
        assert!(!is_allowed_key(""));
    }

    #[test]
    fn test_normalize_key() {
        assert_eq!(normalize_key("escape"), Some("Escape"));
        assert_eq!(normalize_key("RETURN"), Some("Return"));
        assert_eq!(normalize_key("unknown"), None);
    }

    #[test]
    fn test_is_valid_pane_id() {
        assert!(is_valid_pane_id("wD:p1"));
        assert!(is_valid_pane_id("w9:p6"));
        assert!(is_valid_pane_id("term_abc123"));
    }

    #[test]
    fn test_invalid_pane_id() {
        assert!(!is_valid_pane_id(""));
        assert!(!is_valid_pane_id("--flag"));
        assert!(!is_valid_pane_id("-h"));
        assert!(!is_valid_pane_id("id;rm -rf /"));
    }

    #[test]
    fn test_herdr_injector_implements_trait() {
        let injector: Box<dyn TextInjector> = Box::new(HerdrInjector);
        assert!(std::mem::size_of_val(&injector) > 0);
    }
}
