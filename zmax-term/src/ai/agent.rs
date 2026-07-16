//! Autonomous AI agent: a tool-use loop that lets the model read/list/write files and (opt-in) run
//! commands to accomplish a task. The headline of zmax' "CLI IDE with AI agents".
//!
//! The loop runs entirely off the UI thread (blocking IO + network). File access is confined to the
//! workspace root; shell execution is gated behind `ZMAX_AI_AGENT_ALLOW_EXEC=1` (off by default).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::{Content, Tool, Turn};

const MAX_STEPS: usize = 16;
const MAX_TOOL_OUTPUT: usize = 16_000;

/// Review (dry-run) mode: when on, the agent still reasons and reports what it *would* do, but
/// `write_file`/`run_command` make no changes — they report the proposed action instead. This lets
/// the user review the agent's plan and proposed edits before applying them (Cursor's review&apply).
static DRY_RUN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether the agent is in review (dry-run) mode.
pub fn dry_run() -> bool {
    DRY_RUN.load(std::sync::atomic::Ordering::Relaxed)
}

/// Toggle review (dry-run) mode; returns the new state.
pub fn toggle_dry_run() -> bool {
    let n = !dry_run();
    DRY_RUN.store(n, std::sync::atomic::Ordering::Relaxed);
    n
}

const SYSTEM: &str = "You are an autonomous coding agent embedded in the zmax editor. \
You can read, list, and write files in the user's workspace, and (when enabled) run shell commands. \
Work step by step: inspect what you need, make the edits, and stop when the task is done. \
Prefer small, correct edits. When writing a file, write its full new contents. \
When finished, give a one-paragraph summary of what you changed.";

/// Outcome of an agent run.
pub struct AgentResult {
    pub transcript: String,
    pub changed_files: BTreeSet<PathBuf>,
    pub steps: usize,
    /// A `git stash create` SHA snapshot of the workspace taken before editing (if in a git repo
    /// with changes), so the run can be reverted. None if not applicable.
    pub checkpoint: Option<String>,
}

/// Snapshot the working tree without modifying it (`git stash create`), returning the commit SHA.
fn make_checkpoint(root: &Path) -> Option<String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["stash", "create", "ai-agent checkpoint"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let sha = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!sha.is_empty()).then_some(sha)
}

/// The tools exposed to the agent.
fn tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "read_file".into(),
            description: "Read a UTF-8 text file in the workspace. Input: {\"path\": string}.".into(),
            input_schema: serde_json::json!({
                "type":"object",
                "properties":{"path":{"type":"string"}},
                "required":["path"]
            }),
        },
        Tool {
            name: "list_dir".into(),
            description: "List entries of a directory in the workspace. Input: {\"path\": string}.".into(),
            input_schema: serde_json::json!({
                "type":"object",
                "properties":{"path":{"type":"string"}},
                "required":["path"]
            }),
        },
        Tool {
            name: "write_file".into(),
            description: "Create or overwrite a file with full new contents. Input: {\"path\": string, \"content\": string}.".into(),
            input_schema: serde_json::json!({
                "type":"object",
                "properties":{"path":{"type":"string"},"content":{"type":"string"}},
                "required":["path","content"]
            }),
        },
        Tool {
            name: "run_command".into(),
            description: "Run a shell command in the workspace root (disabled unless the user enabled it). Input: {\"command\": string}.".into(),
            input_schema: serde_json::json!({
                "type":"object",
                "properties":{"command":{"type":"string"}},
                "required":["command"]
            }),
        },
        Tool {
            name: "update_plan".into(),
            description: "Record or update your task plan as an ordered list of steps; call this first and whenever the plan changes. Input: {\"steps\": [string]}.".into(),
            input_schema: serde_json::json!({
                "type":"object",
                "properties":{"steps":{"type":"array","items":{"type":"string"}}},
                "required":["steps"]
            }),
        },
    ]
}

/// Resolve a tool-supplied path against `root`, rejecting anything that escapes the workspace.
fn safe_path(root: &Path, p: &str) -> Result<PathBuf, String> {
    let joined = root.join(p);
    let root_c = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    // For existing paths, canonicalize the path itself; for new files, canonicalize the parent.
    let check = match joined.canonicalize() {
        Ok(c) => c,
        Err(_) => {
            let parent = joined.parent().unwrap_or(&joined);
            let parent_c = parent
                .canonicalize()
                .map_err(|_| format!("path '{p}': parent directory does not exist"))?;
            parent_c.join(joined.file_name().unwrap_or_default())
        }
    };
    if !check.starts_with(&root_c) {
        return Err(format!("path '{p}' escapes the workspace"));
    }
    Ok(joined)
}

