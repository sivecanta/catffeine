//! Background AI Agent Activity Detector (`Claude`, `Codex`, `Antigravity`, `Cursor`).
//!
//! Tracks a single user-selected agent and detects when it is actively working vs. finished
//! across both native OS home directories (`USERPROFILE` / `HOME`) and WSL (`\\wsl.localhost\<distro>\home\<user>`):
//! - **Antigravity**: Inspects ALL recently active `~/.gemini/antigravity/brain/*/.system_generated/logs/transcript.jsonl`
//!   files (main conversation + any subagents) plus `.system_generated/tasks/*.log` files.
//! - **Cursor**: Inspects ALL recently active `~/.cursor/projects/*/agent-transcripts/**/*.jsonl` files
//!   (both the parent conversation AND all `subagents/*.jsonl`).
//!   Cursor writes `{"type":"turn_ended","status":"..."}` ONLY when a transcript's turn has finished.
//! - **Claude (`Claude Code`)**: Inspects BOTH:
//!   1. All recent `~/.claude/projects/*/*.jsonl` main session transcripts, scanning backwards to find
//!      the latest conversational event (`"type":"user"`, `"type":"assistant"`, `"type":"api-request"`).
//!      Claude Code appends housekeeping entries (`"cost-state"`, `"last-prompt"`, `"ai-title"`, `"mode"`,
//!      `"atis-latch"`, `"attachment"`) during and after turns — so looking only at the very last line
//!      would misclassify a mid-turn `"cost-state"` or `"attachment"`! A main session is only idle when
//!      its last conversational step is an `"assistant"` message with `"stop_reason":"end_turn"` (or
//!      `"stop_sequence"`) and no subsequent `"user"` / `"api-request"` / `"tool_use"` step.
//!   2. All recent `~/.claude/projects/*/subagents/*.meta.json` files: Claude Code writes
//!      `"finished":false` while a subagent is running and updates it to `"finished":true` when done!
//! - **Codex (`OpenAI Codex`)**: Inspects `~/.codex/sessions/**/*.jsonl` (checking for `"turn.completed"` /
//!   `"task_complete"`), active `.codex/thread-writer-locks`, `.codex/process_manager`, and active `codex`
//!   processes — while ignoring background idle telemetry (`logs_2.sqlite` / `models_cache.json`).

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

