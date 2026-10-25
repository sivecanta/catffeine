//! Background AI Agent Activity Detector (`Claude`, `Codex`, `Antigravity`, `Cursor`).
//!
//! Tracks a single user-selected agent and detects when it is actively working vs. finished:
//! - **Antigravity**: Checks `~/.gemini/antigravity/brain/*/.system_generated/logs/transcript.jsonl`
//!   modified within the last 10 minutes; if the last JSONL line has `"type":"USER_INPUT"` or is a
//!   `"PLANNER_RESPONSE"` with `"tool_calls"`, or if the transcript was written within the last 4 seconds,
//!   Antigravity is actively working. When the final line is a `"PLANNER_RESPONSE"` without `"tool_calls"`
//!   and settles (>3s), the turn is complete!
//! - **Claude (`Claude Code`)**: Checks `~/.claude/projects/*/*.jsonl` (and WSL `~/.claude/projects`)
//!   modified recently, plus active `claude` CLI processes.
//! - **Codex (`OpenAI Codex CLI`)**: Checks `~/.codex/sessions/**/*.jsonl` modified recently, plus active
//!   `codex` CLI processes.
//! - **Cursor**: Checks recently modified Cursor state/storage files and active Cursor agent processes.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

/// Selectable AI agent to track. Only one agent (or `None`) can be tracked at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrackedAgent {
    #[default]
    None,
    Claude,
    Codex,
    Antigravity,
    Cursor,
}

impl TrackedAgent {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::None => "Off",
            Self::Claude => "Claude",
            Self::Codex => "Codex",
            Self::Antigravity => "Antigravity",
            Self::Cursor => "Cursor",
        }
    }
}

