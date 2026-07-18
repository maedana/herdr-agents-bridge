const TEMPLATE: &str = include_str!("ui.html");

pub fn render(session_token: &str) -> String {
    TEMPLATE.replace("__SESSION_TOKEN__", session_token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_replaces_token() {
        let html = render("abc123");
        assert!(html.contains("abc123"));
        assert!(!html.contains("__SESSION_TOKEN__"));
    }

    #[test]
    fn test_render_contains_voicebridge() {
        let html = render("token");
        assert!(html.contains("VoiceBridge"));
    }

    #[test]
    fn test_render_contains_history_replace_state() {
        let html = render("token");
        assert!(html.contains("history.replaceState(null, '', '/')"));
    }
}
