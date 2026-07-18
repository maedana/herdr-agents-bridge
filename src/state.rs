use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use crate::inject::TextInjector;

pub const PORT: u16 = 9876;
pub const INJECT_DELAY_MILLIS: u64 = 300;
pub const MAX_TEXT_LENGTH: usize = 10_000;
pub const RATE_LIMIT_MILLIS: u64 = 1_000;

pub struct AppState {
    pub session_token: String,
    pub allowed_ip: Mutex<Option<IpAddr>>,
    pub ip_locked: AtomicBool,
    pub last_inject_time: Mutex<Instant>,
    pub injector: Box<dyn TextInjector>,
}

impl AppState {
    pub fn new(injector: Box<dyn TextInjector>) -> Self {
        let token = generate_token();
        Self {
            session_token: token,
            allowed_ip: Mutex::new(None),
            ip_locked: AtomicBool::new(false),
            last_inject_time: Mutex::new(Instant::now() - std::time::Duration::from_secs(60)),
            injector,
        }
    }

    pub fn check_rate_limit(&self) -> bool {
        let mut last = self.last_inject_time.lock().unwrap();
        let now = Instant::now();
        if now.duration_since(*last).as_millis() < RATE_LIMIT_MILLIS as u128 {
            return false;
        }
        *last = now;
        true
    }

    pub fn try_register_ip(&self, ip: IpAddr) -> bool {
        if self.ip_locked.load(Ordering::Acquire) {
            return false;
        }
        let mut allowed = self.allowed_ip.lock().unwrap();
        if allowed.is_some() {
            return false;
        }
        *allowed = Some(ip);
        self.ip_locked.store(true, Ordering::Release);
        true
    }

    pub fn is_allowed_ip(&self, ip: &IpAddr) -> bool {
        let allowed = self.allowed_ip.lock().unwrap();
        allowed.as_ref() == Some(ip)
    }

    pub fn check_token(&self, token: &str) -> bool {
        constant_time_eq(self.session_token.as_bytes(), token.as_bytes())
    }
}

fn generate_token() -> String {
    use rand::Rng;
    let bytes: [u8; 8] = rand::rng().random();
    hex_encode(&bytes)
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockInjector;
    impl TextInjector for MockInjector {
        fn inject(&self, _text: &str, _enter: bool) -> Result<(), String> {
            Ok(())
        }
        fn send_key(&self, _key: &str) -> Result<(), String> {
            Ok(())
        }
    }

    fn make_state() -> AppState {
        AppState::new(Box::new(MockInjector))
    }

    #[test]
    fn test_token_is_16_hex_chars() {
        let state = make_state();
        assert_eq!(state.session_token.len(), 16);
        assert!(state.session_token.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_check_token_valid() {
        let state = make_state();
        let token = state.session_token.clone();
        assert!(state.check_token(&token));
    }

    #[test]
    fn test_check_token_invalid() {
        let state = make_state();
        assert!(!state.check_token("bad_token"));
    }

    #[test]
    fn test_check_token_partial() {
        let state = make_state();
        let partial = &state.session_token[..8];
        assert!(!state.check_token(partial));
    }

    #[test]
    fn test_try_register_ip_first_succeeds() {
        let state = make_state();
        let ip: IpAddr = "192.168.1.10".parse().unwrap();
        assert!(state.try_register_ip(ip));
        assert!(state.is_allowed_ip(&ip));
    }

    #[test]
    fn test_try_register_ip_second_fails() {
        let state = make_state();
        let ip1: IpAddr = "192.168.1.10".parse().unwrap();
        let ip2: IpAddr = "192.168.1.20".parse().unwrap();
        assert!(state.try_register_ip(ip1));
        assert!(!state.try_register_ip(ip2));
    }

    #[test]
    fn test_is_allowed_ip_unregistered() {
        let state = make_state();
        let ip: IpAddr = "192.168.1.10".parse().unwrap();
        assert!(!state.is_allowed_ip(&ip));
    }

    #[test]
    fn test_rate_limit_first_passes() {
        let state = make_state();
        assert!(state.check_rate_limit());
    }

    #[test]
    fn test_rate_limit_second_blocked() {
        let state = make_state();
        assert!(state.check_rate_limit());
        assert!(!state.check_rate_limit());
    }

    #[test]
    fn test_rate_limit_after_cooldown() {
        let state = make_state();
        {
            let mut last = state.last_inject_time.lock().unwrap();
            *last = Instant::now() - std::time::Duration::from_secs(2);
        }
        assert!(state.check_rate_limit());
    }

    #[test]
    fn test_constant_time_eq() {
        assert!(constant_time_eq(b"hello", b"hello"));
        assert!(!constant_time_eq(b"hello", b"world"));
        assert!(!constant_time_eq(b"hello", b"hell"));
    }
}