/// Pure classifier for a collection of Cursor transcript `(last_line, age)` pairs.
pub fn is_any_cursor_transcript_busy<'a, I>(transcripts: I) -> bool
where
    I: IntoIterator<Item = (&'a str, Duration)>,
{
    transcripts
        .into_iter()
        .any(|(last_line, age)| is_cursor_transcript_busy(last_line, age))
}

/// Pure classifier for the tail of a Claude Code main session `.jsonl` file.
///
/// Why we scan the tail instead of only the last line:
/// Claude Code frequently writes non-conversational metadata lines (`"type":"cost-state"`,
/// `"type":"last-prompt"`, `"type":"ai-title"`, `"type":"mode"`, `"type":"atis-latch"`,
/// `"type":"attachment"`, `"type":"bridge-session"`, `"type":"file-history-snapshot"`) both
/// in the middle of a turn (right after `"api-request"`) and after `"end_turn"`.
///
/// By scanning backwards from the end of the tail for the most recent conversational event:
/// - If we hit `"subtype":"turn_duration"` or `"subtype":"away_summary"`, or an `"assistant"`
///   message with `"stop_reason":"end_turn"` / `"stop_sequence"`, the turn is finished!
/// - If we hit `"stop_reason":"tool_use"`, `"type":"user"`, `"type":"api-request"`, or
///   `"type":"api-request-blob"`, Claude is still actively working on the turn!
pub fn is_claude_transcript_busy(tail_text: &str, age: Duration) -> bool {
    if age > Duration::from_secs(1800) {
        return false;
    }
    if age <= Duration::from_secs(4) {
        return true;
    }

    for raw_line in tail_text.lines().rev() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }

        // Explicit turn completion markers in Claude Code:
        if line.contains(r#""subtype":"turn_duration""#)
            || line.contains(r#""subtype": "turn_duration""#)
            || line.contains(r#""type":"turn_ended""#)
            || line.contains(r#""stop_reason":"end_turn""#)
            || line.contains(r#""stop_reason": "end_turn""#)
            || line.contains(r#""stop_reason":"stop_sequence""#)
        {
            return false;
        }

        // Explicit mid-turn markers (waiting on API response or tool execution):
        if line.contains(r#""stop_reason":"tool_use""#)
            || line.contains(r#""stop_reason": "tool_use""#)
            || line.contains(r#""type":"api-request""#)
            || line.contains(r#""type":"api-request-blob""#)
            || line.contains(r#""type":"user""#)
            || line.contains(r#""type": "user""#)
            || line.contains(r#""type":"tool_result""#)
        {
            return true;
        }
        // Ignore housekeeping lines ("cost-state", "last-prompt", "ai-title", "mode", "attachment", etc.)
        // and continue scanning backwards!
    }

    false
}

/// Pure classifier for a Claude Code subagent `.meta.json` file.
///
/// Claude Code creates `subagents/agent-<id>.meta.json` for every spawned subagent and sets
/// `"finished":true` only when that subagent finishes!
pub fn is_claude_subagent_meta_busy(meta_json: &str, age: Duration) -> bool {
    if age > Duration::from_secs(3600) {
        return false;
    }
    let trimmed = meta_json.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.contains(r#""finished":true"#) || trimmed.contains(r#""finished": true"#) {
        return false;
    }
    true
}

/// Pure classifier for the tail of a Codex `.jsonl` session / rollout file.
///
/// Codex streams JSONL events (`thread.started`, `turn.started`, `item.started`, `item.completed`)
/// and writes `"type":"turn.completed"` or `"type":"task_complete"` when the turn finishes.
pub fn is_codex_transcript_busy(tail_text: &str, age: Duration) -> bool {
    if age > Duration::from_secs(1800) {
        return false;
    }
    if age <= Duration::from_secs(4) {
        return true;
    }
    for raw_line in tail_text.lines().rev() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        if line.contains(r#""type":"turn.completed""#)
            || line.contains(r#""type": "turn.completed""#)
            || line.contains(r#""type":"turn.failed""#)
            || line.contains(r#""type":"task_complete""#)
            || line.contains(r#""type":"turn_aborted""#)
        {
            return false;
        }
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
            Duration::from_secs(3600),
            &mut recent_files,
        );
        for (path, age) in recent_files {
            let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let is_in_subagents = path
                .parent()
                .and_then(|p| p.file_name())
                .is_some_and(|n| n == "subagents");

            if is_in_subagents {
                // For Claude Code subagents, check `agent-*.meta.json` which has `"finished": true/false`!
                if file_name.ends_with(".meta.json") {
                    if let Ok(meta_text) = std::fs::read_to_string(&path) {
                        if is_claude_subagent_meta_busy(&meta_text, age) {
                            return true;
                        }
                    }
                }
            } else if file_name.ends_with(".jsonl") {
                // Main Claude Code session JSONL: scan the tail backwards past housekeeping records
                if let Some(tail_text) = read_tail_text(&path, 32_768) {
                    if is_claude_transcript_busy(&tail_text, age) {
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
        if !codex_dir.exists() {
            continue;
        }

        // 1. Check active thread writer locks (`~/.codex/thread-writer-locks`)
        let locks_dir = codex_dir.join("thread-writer-locks");
        if let Some((_lock, age)) = most_recent_file_age_under(&locks_dir, 2, now) {
            if age <= Duration::from_secs(120) {
                return true;
            }
        }

        // 2. Check active process manager entries (`~/.codex/process_manager`)
        let proc_dir = codex_dir.join("process_manager");
        if let Some((_proc_file, age)) = most_recent_file_age_under(&proc_dir, 2, now) {
            if age <= Duration::from_secs(60) {
                return true;
            }
        }

        // 3. Check `~/.codex/sessions/**/*.jsonl` rollout files (uncompressed active sessions)
        let sessions_dir = codex_dir.join("sessions");
        let mut session_files = Vec::new();
        collect_recent_files_under(
            &sessions_dir,
            5,
            now,
            Duration::from_secs(1800),
            &mut session_files,
        );
        for (path, age) in session_files {
            if path.extension().is_some_and(|e| e == "jsonl") {
                if let Some(tail_text) = read_tail_text(&path, 32_768) {
                    if is_codex_transcript_busy(&tail_text, age) {
                        return true;
                    }
                }
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
        let parent_last_line =
            r#"{"role":"assistant","message":{"content":[{"type":"tool_use","name":"Task"}]}}"#;
        let finished_subagent_t1 = r#"{"type":"turn_ended","status":"success"}"#;
        let running_subagent_t2 =
            r#"{"role":"assistant","message":{"content":[{"type":"tool_use","name":"Shell"}]}}"#;

        assert!(is_any_cursor_transcript_busy([
            (finished_subagent_t1, Duration::from_secs(1)),
            (running_subagent_t2, Duration::from_secs(2)),
            (parent_last_line, Duration::from_secs(660)),
        ]));

        assert!(is_any_cursor_transcript_busy([
            (finished_subagent_t1, Duration::from_secs(5)),
            (parent_last_line, Duration::from_secs(700)),
        ]));

        let finished_parent = r#"{"type":"turn_ended","status":"success"}"#;
        assert!(!is_any_cursor_transcript_busy([
            (finished_subagent_t1, Duration::from_secs(15)),
            (finished_parent, Duration::from_secs(1)),
        ]));
    }

    #[test]
    fn claude_scans_past_housekeeping_lines_and_checks_subagent_meta() {
        // Mid-turn: api-request followed by housekeeping lines ("last-prompt", "ai-title", "attachment")
        let mid_turn_with_housekeeping = concat!(
            r#"{"type":"user","message":{"content":[{"type":"tool_result"}]}}"#,
            "\n",
            r#"{"type":"api-request","data":{}}"#,
            "\n",
            r#"{"type":"last-prompt","text":"fix bug"}"#,
            "\n",
            r#"{"type":"ai-title","title":"Fixing bug"}"#,
            "\n",
            r#"{"type":"attachment","data":{}}"#
        );
        assert!(is_claude_transcript_busy(
            mid_turn_with_housekeeping,
            Duration::from_secs(25)
        ));

        // Finished turn: assistant with stop_reason="end_turn" followed by "turn_duration" and "cost-state"
        let finished_turn_with_housekeeping = concat!(
            r#"{"type":"assistant","message":{"stop_reason":"end_turn","content":[{"type":"text","text":"Fixed!"}]}}"#,
            "\n",
            r#"{"type":"system","subtype":"turn_duration","durationMs":4200}"#,
            "\n",
            r#"{"type":"last-prompt","text":"fix bug"}"#,
            "\n",
            r#"{"type":"cost-state","cost":0.05}"#
        );
        assert!(!is_claude_transcript_busy(
            finished_turn_with_housekeeping,
            Duration::from_secs(15)
        ));

        // Subagent meta.json:
        let running_meta =
            r#"{"agentType":"general-purpose","description":"T1","finished":false}"#;
        assert!(is_claude_subagent_meta_busy(
            running_meta,
            Duration::from_secs(120)
        ));

        let finished_meta =
            r#"{"agentType":"general-purpose","description":"T1","finished":true}"#;
        assert!(!is_claude_subagent_meta_busy(
            finished_meta,
            Duration::from_secs(5)
        ));
    }

    #[test]
    fn codex_transcript_busy_until_turn_completed() {
        let mid_turn = concat!(
            r#"{"type":"turn.started"}"#,
            "\n",
            r#"{"type":"item.completed","item":{"type":"command_execution","command":"pytest"}}"#
        );
        assert!(is_codex_transcript_busy(mid_turn, Duration::from_secs(20)));

        let completed_turn = concat!(
            r#"{"type":"item.completed","item":{"type":"agent_message","text":"All tests pass."}}"#,
            "\n",
            r#"{"type":"turn.completed","usage":{"input_tokens":100,"output_tokens":50}}"#
        );
        assert!(!is_codex_transcript_busy(
            completed_turn,
            Duration::from_secs(10)
        ));
    }
}