fn truncate(mut s: String) -> String {
    if s.len() > MAX_TOOL_OUTPUT {
        s.truncate(MAX_TOOL_OUTPUT);
        s.push_str("\n…(truncated)");
    }
    s
}

/// Execute one tool call. Returns `(output, is_error)`.
fn exec_tool(
    root: &Path,
    name: &str,
    input: &serde_json::Value,
    changed: &mut BTreeSet<PathBuf>,
) -> (String, bool) {
    match name {
        "read_file" => {
            let p = input["path"].as_str().unwrap_or("");
            match safe_path(root, p)
                .and_then(|pb| std::fs::read_to_string(&pb).map_err(|e| format!("read '{p}': {e}")))
            {
                Ok(c) => (truncate(c), false),
                Err(e) => (e, true),
            }
        }
        "list_dir" => {
            let p = input["path"].as_str().unwrap_or(".");
            match safe_path(root, p) {
                Ok(pb) => match std::fs::read_dir(&pb) {
                    Ok(rd) => {
                        let mut names: Vec<String> = rd
                            .filter_map(|e| e.ok())
                            .map(|e| {
                                let n = e.file_name().to_string_lossy().into_owned();
                                if e.path().is_dir() {
                                    format!("{n}/")
                                } else {
                                    n
                                }
                            })
                            .collect();
                        names.sort();
                        (truncate(names.join("\n")), false)
                    }
                    Err(e) => (format!("list '{p}': {e}"), true),
                },
                Err(e) => (e, true),
            }
        }
        "write_file" => {
            let p = input["path"].as_str().unwrap_or("");
            let content = input["content"].as_str().unwrap_or("");
            match safe_path(root, p) {
                Ok(pb) => {
                    if dry_run() {
                        return (
                            format!(
                                "[review] would write {} ({} bytes) — not applied",
                                p,
                                content.len()
                            ),
                            false,
                        );
                    }
                    if let Some(parent) = pb.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    match std::fs::write(&pb, content) {
                        Ok(()) => {
                            changed.insert(pb.clone());
                            (format!("wrote {} ({} bytes)", p, content.len()), false)
                        }
                        Err(e) => (format!("write '{p}': {e}"), true),
                    }
                }
                Err(e) => (e, true),
            }
        }
        "run_command" => {
            if std::env::var("ZMAX_AI_AGENT_ALLOW_EXEC").ok().as_deref() != Some("1") {
                return (
                    "command execution is disabled (set ZMAX_AI_AGENT_ALLOW_EXEC=1 to enable)"
                        .into(),
                    true,
                );
            }
            let cmd = input["command"].as_str().unwrap_or("");
            if dry_run() {
                return (format!("[review] would run `{cmd}` — not executed"), false);
            }
            match std::process::Command::new("sh")
                .arg("-c")
                .arg(cmd)
                .current_dir(root)
                .output()
            {
                Ok(o) => {
                    let mut out = String::from_utf8_lossy(&o.stdout).into_owned();
                    let err = String::from_utf8_lossy(&o.stderr);
                    if !err.trim().is_empty() {
                        out.push_str("\n[stderr]\n");
                        out.push_str(&err);
                    }
                    (truncate(out), !o.status.success())
                }
                Err(e) => (format!("run '{cmd}': {e}"), true),
            }
        }
        "update_plan" => {
            let steps: Vec<String> = input["steps"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|s| s.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            if steps.is_empty() {
                return ("update_plan: no steps".into(), true);
            }
            let plan = steps
                .iter()
                .enumerate()
                .map(|(i, s)| format!("{}. {s}", i + 1))
                .collect::<Vec<_>>()
                .join("\n");
            (plan, false)
        }
        other => (format!("unknown tool '{other}'"), true),
    }
}

/// Run the agent loop to completion (or `MAX_STEPS`). Blocking — call from `spawn_blocking`.
pub fn run(task: String, root: PathBuf) -> Result<AgentResult, String> {
    let provider = super::provider()?;
    if !provider.supports_tools() {
        return Err(format!(
            "the '{}' provider has no agent tool-use yet (set ZMAX_AI_PROVIDER=anthropic)",
            provider.name()
        ));
    }
    let checkpoint = make_checkpoint(&root);
    let tools = tools();
    let base_system = if dry_run() {
        format!(
            "{SYSTEM}\n\nREVIEW MODE: file writes and commands are NOT applied — they are recorded \
             for the user to review. Still call write_file with the full proposed contents and \
             run_command with the exact command, then finish with a summary of every change you \
             are proposing so the user can decide whether to apply them."
        )
    } else {
        SYSTEM.to_string()
    };
    let system = super::system_with_rules(&base_system);
    let mut turns = vec![Turn::user_text(task)];
    let mut transcript = String::new();
    let mut changed = BTreeSet::new();
    let mut steps = 0;

    while steps < MAX_STEPS {
        steps += 1;
        let reply = provider.agent_turn(Some(&system), &turns, &tools)?;
        if !reply.text.trim().is_empty() {
            transcript.push_str(reply.text.trim());
            transcript.push_str("\n\n");
        }
        if reply.tool_uses.is_empty() {
            break; // model is done
        }
        // Record the assistant turn (text + tool_use blocks) so the model sees its own calls.
        let mut acontent: Vec<Content> = Vec::new();
        if !reply.text.is_empty() {
            acontent.push(Content::Text(reply.text.clone()));
        }
        for tu in &reply.tool_uses {
            acontent.push(Content::ToolUse(tu.clone()));
        }
        turns.push(Turn {
            role: super::Role::Assistant,
            content: acontent,
        });
        // Execute the tools and feed the results back as a user turn.
        let mut results: Vec<Content> = Vec::new();
        for tu in &reply.tool_uses {
            let (out, is_error) = exec_tool(&root, &tu.name, &tu.input, &mut changed);
            transcript.push_str(&format!(
                "→ {}({}) {}\n",
                tu.name,
                tu.input
                    .get("path")
                    .or_else(|| tu.input.get("command"))
                    .and_then(|v| v.as_str())
                    .unwrap_or(""),
                if is_error { "[error]" } else { "[ok]" }
            ));
            if tu.name == "update_plan" && !is_error {
                transcript.push_str(&format!("Plan:\n{out}\n"));
            }
            results.push(Content::ToolResult {
                tool_use_id: tu.id.clone(),
                content: out,
                is_error,
            });
        }
        turns.push(Turn {
            role: super::Role::User,
            content: results,
        });
    }

    Ok(AgentResult {
        transcript,
        changed_files: changed,
        steps,
        checkpoint,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serializes tests that touch the process-global DRY_RUN flag so they don't race.
    static DRY_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn safe_path_rejects_escape() {
        let root = std::env::temp_dir();
        assert!(safe_path(&root, "../etc/passwd").is_err());
    }

    #[test]
    fn read_write_roundtrip_and_changed_tracking() {
        let _g = DRY_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("zai-agent-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut changed = BTreeSet::new();
        let (msg, err) = exec_tool(
            &dir,
            "write_file",
            &serde_json::json!({"path":"a.txt","content":"hello"}),
            &mut changed,
        );
        assert!(!err, "{msg}");
        assert_eq!(changed.len(), 1);
        let (out, err) = exec_tool(
            &dir,
            "read_file",
            &serde_json::json!({"path":"a.txt"}),
            &mut changed,
        );
        assert!(!err);
        assert_eq!(out, "hello");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn dry_run_reports_without_writing() {
        let _g = DRY_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("zai-agent-dry-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(!dry_run());
        assert!(toggle_dry_run());
        let mut changed = BTreeSet::new();
        let (msg, err) = exec_tool(
            &dir,
            "write_file",
            &serde_json::json!({"path":"dry.txt","content":"nope"}),
            &mut changed,
        );
        assert!(!err, "{msg}");
        assert!(msg.contains("review"), "{msg}");
        assert!(changed.is_empty(), "dry-run must not record changes");
        assert!(
            !dir.join("dry.txt").exists(),
            "dry-run must not write the file"
        );
        assert!(!toggle_dry_run()); // reset to off so other tests are unaffected
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn run_command_gated_off_by_default() {
        std::env::remove_var("ZMAX_AI_AGENT_ALLOW_EXEC");
        let root = std::env::temp_dir();
        let (msg, err) = exec_tool(
            &root,
            "run_command",
            &serde_json::json!({"command":"echo hi"}),
            &mut BTreeSet::new(),
        );
        assert!(err);
        assert!(msg.contains("disabled"));
    }
}
