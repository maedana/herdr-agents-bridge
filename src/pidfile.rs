use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

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

fn tunnel_pid_path_in(dir: &Path) -> PathBuf {
    dir.join("tunnel_pid")
}

fn tunnel_url_path_in(dir: &Path) -> PathBuf {
    dir.join("tunnel_url")
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

pub fn is_running() -> bool {
    is_running_in(&default_runtime_dir())
}

pub fn write_tunnel(pid: u32, url: &str) -> Result<(), String> {
    write_tunnel_in(&default_runtime_dir(), pid, url)
}

pub fn read_tunnel_url() -> Option<String> {
    read_tunnel_url_in(&default_runtime_dir())
}

pub fn read_tunnel_pid() -> Option<u32> {
    read_tunnel_pid_in(&default_runtime_dir())
}

pub fn cleanup_tunnel() {
    cleanup_tunnel_in(&default_runtime_dir());
}

fn is_running_in(dir: &Path) -> bool {
    let Some(pid) = read_pid_in(dir) else {
        return false;
    };
    Path::new(&format!("/proc/{pid}")).exists()
}

fn write_in(dir: &Path, pid: u32, url: &str) -> Result<(), String> {
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .map_err(|e| format!("mkdir failed: {e}"))?;
    write_file_restricted(&pid_path_in(dir), pid.to_string().as_bytes())?;
    write_file_restricted(&url_path_in(dir), url.as_bytes())?;
    Ok(())
}

fn write_file_restricted(path: &Path, data: &[u8]) -> Result<(), String> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| format!("open {} failed: {e}", path.display()))?;
    f.write_all(data)
        .map_err(|e| format!("write {} failed: {e}", path.display()))
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
    cleanup_tunnel_in(dir);
    let _ = fs::remove_dir(dir);
}

fn write_tunnel_in(dir: &Path, pid: u32, url: &str) -> Result<(), String> {
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .map_err(|e| format!("mkdir failed: {e}"))?;
    write_file_restricted(&tunnel_pid_path_in(dir), pid.to_string().as_bytes())?;
    write_file_restricted(&tunnel_url_path_in(dir), url.as_bytes())?;
    Ok(())
}

fn read_tunnel_pid_in(dir: &Path) -> Option<u32> {
    fs::read_to_string(tunnel_pid_path_in(dir))
        .ok()?
        .trim()
        .parse()
        .ok()
}

fn read_tunnel_url_in(dir: &Path) -> Option<String> {
    let url = fs::read_to_string(tunnel_url_path_in(dir)).ok()?;
    let url = url.trim().to_string();
    if url.is_empty() { None } else { Some(url) }
}

fn cleanup_tunnel_in(dir: &Path) {
    let _ = fs::remove_file(tunnel_pid_path_in(dir));
    let _ = fs::remove_file(tunnel_url_path_in(dir));
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
    fn test_file_permissions_are_restricted() {
        use std::os::unix::fs::MetadataExt;
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path().join("hab");
        write_in(&d, 1, "http://secret").unwrap();
        let dir_mode = fs::metadata(&d).unwrap().mode() & 0o777;
        assert_eq!(dir_mode, 0o700);
        let url_mode = fs::metadata(url_path_in(&d)).unwrap().mode() & 0o777;
        assert_eq!(url_mode, 0o600);
        let pid_mode = fs::metadata(pid_path_in(&d)).unwrap().mode() & 0o777;
        assert_eq!(pid_mode, 0o600);
    }

    #[test]
    fn test_is_running_with_live_pid() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path().join("hab");
        let own_pid = std::process::id();
        write_in(&d, own_pid, "http://test").unwrap();
        assert!(is_running_in(&d));
    }

    #[test]
    fn test_is_running_with_dead_pid() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path().join("hab");
        write_in(&d, 999_999_999, "http://test").unwrap();
        assert!(!is_running_in(&d));
    }

    #[test]
    fn test_is_running_no_pidfile() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path().join("hab");
        assert!(!is_running_in(&d));
    }

    #[test]
    fn test_write_and_read_tunnel() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path().join("hab");
        write_in(&d, 1, "http://local").unwrap();
        write_tunnel_in(&d, 999, "https://abc.trycloudflare.com").unwrap();
        assert_eq!(read_tunnel_pid_in(&d), Some(999));
        assert_eq!(
            read_tunnel_url_in(&d),
            Some("https://abc.trycloudflare.com".to_string())
        );
    }

    #[test]
    fn test_read_tunnel_url_missing() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path().join("hab");
        assert_eq!(read_tunnel_url_in(&d), None);
    }

    #[test]
    fn test_cleanup_tunnel() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path().join("hab");
        write_in(&d, 1, "http://local").unwrap();
        write_tunnel_in(&d, 999, "https://tunnel").unwrap();
        cleanup_tunnel_in(&d);
        assert_eq!(read_tunnel_url_in(&d), None);
        assert_eq!(read_tunnel_pid_in(&d), None);
        assert_eq!(read_pid_in(&d), Some(1));
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
