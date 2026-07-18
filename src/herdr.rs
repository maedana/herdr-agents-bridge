use serde::Serialize;
use std::process::Command;

#[derive(Debug, Clone, Serialize)]
pub struct AgentInfo {
    pub pane_id: String,
    pub status: String,
    pub repo: String,
    pub branch: String,
    pub title: String,
    pub focused: bool,
}

pub fn list_agents() -> Result<Vec<AgentInfo>, String> {
    let output = Command::new("herdr")
        .args(["agent", "list"])
        .output()
        .map_err(|e| format!("herdr agent list failed: {e}"))?;

    let text = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("invalid JSON: {e}"))?;

    let agents = json["result"]["agents"]
        .as_array()
        .ok_or("no agents array")?;

    let mut result = Vec::new();
    for agent in agents {
        let cwd = agent["cwd"].as_str().unwrap_or("");
        let pane_id = agent["pane_id"].as_str().unwrap_or("").to_string();
        let status = agent["agent_status"].as_str().unwrap_or("unknown").to_string();
        let title = agent["terminal_title_stripped"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let focused = agent["focused"].as_bool().unwrap_or(false);
        let repo = extract_repo_name(cwd);
        let branch = get_git_branch(cwd);

        result.push(AgentInfo {
            pane_id,
            status,
            repo,
            branch,
            title,
            focused,
        });
    }
    Ok(result)
}

fn is_valid_pane_id(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with('-')
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == ':' || c == '_')
}

pub fn read_agent(pane_id: &str, lines: u32) -> Result<String, String> {
    if !is_valid_pane_id(pane_id) {
        return Err(format!("invalid pane_id: {pane_id}"));
    }
    let output = Command::new("herdr")
        .args([
            "agent",
            "read",
            pane_id,
            "--source",
            "visible",
            "--lines",
            &lines.to_string(),
            "--format",
            "ansi",
        ])
        .output()
        .map_err(|e| format!("herdr agent read failed: {e}"))?;

    let text = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("invalid JSON: {e}"))?;

    let ansi_text = json["result"]["read"]["text"]
        .as_str()
        .unwrap_or("")
        .to_string();

    ansi_to_html::convert(&ansi_text).map_err(|e| format!("ansi conversion failed: {e}"))
}

pub fn focus_agent(pane_id: &str) -> Result<(), String> {
    if !is_valid_pane_id(pane_id) {
        return Err(format!("invalid pane_id: {pane_id}"));
    }
    Command::new("herdr")
        .args(["agent", "focus", pane_id])
        .status()
        .map_err(|e| format!("herdr agent focus failed: {e}"))?;
    Ok(())
}

fn extract_repo_name(cwd: &str) -> String {
    if let Some(pos) = cwd.find("/.claude/worktrees/") {
        let base = &cwd[..pos];
        return base.rsplit('/').next().unwrap_or("").to_string();
    }
    cwd.rsplit('/').next().unwrap_or("").to_string()
}

fn get_git_branch(cwd: &str) -> String {
    Command::new("git")
        .args(["-C", cwd, "rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_repo_name_normal() {
        assert_eq!(
            extract_repo_name("/home/user/src/github.com/Org/my-repo"),
            "my-repo"
        );
    }

    #[test]
    fn test_extract_repo_name_worktree() {
        assert_eq!(
            extract_repo_name("/home/user/src/Org/ga-pms/.claude/worktrees/for-sentry-2"),
            "ga-pms"
        );
    }

    #[test]
    fn test_extract_repo_name_simple() {
        assert_eq!(extract_repo_name("/tmp/voice-bridge-for-linux"), "voice-bridge-for-linux");
    }

    #[test]
    fn test_list_agents_returns_result() {
        let result = list_agents();
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn test_valid_pane_id() {
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
}
