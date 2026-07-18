use std::fs;
use std::path::PathBuf;

use std::path::Path;

fn default_runtime_dir() -> PathBuf {
    std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
        .join("herdr-agents-bridge")
}

fn pid_path_in(dir: &Path) -> PathBuf {
    dir.join("pid")
}

fn url_path_in(dir: &Path) -> PathBuf {
    dir.join("url")
}

pub fn write(pid: u32, url: &str) -> Result<(), String> {
    write_in(&default_runtime_dir(), pid, url)
}

pub fn read_pid() -> Option<u32> {
    read_pid_in(&default_runtime_dir())
}

pub fn read_url() -> Option<String> {
    read_url_in(&default_runtime_dir())
}

pub fn cleanup() {
    cleanup_in(&default_runtime_dir());
}

fn write_in(dir: &Path, pid: u32, url: &str) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| format!("mkdir failed: {e}"))?;
    fs::write(pid_path_in(dir), pid.to_string())
        .map_err(|e| format!("write pid failed: {e}"))?;
    fs::write(url_path_in(dir), url).map_err(|e| format!("write url failed: {e}"))?;
    Ok(())
}

fn read_pid_in(dir: &Path) -> Option<u32> {
    fs::read_to_string(pid_path_in(dir))
        .ok()?
        .trim()
        .parse()
        .ok()
}

fn read_url_in(dir: &Path) -> Option<String> {
    let url = fs::read_to_string(url_path_in(dir)).ok()?;
    let url = url.trim().to_string();
    if url.is_empty() { None } else { Some(url) }
}

fn cleanup_in(dir: &Path) {
    let _ = fs::remove_file(pid_path_in(dir));
    let _ = fs::remove_file(url_path_in(dir));
    let _ = fs::remove_dir(dir);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_write_and_read_pid() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path().join("hab");
        write_in(&d, 12345, "http://example.com").unwrap();
        assert_eq!(read_pid_in(&d), Some(12345));
    }

    #[test]
    fn test_write_and_read_url() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path().join("hab");
        write_in(&d, 1, "http://192.168.1.1:9876/?t=abc123").unwrap();
        assert_eq!(
            read_url_in(&d),
            Some("http://192.168.1.1:9876/?t=abc123".to_string())
        );
    }

    #[test]
    fn test_read_pid_missing() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path().join("hab");
        assert_eq!(read_pid_in(&d), None);
    }

    #[test]
    fn test_read_url_missing() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path().join("hab");
        assert_eq!(read_url_in(&d), None);
    }

    #[test]
    fn test_cleanup() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path().join("hab");
        write_in(&d, 99, "http://test").unwrap();
        cleanup_in(&d);
        assert_eq!(read_pid_in(&d), None);
        assert_eq!(read_url_in(&d), None);
    }
}
