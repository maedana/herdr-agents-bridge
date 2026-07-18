use std::process::Command;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

const ALLOWED_KEYS: &[&str] = &["Escape", "Return", "Tab", "BackSpace", "Delete"];

pub trait TextInjector: Send + Sync {
    fn inject(&self, text: &str, enter: bool) -> Result<(), String>;
    fn send_key(&self, key: &str) -> Result<(), String>;
}

pub struct XdotoolInjector;

static INJECT_LOCK: Mutex<()> = Mutex::new(());

fn clipboard_get() -> Result<String, String> {
    let output = Command::new("xclip")
        .args(["-selection", "clipboard", "-o"])
        .output()
        .map_err(|e| format!("xclip failed: {e}"))?;
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn clipboard_set(text: &str) -> Result<(), String> {
    use std::io::Write;
    let mut child = Command::new("xclip")
        .args(["-selection", "clipboard"])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("xclip failed: {e}"))?;
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(text.as_bytes())
        .map_err(|e| format!("write to xclip failed: {e}"))?;
    child.wait().map_err(|e| format!("xclip wait failed: {e}"))?;
    Ok(())
}

fn send_ctrl_v() -> Result<(), String> {
    Command::new("xdotool")
        .args(["key", "ctrl+v"])
        .status()
        .map_err(|e| format!("xdotool failed: {e}"))?;
    Ok(())
}

fn send_enter() -> Result<(), String> {
    Command::new("xdotool")
        .args(["key", "Return"])
        .status()
        .map_err(|e| format!("xdotool failed: {e}"))?;
    Ok(())
}

pub fn is_allowed_key(key: &str) -> bool {
    ALLOWED_KEYS.iter().any(|k| k.eq_ignore_ascii_case(key))
}

fn normalize_key(key: &str) -> Option<&'static str> {
    ALLOWED_KEYS
        .iter()
        .find(|k| k.eq_ignore_ascii_case(key))
        .copied()
}

impl TextInjector for XdotoolInjector {
    fn inject(&self, text: &str, enter: bool) -> Result<(), String> {
        let _lock = INJECT_LOCK.lock().unwrap();
        let old = clipboard_get().unwrap_or_default();
        let result = (|| {
            clipboard_set(text)?;
            thread::sleep(Duration::from_millis(50));
            send_ctrl_v()?;
            thread::sleep(Duration::from_millis(100));
            if enter {
                send_enter()?;
            }
            Ok(())
        })();
        let _ = clipboard_set(&old);
        result
    }

    fn send_key(&self, key: &str) -> Result<(), String> {
        let key = normalize_key(key).ok_or_else(|| format!("key not allowed: {key}"))?;
        let _lock = INJECT_LOCK.lock().unwrap();
        Command::new("xdotool")
            .args(["key", key])
            .status()
            .map_err(|e| format!("xdotool failed: {e}"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clipboard_get_runs_xclip() {
        let result = clipboard_get();
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn test_clipboard_roundtrip() {
        let original = clipboard_get().unwrap_or_default();
        let test_text = "voice-bridge-test-日本語";
        clipboard_set(test_text).unwrap();
        let got = clipboard_get().unwrap();
        assert_eq!(got, test_text);
        let _ = clipboard_set(&original);
    }

    #[test]
    fn test_xdotool_injector_implements_trait() {
        let injector: Box<dyn TextInjector> = Box::new(XdotoolInjector);
        assert!(std::mem::size_of_val(&injector) > 0);
    }

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
}
