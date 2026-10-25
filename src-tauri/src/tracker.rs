//! Background AI Agent Activity Detector (`Claude`, `Codex`, `Antigravity`, `Cursor`).
//!
//! Tracks a single user-selected agent and detects when it is actively working vs. finished:
//! - **Antigravity**: Inspects the tail of `~/.gemini/antigravity/brain/*/.system_generated/logs/transcript.jsonl`
//!   plus active `.system_generated/tasks/*.log` files.
//!   An Antigravity conversation is busy if:
//!   1. Any background task step in the tail still has `"status":"RUNNING"` without a matching
//!      `"type":"SYSTEM_MESSAGE"` completion for that task (`sender=<task-id>`), OR
//!   2. Any `.system_generated/tasks/*.log` file was modified recently, OR
//!   3. The final step in `transcript.jsonl` is not a `"PLANNER_RESPONSE"`, OR
//!   4. The final `"PLANNER_RESPONSE"` has `"tool_calls"` (inline or in `"truncated_fields"`), OR
//!   5. `transcript.jsonl` was written within the last 5 seconds.
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

/// Pure classifier for the tail of an Antigravity `transcript.jsonl` file.
///
/// Handles two critical mid-turn cases:
/// 1. **Synchronous tool steps**: `USER_INPUT` -> `PLANNER_RESPONSE (tool_calls)` -> `GENERIC` -> ...
/// 2. **Async background tasks**: `GENERIC` with `"status":"RUNNING"` and `task id: .../task-N`,
///    followed by an interim `PLANNER_RESPONSE` ("I launched the command and will wait for it to finish")
///    while the background task is still running! In `transcript.jsonl` (append-only), the `RUNNING`
///    line remains `"status":"RUNNING"` until a later `"type":"SYSTEM_MESSAGE"` line arrives with
///    `Task id ".../task-N" finished`.
pub fn is_antigravity_transcript_busy(tail_text: &str, age: Duration) -> bool {
    if age > Duration::from_secs(600) {
        return false;
    }
    if age <= Duration::from_secs(5) {
        return true;
    }

    let lines: Vec<&str> = tail_text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();

    let Some(&last_line) = lines.last() else {
        return false;
    };

    // Check if any recent step in the tail launched a background task ("status":"RUNNING")
    // that has not yet been closed by a subsequent SYSTEM_MESSAGE ("Task id \"...\" finished").
    let mut pending_running_tasks: Vec<&str> = Vec::new();
    for &line in &lines {
        if line.contains(r#""status":"RUNNING""#) || line.contains(r#""status": "RUNNING""#) {
            if let Some(task_id) = extract_running_task_id(line) {
                if !pending_running_tasks.contains(&task_id) {
                    pending_running_tasks.push(task_id);
                }
            } else {
                // Unknown running step without task_id: if it's within the last 3 lines, still busy
                pending_running_tasks.push("__running__");
            }
        } else if line.contains(r#""type":"SYSTEM_MESSAGE""#)
            || line.contains(r#""type": "SYSTEM_MESSAGE""#)
        {
            pending_running_tasks.retain(|&id| id != "__running__" && !line.contains(id));
        }
    }

    if !pending_running_tasks.is_empty() {
        return true;
    }

    let is_planner_response = last_line.contains(r#""type":"PLANNER_RESPONSE""#)
        || last_line.contains(r#""type": "PLANNER_RESPONSE""#);

    if !is_planner_response {
        // Any non-PLANNER_RESPONSE step (USER_INPUT, GENERIC tool result, SYSTEM_MESSAGE, etc.)
        // means the agent is mid-turn thinking about the tool output or user message!
        return true;
    }

    last_line.contains(r#""tool_calls":[{"#)
        || last_line.contains(r#""tool_calls": [{"#)
        || last_line.contains(r#""tool_calls""#)
}

fn extract_running_task_id(line: &str) -> Option<&str> {
    let marker = "task id: ";
    let start = line.find(marker)? + marker.len();
    let rest = &line[start..];
    let end = rest.find(['\\', '"', '\n', '\r', ' '])?;
    let id = &rest[..end];
    if id.is_empty() {
        None
    } else {
        Some(id)
    }
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

fn read_tail_text(path: &Path, max_bytes: u64) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    if len == 0 {
        return None;
    }
    let read_size = len.min(max_bytes);
    file.seek(SeekFrom::End(-(read_size as i64))).ok()?;
    let mut buf = Vec::with_capacity(read_size as usize);
    file.read_to_end(&mut buf).ok()?;
    let text = String::from_utf8_lossy(&buf);
    // If we seeked into the middle of the file, skip the partial first line
    if len > max_bytes {
        if let Some(idx) = text.find('\n') {
            return Some(text[idx + 1..].to_string());
        }
    }
    Some(text.to_string())
}

fn read_last_non_empty_line(path: &Path) -> Option<String> {
    let tail = read_tail_text(path, 32_768)?;
    tail.lines()
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
            if let Some(tail_text) = read_tail_text(&transcript, 65_536) {
                if is_antigravity_transcript_busy(&tail_text, age) {
                    return true;
                }
            }
            // Also check if any background task log in `.system_generated/tasks` was written recently
            let tasks_dir = sys_dir.join("tasks");
            if let Some((_task_log, task_age)) = most_recent_file_age_under(&tasks_dir, 2, now) {
                if task_age <= Duration::from_secs(8) {
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

        // Background task RUNNING followed by interim PLANNER_RESPONSE ("I launched the command and will wait")
        let running_bg_task_with_interim_response = concat!(
            r#"{"step_index":629,"type":"GENERIC","status":"RUNNING","content":"Tool is running as a background task with task id: conv-1/task-629\nLog output"}"#,
            "\n",
            r#"{"step_index":630,"type":"PLANNER_RESPONSE","status":"DONE","content":"It is rebuilding now — I will update you when it finishes!"}"#
        );
        assert!(is_antigravity_transcript_busy(
            running_bg_task_with_interim_response,
            Duration::from_secs(40)
        ));

        // Once the background task finishes via SYSTEM_MESSAGE AND the model emits its final PLANNER_RESPONSE:
        let finished_bg_task_and_final_response = concat!(
            r#"{"step_index":629,"type":"GENERIC","status":"RUNNING","content":"Tool is running as a background task with task id: conv-1/task-629\nLog output"}"#,
            "\n",
            r#"{"step_index":630,"type":"PLANNER_RESPONSE","status":"DONE","content":"It is rebuilding now"}"#,
            "\n",
            r#"{"step_index":631,"type":"SYSTEM_MESSAGE","status":"DONE","content":"sender=conv-1/task-629 priority=HIGH Task id \"conv-1/task-629\" finished"}"#,
            "\n",
            r#"{"step_index":632,"type":"PLANNER_RESPONSE","status":"DONE","content":"All done!"}"#
        );
        assert!(!is_antigravity_transcript_busy(
            finished_bg_task_and_final_response,
            Duration::from_secs(8)
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
