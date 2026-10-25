//! Background AI Agent Activity Detector (`Claude`, `Codex`, `Antigravity`, `Cursor`).
//!
//! Tracks a single user-selected agent and detects when it is actively working vs. finished
//! across both native OS home directories (`USERPROFILE` / `HOME`) and WSL (`\\wsl.localhost\<distro>\home\<user>`):
//! - **Antigravity**: Inspects ALL recently active `~/.gemini/antigravity/brain/*/.system_generated/logs/transcript.jsonl`
//!   files (main conversation + any subagents) plus `.system_generated/tasks/*.log` files.
//! - **Cursor**: Inspects ALL recently active `~/.cursor/projects/*/agent-transcripts/**/*.jsonl` files
//!   (both the parent conversation AND all `subagents/*.jsonl`).
//!   When a Cursor parent agent spawns subagents (`T1`, `T2`, `T3`), individual subagents write
//!   `{"type":"turn_ended"}` as they finish, while the **parent** transcript (`<id>/<id>.jsonl`) and
//!   remaining subagents do NOT have `{"type":"turn_ended"}` until the entire multi-agent job is done!
//!   By checking whether **any** recent `.jsonl` transcript is still open (missing `turn_ended`),
//!   Cursor stays `Working` across all subagent completions until the parent turn itself ends!
//! - **Claude (`Claude Code`)**: Inspects ALL recent `~/.claude/projects/**/*.jsonl` files across native + WSL homes.
//! - **Codex (`OpenAI Codex CLI`)**: Inspects `~/.codex` session logs across native + WSL homes.

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
pub fn is_antigravity_transcript_busy(tail_text: &str, age: Duration) -> bool {
    if age > Duration::from_secs(1800) {
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

/// Pure classifier for the last line of a Cursor `agent-transcripts/**/*.jsonl` file.
///
/// Cursor writes `{"type":"turn_ended","status":"success"}` ONLY when a transcript's turn has
/// completely finished. When a parent agent spawns subagents (`T1`, `T2`, `T3`), the parent
/// transcript remains on `{"role":"assistant",...}` (without `turn_ended`) for up to 30+ minutes
/// while the subagents execute!
pub fn is_cursor_transcript_busy(last_line: &str, age: Duration) -> bool {
    if age > Duration::from_secs(3600) {
        return false;
    }
    let trimmed = last_line.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.contains(r#""type":"turn_ended""#) || trimmed.contains(r#""type": "turn_ended""#) {
        return false;
    }
    true
}

/// Pure classifier for a collection of Cursor transcript `(last_line, age)` pairs
/// (e.g. parent conversation + spawned subagents).
///
/// Returns `true` if **any** transcript in the active session has not yet written `"turn_ended"`.
pub fn is_any_cursor_transcript_busy<'a, I>(transcripts: I) -> bool
where
    I: IntoIterator<Item = (&'a str, Duration)>,
{
    transcripts
        .into_iter()
        .any(|(last_line, age)| is_cursor_transcript_busy(last_line, age))
}

/// Pure classifier for the last line of a Claude Code session `.jsonl` file.
pub fn is_claude_transcript_busy(last_line: &str, age: Duration) -> bool {
    if age > Duration::from_secs(1800) {
        return false;
    }
    if age <= Duration::from_secs(5) {
        return true;
    }
    let trimmed = last_line.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.contains(r#""type":"turn_ended""#) {
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

/// Returns candidate home directories, including both native OS (`USERPROFILE` / `HOME`)
/// and WSL home directories (`\\wsl.localhost\<distro>\home\<user>`) when running on Windows.
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

    #[cfg(target_os = "windows")]
    if let Some(user) = homes
        .first()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .map(str::to_string)
    {
        for distro in ["Ubuntu", "Ubuntu-24.04", "Ubuntu-22.04", "Debian"] {
            let wsl_home = PathBuf::from(format!(r"\\wsl.localhost\{distro}\home\{user}"));
            if wsl_home.exists() && !homes.contains(&wsl_home) {
                homes.push(wsl_home);
            }
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
            if age > Duration::from_secs(1800) {
                continue;
            }
            if let Some(tail_text) = read_tail_text(&transcript, 65_536) {
                if is_antigravity_transcript_busy(&tail_text, age) {
                    return true;
                }
            }
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

/// Collects all files under `dir` (up to `max_depth`) modified within `max_age`.
fn collect_recent_files_under(
    dir: &Path,
    max_depth: u8,
    now: SystemTime,
    max_age: Duration,
    out: &mut Vec<(PathBuf, Duration)>,
) {
    if max_depth == 0 || !dir.exists() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        if ft.is_dir() {
            collect_recent_files_under(&path, max_depth - 1, now, max_age, out);
        } else if ft.is_file() {
            if let Ok(meta) = entry.metadata() {
                if let Ok(modified) = meta.modified() {
                    let age = now.duration_since(modified).unwrap_or(Duration::MAX);
                    if age <= max_age {
                        out.push((path, age));
                    }
                }
            }
        }
    }
}

fn most_recent_file_age_under(
    dir: &Path,
    max_depth: u8,
    now: SystemTime,
) -> Option<(PathBuf, Duration)> {
    let mut files = Vec::new();
    collect_recent_files_under(dir, max_depth, now, Duration::MAX, &mut files);
    files.into_iter().min_by_key(|(_, age)| *age)
}

fn check_cursor_busy() -> bool {
    let now = SystemTime::now();
    for home in candidate_home_dirs() {
        let projects_dir = home.join(".cursor").join("projects");
        let Ok(projects) = std::fs::read_dir(projects_dir) else {
            continue;
        };
        for project in projects.flatten() {
            let transcripts_dir = project.path().join("agent-transcripts");
            let mut recent_files = Vec::new();
            // Depth 4 covers both `agent-transcripts/<id>/<id>.jsonl` AND `agent-transcripts/<id>/subagents/<subid>.jsonl`!
            collect_recent_files_under(
                &transcripts_dir,
                4,
                now,
                Duration::from_secs(3600),
                &mut recent_files,
            );
            for (path, age) in recent_files {
                if path.extension().is_some_and(|e| e == "jsonl") {
                    if let Some(last_line) = read_last_non_empty_line(&path) {
                        if is_cursor_transcript_busy(&last_line, age) {
                            return true;
                        }
                    }
                }
            }
        }
    }
    false
}

fn check_claude_busy() -> bool {
    let now = SystemTime::now();
    for home in candidate_home_dirs() {
        let projects_dir = home.join(".claude").join("projects");
        let mut recent_files = Vec::new();
        collect_recent_files_under(
            &projects_dir,
            4,
            now,
            Duration::from_secs(1800),
            &mut recent_files,
        );
        for (path, age) in recent_files {
            if path.extension().is_some_and(|e| e == "jsonl") {
                if let Some(last_line) = read_last_non_empty_line(&path) {
                    if is_claude_transcript_busy(&last_line, age) {
                        return true;
                    }
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

        let tool_call_line =
            r#"{"step_index":11,"type":"PLANNER_RESPONSE","tool_calls":[{"id":"1"}]}"#;
        assert!(is_antigravity_transcript_busy(
            tool_call_line,
            Duration::from_secs(30)
        ));

        let truncated_tool_call =
            r#"{"step_index":12,"type":"PLANNER_RESPONSE","truncated_fields":["tool_calls"]}"#;
        assert!(is_antigravity_transcript_busy(
            truncated_tool_call,
            Duration::from_secs(30)
        ));

        let generic_tool_output =
            r#"{"step_index":13,"type":"GENERIC","status":"DONE","content":"Created file."}"#;
        assert!(is_antigravity_transcript_busy(
            generic_tool_output,
            Duration::from_secs(45)
        ));

        let running_bg_task_with_interim_response = concat!(
            r#"{"step_index":629,"type":"GENERIC","status":"RUNNING","content":"Tool is running as a background task with task id: conv-1/task-629\nLog output"}"#,
            "\n",
            r#"{"step_index":630,"type":"PLANNER_RESPONSE","status":"DONE","content":"It is rebuilding now — I will update you when it finishes!"}"#
        );
        assert!(is_antigravity_transcript_busy(
            running_bg_task_with_interim_response,
            Duration::from_secs(40)
        ));

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
    fn cursor_stays_busy_when_subagent_completes_but_parent_or_other_subagent_is_running() {
        // Parent transcript is waiting on subagents (no turn_ended yet, modified 11 mins ago)
        let parent_last_line =
            r#"{"role":"assistant","message":{"content":[{"type":"tool_use","name":"Task"}]}}"#;
        // Subagent T1 just finished and wrote turn_ended (modified 1 sec ago)
        let finished_subagent_t1 = r#"{"type":"turn_ended","status":"success"}"#;
        // Subagent T2 is still running tests (modified 2 secs ago)
        let running_subagent_t2 =
            r#"{"role":"assistant","message":{"content":[{"type":"tool_use","name":"Shell"}]}}"#;

        // Even though T1 finished most recently, checking ALL transcripts keeps Cursor busy!
        assert!(is_any_cursor_transcript_busy([
            (finished_subagent_t1, Duration::from_secs(1)),
            (running_subagent_t2, Duration::from_secs(2)),
            (parent_last_line, Duration::from_secs(660)),
        ]));

        // Even if T1 and T2 both finished, parent is still open (hasn't written turn_ended yet):
        assert!(is_any_cursor_transcript_busy([
            (finished_subagent_t1, Duration::from_secs(5)),
            (parent_last_line, Duration::from_secs(700)),
        ]));

        // Only when BOTH subagents AND the parent transcript have written turn_ended is Cursor done!
        let finished_parent = r#"{"type":"turn_ended","status":"success"}"#;
        assert!(!is_any_cursor_transcript_busy([
            (finished_subagent_t1, Duration::from_secs(15)),
            (finished_parent, Duration::from_secs(1)),
        ]));
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