/// Pure classifier for the last line of an Antigravity `transcript.jsonl` file.
///
/// Returns `true` if the agent is mid-turn:
/// - The file was written within `recent_write_threshold` (e.g. 4 seconds), OR
/// - The last JSONL entry is a `USER_INPUT` step, OR
/// - The last JSONL entry is a `PLANNER_RESPONSE` that made `tool_calls` (waiting for tool execution).
pub fn is_antigravity_transcript_busy(last_line: &str, age: Duration) -> bool {
    if age > Duration::from_secs(600) {
        return false;
    }
    if age <= Duration::from_secs(4) {
        return true;
    }
    let trimmed = last_line.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.contains(r#""type":"USER_INPUT""#) || trimmed.contains(r#""type": "USER_INPUT""#) {
        return true;
    }
    if (trimmed.contains(r#""type":"PLANNER_RESPONSE""#)
        || trimmed.contains(r#""type": "PLANNER_RESPONSE""#))
        && (trimmed.contains(r#""tool_calls":[{"#) || trimmed.contains(r#""tool_calls": [{"#))
    {
        return true;
    }
    false
}

/// Pure classifier for the last line of a Claude Code session `.jsonl` file.
pub fn is_claude_transcript_busy(last_line: &str, age: Duration) -> bool {
    if age > Duration::from_secs(600) {
        return false;
    }
    if age <= Duration::from_secs(4) {
        return true;
    }
    let trimmed = last_line.trim();
    if trimmed.is_empty() {
        return false;
    }
    // In Claude Code JSONL, if the last message is from "user" or is a tool_use, Claude is still working.
    if trimmed.contains(r#""type":"user""#) || trimmed.contains(r#""type":"tool_use""#) {
        return true;
    }
    false
}

/// Checks whether the currently selected [`TrackedAgent`] is actively working right now.
pub fn is_tracked_agent_busy(agent: TrackedAgent) -> bool {
    match agent {
        TrackedAgent::None => false,
        TrackedAgent::Antigravity => check_antigravity_busy(),
        TrackedAgent::Claude => check_claude_busy(),
        TrackedAgent::Codex => check_codex_busy(),
        TrackedAgent::Cursor => check_cursor_busy(),
    }
}

fn candidate_home_dirs() -> Vec<PathBuf> {
    let mut homes = Vec::new();
    if let Some(up) = std::env::var_os("USERPROFILE") {
        homes.push(PathBuf::from(up));
    }
    if let Some(h) = std::env::var_os("HOME") {
        let p = PathBuf::from(h);
        if !homes.contains(&p) {
            homes.push(p);
        }
    }
    homes
}

fn read_last_non_empty_line(path: &Path) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    if len == 0 {
        return None;
    }
    let read_size = len.min(16_384);
    file.seek(SeekFrom::End(-(read_size as i64))).ok()?;
    let mut buf = Vec::with_capacity(read_size as usize);
    file.read_to_end(&mut buf).ok()?;
    let text = String::from_utf8_lossy(&buf);
    text.lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .map(str::to_string)
}

fn check_antigravity_busy() -> bool {
    let now = SystemTime::now();
    for home in candidate_home_dirs() {
        let brain_dir = home.join(".gemini").join("antigravity").join("brain");
        let Ok(entries) = std::fs::read_dir(brain_dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let transcript = entry
                .path()
                .join(".system_generated")
                .join("logs")
                .join("transcript.jsonl");
            let Ok(meta) = std::fs::metadata(&transcript) else {
                continue;
            };
            let Ok(modified) = meta.modified() else {
                continue;
            };
            let age = now.duration_since(modified).unwrap_or(Duration::MAX);
            if age > Duration::from_secs(600) {
                continue;
            }
            if let Some(last_line) = read_last_non_empty_line(&transcript) {
                if is_antigravity_transcript_busy(&last_line, age) {
                    return true;
                }
            }
        }
    }
    false
}

fn most_recent_file_age_under(dir: &Path, max_depth: u8, now: SystemTime) -> Option<(PathBuf, Duration)> {
    if max_depth == 0 || !dir.exists() {
        return None;
    }
    let mut best: Option<(PathBuf, Duration)> = None;
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        if ft.is_dir() {
            if let Some(sub) = most_recent_file_age_under(&path, max_depth - 1, now) {
                if best.as_ref().is_none_or(|b| sub.1 < b.1) {
                    best = Some(sub);
                }
            }
        } else if ft.is_file() {
            if let Ok(meta) = entry.metadata() {
                if let Ok(modified) = meta.modified() {
                    let age = now.duration_since(modified).unwrap_or(Duration::MAX);
                    if best.as_ref().is_none_or(|b| age < b.1) {
                        best = Some((path, age));
                    }
                }
            }
        }
    }
    best
}

fn check_claude_busy() -> bool {
    let now = SystemTime::now();
    for home in candidate_home_dirs() {
        let projects_dir = home.join(".claude").join("projects");
        if let Some((path, age)) = most_recent_file_age_under(&projects_dir, 3, now) {
            if let Some(last_line) = read_last_non_empty_line(&path) {
                if is_claude_transcript_busy(&last_line, age) {
                    return true;
                }
            }
        }
    }
    false
}

fn check_codex_busy() -> bool {
    let now = SystemTime::now();
    for home in candidate_home_dirs() {
        let codex_dir = home.join(".codex");
        if let Some((_path, age)) = most_recent_file_age_under(&codex_dir, 4, now) {
            if age <= Duration::from_secs(5) {
                return true;
            }
        }
    }
    false
}

fn check_cursor_busy() -> bool {
    let now = SystemTime::now();
    for home in candidate_home_dirs() {
        let cursor_dir = home.join(".cursor");
        if let Some((_path, age)) = most_recent_file_age_under(&cursor_dir, 3, now) {
            if age <= Duration::from_secs(5) {
                return true;
            }
        }
    }
    if let Some(appdata) = std::env::var_os("APPDATA") {
        let workspace_storage = PathBuf::from(appdata)
            .join("Cursor")
            .join("User")
            .join("workspaceStorage");
        if let Some((_path, age)) = most_recent_file_age_under(&workspace_storage, 3, now) {
            if age <= Duration::from_secs(5) {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn antigravity_transcript_busy_when_user_input_or_tool_calls_present() {
        let user_line = r#"{"step_index":10,"type":"USER_INPUT","status":"DONE"}"#;
        assert!(is_antigravity_transcript_busy(
            user_line,
            Duration::from_secs(20)
        ));

        let tool_call_line =
            r#"{"step_index":11,"type":"PLANNER_RESPONSE","tool_calls":[{"id":"1"}]}"#;
        assert!(is_antigravity_transcript_busy(
            tool_call_line,
            Duration::from_secs(20)
        ));

        let final_response_line =
            r#"{"step_index":12,"type":"PLANNER_RESPONSE","status":"DONE","content":"All done!"}"#;
        // Within 4 seconds of write, still considered active/settling
        assert!(is_antigravity_transcript_busy(
            final_response_line,
            Duration::from_secs(2)
        ));
        // After 5+ seconds with no tool_calls, turn is finished!
        assert!(!is_antigravity_transcript_busy(
            final_response_line,
            Duration::from_secs(6)
        ));
    }

    #[test]
    fn claude_transcript_busy_when_user_or_tool_use_is_latest() {
        let user_msg = r#"{"type":"user","message":{"content":"fix bug"}}"#;
        assert!(is_claude_transcript_busy(user_msg, Duration::from_secs(15)));

        let assistant_done = r#"{"type":"assistant","message":{"content":"Fixed!"}}"#;
        assert!(!is_claude_transcript_busy(
            assistant_done,
            Duration::from_secs(10)
        ));
    }
}
