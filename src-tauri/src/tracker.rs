//! Background AI Agent Activity Detector (`Claude`, `Codex`, `Antigravity`, `Cursor`).
//!
//! Tracks a single user-selected agent and detects when it is actively working vs. finished:
//! - **Antigravity**: Inspects `~/.gemini/antigravity/brain/*/.system_generated/logs/transcript.jsonl`
//!   plus active `.system_generated/tasks/*.log` files.
//!   An Antigravity conversation is mid-turn unless the final step in `transcript.jsonl` is a
//!   `"PLANNER_RESPONSE"` with `"status":"DONE"`, has **no** `"tool_calls"` (and `"tool_calls"` was
//!   not truncated into `"truncated_fields"`), and no transcript or background task log has been
//!   written within the settle window.
//! - **Claude (`Claude Code`)**: Inspects `~/.claude/projects/*/*.jsonl`; a turn is only finished
//!   when the final entry is `"type":"assistant"` without a pending `"tool_use"` block.
//! - **Codex (`OpenAI Codex CLI`)**: Inspects `~/.codex` session logs with a 15-second settle window.
//! - **Cursor**: Inspects `~/.cursor` and `%APPDATA%/Cursor/User/workspaceStorage` with a 15-second
//!   settle window.

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
/// Key insight: During a multi-step agent turn, Antigravity appends intermediate steps like:
/// - `"type":"USER_INPUT"` (user prompt received)
/// - `"type":"PLANNER_RESPONSE"` with `"tool_calls":[...]` (or `"truncated_fields":["tool_calls"]`)
/// - `"type":"GENERIC"` (tool output returned; model is now thinking for the next step!)
/// - `"type":"SYSTEM_MESSAGE"` (background task finished; model is now waking up!)
///
/// Therefore, a turn is **busy** during ALL of those intermediate steps, and is ONLY idle/done
/// when the last step is a `"PLANNER_RESPONSE"` that has no `"tool_calls"` (neither inline nor
/// in `"truncated_fields"`) and the file has settled for >5 seconds.
pub fn is_antigravity_transcript_busy(last_line: &str, age: Duration) -> bool {
    if age > Duration::from_secs(600) {
        return false;
    }
    if age <= Duration::from_secs(5) {
        return true;
    }
    let trimmed = last_line.trim();
    if trimmed.is_empty() {
        return false;
    }

    let is_planner_response = trimmed.contains(r#""type":"PLANNER_RESPONSE""#)
        || trimmed.contains(r#""type": "PLANNER_RESPONSE""#);

    if !is_planner_response {
        // Any non-PLANNER_RESPONSE step (USER_INPUT, GENERIC tool result, SYSTEM_MESSAGE, etc.)
        // means the agent is mid-turn thinking about the tool output or user message!
        return true;
    }

    trimmed.contains(r#""tool_calls":[{"#)
        || trimmed.contains(r#""tool_calls": [{"#)
        || trimmed.contains(r#""tool_calls""#)
}

/// Pure classifier for the last line of a Claude Code session `.jsonl` file.
pub fn is_claude_transcript_busy(last_line: &str, age: Duration) -> bool {
    if age > Duration::from_secs(600) {
        return false;
    }
    if age <= Duration::from_secs(5) {
        return true;
    }
    let trimmed = last_line.trim();
    if trimmed.is_empty() {
        return false;
    }
    let is_assistant =
        trimmed.contains(r#""type":"assistant""#) || trimmed.contains(r#""type": "assistant""#);
    if !is_assistant {
        return true;
    }
    trimmed.contains(r#""type":"tool_use""#) || trimmed.contains(r#""type": "tool_use""#)
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
    let read_size = len.min(32_768);
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
            let sys_dir = entry.path().join(".system_generated");
            let transcript = sys_dir.join("logs").join("transcript.jsonl");
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
            // Also check if any background task log in `.system_generated/tasks` was written recently
            let tasks_dir = sys_dir.join("tasks");
            if let Some((_task_log, task_age)) = most_recent_file_age_under(&tasks_dir, 2, now) {
                if task_age <= Duration::from_secs(6) {
                    return true;
                }
            }
        }
    }
    false
}

fn most_recent_file_age_under(
    dir: &Path,
    max_depth: u8,
    now: SystemTime,
) -> Option<(PathBuf, Duration)> {
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
            if age <= Duration::from_secs(15) {
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
            if age <= Duration::from_secs(15) {
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
            if age <= Duration::from_secs(15) {
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
    fn antigravity_transcript_busy_across_all_mid_turn_step_types() {
        let user_line = r#"{"step_index":10,"type":"USER_INPUT","status":"DONE"}"#;
        assert!(is_antigravity_transcript_busy(
            user_line,
            Duration::from_secs(30)
        ));

        // Tool call inline
        let tool_call_line =
            r#"{"step_index":11,"type":"PLANNER_RESPONSE","tool_calls":[{"id":"1"}]}"#;
        assert!(is_antigravity_transcript_busy(
            tool_call_line,
            Duration::from_secs(30)
        ));

        // Tool call truncated into truncated_fields
        let truncated_tool_call =
            r#"{"step_index":12,"type":"PLANNER_RESPONSE","truncated_fields":["tool_calls"]}"#;
        assert!(is_antigravity_transcript_busy(
            truncated_tool_call,
            Duration::from_secs(30)
        ));

        // GENERIC tool result step (model is now thinking about the tool output!)
        let generic_tool_output =
            r#"{"step_index":13,"type":"GENERIC","status":"DONE","content":"Created file."}"#;
        assert!(is_antigravity_transcript_busy(
            generic_tool_output,
            Duration::from_secs(45)
        ));

        // SYSTEM_MESSAGE step (background task completed, model is waking up!)
        let system_msg =
            r#"{"step_index":14,"type":"SYSTEM_MESSAGE","status":"DONE","content":"Task finished"}"#;
        assert!(is_antigravity_transcript_busy(
            system_msg,
            Duration::from_secs(25)
        ));

        // Final PLANNER_RESPONSE without tool_calls:
        let final_response_line =
            r#"{"step_index":15,"type":"PLANNER_RESPONSE","status":"DONE","content":"All done!"}"#;
        // Within 5 seconds of write, still settling
        assert!(is_antigravity_transcript_busy(
            final_response_line,
            Duration::from_secs(2)
        ));
        // After 6+ seconds with no tool_calls, turn is truly finished!
        assert!(!is_antigravity_transcript_busy(
            final_response_line,
            Duration::from_secs(6)
        ));
    }

    #[test]
    fn claude_transcript_busy_when_user_or_tool_use_is_latest() {
        let user_msg = r#"{"type":"user","message":{"content":"fix bug"}}"#;
        assert!(is_claude_transcript_busy(user_msg, Duration::from_secs(15)));

        let tool_result_msg = r#"{"type":"tool_result","content":"ok"}"#;
        assert!(is_claude_transcript_busy(
            tool_result_msg,
            Duration::from_secs(20)
        ));

        let assistant_done = r#"{"type":"assistant","message":{"content":"Fixed!"}}"#;
        assert!(!is_claude_transcript_busy(
            assistant_done,
            Duration::from_secs(10)
        ));
    }
}
