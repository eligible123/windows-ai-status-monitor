use crate::config::{AgentConfig, AppConfig};
#[cfg(not(windows))]
use anyhow::Context;
use anyhow::Result;
use chrono::{DateTime, Local, Utc};
use serde::Serialize;
use serde_json::Value;
#[cfg(not(windows))]
use std::process::Command;
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime},
};
#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
    System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    },
    System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum AgentState {
    Running,
    Waiting,
    Idle,
    Offline,
}

impl std::fmt::Display for AgentState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentState::Running => write!(f, "RUNNING"),
            AgentState::Waiting => write!(f, "WAITING"),
            AgentState::Idle => write!(f, "IDLE"),
            AgentState::Offline => write!(f, "OFFLINE"),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct StatusPayload {
    pub agent: String,
    pub state: AgentState,
    pub detail: String,
    pub started_at: Option<String>,
    pub updated_at: String,
    pub duration_secs: u64,
    pub pid_count: usize,
}

#[derive(Debug, Clone)]
pub struct MonitorRuntime {
    pub config: AppConfig,
    pub status: StatusPayload,
    last_state: AgentState,
    state_started_at: DateTime<Utc>,
    last_output_at: Option<Instant>,
}

#[derive(Debug, Clone)]
struct ProcessHit {
    name: String,
    pid: String,
    title: String,
    active_child: Option<String>,
}

#[derive(Debug, Clone)]
struct LogProbe {
    text: String,
    modified_recently: bool,
}

#[derive(Debug, Clone)]
struct SessionProbe {
    state: Option<AgentState>,
    detail: String,
    modified_recently: bool,
    pending_tool: Option<String>,
}

impl MonitorRuntime {
    pub fn new(config: AppConfig) -> Self {
        let now = Utc::now();
        let agent = config.active_agent();
        let status = StatusPayload {
            agent: agent.label,
            state: AgentState::Idle,
            detail: "Monitor initialized".to_string(),
            started_at: Some(now.to_rfc3339()),
            updated_at: now.to_rfc3339(),
            duration_secs: 0,
            pid_count: 0,
        };

        Self {
            config,
            status,
            last_state: AgentState::Idle,
            state_started_at: now,
            last_output_at: None,
        }
    }

    pub fn reload_config(&mut self, config: AppConfig) {
        self.config = config;
        self.status.detail = "Configuration reloaded".to_string();
    }

    pub fn poll(&mut self) -> Result<(StatusPayload, bool)> {
        let agent = self.config.active_agent();
        let process_hits = find_processes(&agent)?;
        let log_probe = read_logs(&agent, self.config.idle_timeout_secs);
        let session_probe = read_session_probe(&agent, self.config.idle_timeout_secs);
        let now_instant = Instant::now();

        if log_probe.modified_recently || session_probe.modified_recently {
            self.last_output_at = Some(now_instant);
        }

        let lower_log = log_probe.text.to_lowercase();
        let pid_count = process_hits.len();
        let (state, detail) = decide_state(
            &agent,
            &process_hits,
            &session_probe,
            &lower_log,
            self.last_output_at,
            self.config.idle_timeout_secs,
        );

        let changed = state != self.last_state || self.status.agent != agent.label;
        let now = Utc::now();
        if changed {
            self.last_state = state;
            self.state_started_at = now;
        }

        self.status = StatusPayload {
            agent: agent.label,
            state,
            detail,
            started_at: Some(self.state_started_at.to_rfc3339()),
            updated_at: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            duration_secs: now
                .signed_duration_since(self.state_started_at)
                .num_seconds()
                .max(0) as u64,
            pid_count,
        };

        Ok((self.status.clone(), changed))
    }
}

fn decide_state(
    agent: &AgentConfig,
    hits: &[ProcessHit],
    session_probe: &SessionProbe,
    lower_log: &str,
    last_output_at: Option<Instant>,
    idle_timeout_secs: u64,
) -> (AgentState, String) {
    let active_child = hits.iter().find_map(|hit| hit.active_child.as_ref());

    if let Some(tool) = &session_probe.pending_tool {
        if let Some(activity) = active_child {
            return (
                AgentState::Running,
                format!("Claude tool {tool} is executing via {activity}"),
            );
        }

        if !hits.is_empty() {
            return (
                AgentState::Waiting,
                format!("Claude is waiting for approval to run {tool}"),
            );
        }
    }

    if let Some(state) = session_probe.state {
        if !hits.is_empty() || state == AgentState::Idle {
            return (state, session_probe.detail.clone());
        }
    }

    if !lower_log.is_empty() {
        if contains_any(lower_log, &agent.waiting_patterns) {
            return (
                AgentState::Waiting,
                "Waiting keyword detected in configured log".to_string(),
            );
        }

        if contains_any(lower_log, &agent.running_patterns) {
            return (
                AgentState::Running,
                "Running keyword detected in configured log".to_string(),
            );
        }

        if contains_any(lower_log, &agent.done_patterns) {
            return (
                AgentState::Idle,
                "Completion keyword detected in configured log".to_string(),
            );
        }
    }

    if let Some(last_output_at) = last_output_at {
        if last_output_at.elapsed() <= Duration::from_secs(idle_timeout_secs) && !hits.is_empty() {
            return (
                AgentState::Running,
                "Recent configured log activity".to_string(),
            );
        }
    }

    if hits.is_empty() {
        return (AgentState::Offline, "No matching process found".to_string());
    }

    if let Some(activity) = active_child {
        return (
            AgentState::Running,
            format!("Active child process detected: {activity}"),
        );
    }

    let summary = hits
        .iter()
        .map(|hit| {
            if hit.title.trim().is_empty() || hit.title == "N/A" {
                format!("{}#{}", hit.name, hit.pid)
            } else {
                format!("{}#{} {}", hit.name, hit.pid, hit.title)
            }
        })
        .take(3)
        .collect::<Vec<_>>()
        .join("; ");

    (AgentState::Idle, format!("Process exists: {summary}"))
}

fn contains_any(haystack: &str, patterns: &[String]) -> bool {
    patterns
        .iter()
        .filter(|pattern| !pattern.trim().is_empty())
        .any(|pattern| haystack.contains(&pattern.to_lowercase()))
}

#[cfg(windows)]
#[derive(Debug, Clone)]
struct SnapshotProcess {
    name: String,
    pid: u32,
    parent_pid: u32,
}

#[cfg(windows)]
fn find_processes(agent: &AgentConfig) -> Result<Vec<ProcessHit>> {
    let processes = snapshot_processes();
    let matched = processes
        .iter()
        .filter_map(|process| {
            if !process_name_matches(agent, &process.name) {
                return None;
            }

            let path = process_path(process.pid);
            if process_context_matches(agent, &process.name, &path, "") {
                Some((process.clone(), path))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    let matched_parent_ids = matched
        .iter()
        .filter(|(process, _)| !is_generic_host(&process.name))
        .map(|(process, _)| process.pid)
        .collect::<HashSet<_>>();

    let mut hits = Vec::new();
    for (process, path) in matched {
        let active_child = if is_activity_process(&process.name) {
            Some(format!("{}#{}", process.name, process.pid))
        } else {
            processes
                .iter()
                .find(|child| {
                    matched_parent_ids.contains(&child.parent_pid)
                        && child.parent_pid == process.pid
                        && is_activity_process(&child.name)
                })
                .map(|child| format!("{}#{}", child.name, child.pid))
        };

        hits.push(ProcessHit {
            name: process.name,
            pid: process.pid.to_string(),
            title: path,
            active_child,
        });
    }

    Ok(hits)
}

#[cfg(windows)]
fn snapshot_processes() -> Vec<SnapshotProcess> {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Vec::new();
    }

    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

    let mut processes = Vec::new();
    let mut has_entry = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;

    while has_entry {
        processes.push(SnapshotProcess {
            name: utf16_to_string(&entry.szExeFile),
            pid: entry.th32ProcessID,
            parent_pid: entry.th32ParentProcessID,
        });
        has_entry = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
    }

    unsafe {
        CloseHandle(snapshot);
    }

    processes
}

#[cfg(windows)]
fn process_path(pid: u32) -> String {
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle == 0 {
        return String::new();
    }

    let mut buffer = vec![0u16; 32768];
    let mut size = buffer.len() as u32;
    let ok = unsafe { QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut size) };

    unsafe {
        CloseHandle(handle);
    }

    if ok == 0 || size == 0 {
        String::new()
    } else {
        String::from_utf16_lossy(&buffer[..size as usize])
    }
}

#[cfg(not(windows))]
fn find_processes(agent: &AgentConfig) -> Result<Vec<ProcessHit>> {
    let mut command = Command::new("tasklist");
    command.args(["/fo", "csv"]);

    let output = command.output().context("run tasklist")?;

    if !output.status.success() {
        return Ok(Vec::new());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut hits = Vec::new();

    for (idx, line) in stdout.lines().enumerate() {
        if idx == 0 || line.trim().is_empty() {
            continue;
        }

        let fields = parse_csv_line(line);
        if fields.len() < 2 {
            continue;
        }

        let name = fields[0].trim().to_string();
        let pid = fields[1].trim().to_string();
        let title = fields.last().cloned().unwrap_or_default();

        if process_name_matches(agent, &name) && process_context_matches(agent, &name, "", &title) {
            hits.push(ProcessHit {
                name,
                pid,
                title,
                active_child: None,
            });
        }
    }

    Ok(hits)
}

fn process_name_matches(agent: &AgentConfig, name: &str) -> bool {
    agent
        .process_names
        .iter()
        .any(|pattern| wildcard_match(pattern, name))
}

fn process_context_matches(agent: &AgentConfig, name: &str, path: &str, title: &str) -> bool {
    let context = format!("{name} {path} {title}").to_lowercase();

    if agent
        .exclude_keywords
        .iter()
        .filter(|keyword| !keyword.trim().is_empty())
        .any(|keyword| context.contains(&keyword.to_lowercase()))
    {
        return false;
    }

    agent.match_keywords.is_empty()
        || agent
            .match_keywords
            .iter()
            .filter(|keyword| !keyword.trim().is_empty())
            .any(|keyword| context.contains(&keyword.to_lowercase()))
}

fn wildcard_match(pattern: &str, value: &str) -> bool {
    let pattern = pattern.to_lowercase();
    let value = value.to_lowercase();

    if pattern == "*" {
        return true;
    }

    let parts = pattern.split('*').collect::<Vec<_>>();
    if parts.len() == 1 {
        return pattern == value;
    }

    let mut remaining = value.as_str();
    for (idx, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }

        if let Some(found) = remaining.find(part) {
            if idx == 0 && !pattern.starts_with('*') && found != 0 {
                return false;
            }
            remaining = &remaining[found + part.len()..];
        } else {
            return false;
        }
    }

    pattern.ends_with('*')
        || parts
            .last()
            .map_or(true, |last| remaining.is_empty() || last.is_empty())
}

fn is_generic_host(name: &str) -> bool {
    matches!(
        name.to_lowercase().as_str(),
        "node.exe"
            | "cmd.exe"
            | "powershell.exe"
            | "pwsh.exe"
            | "bash.exe"
            | "sh.exe"
            | "windowsterminal.exe"
            | "wt.exe"
    )
}

fn is_activity_process(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.starts_with("codex-command-runner")
        || matches!(
            lower.as_str(),
            "cmd.exe"
                | "powershell.exe"
                | "pwsh.exe"
                | "bash.exe"
                | "sh.exe"
                | "git.exe"
                | "node.exe"
                | "npm.exe"
                | "python.exe"
                | "python3.exe"
                | "cargo.exe"
                | "rustc.exe"
        )
}

#[cfg(windows)]
fn utf16_to_string(buffer: &[u16]) -> String {
    let len = buffer
        .iter()
        .position(|ch| *ch == 0)
        .unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..len])
}

#[cfg(not(windows))]
fn parse_csv_line(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '"' if in_quotes && chars.peek() == Some(&'"') => {
                current.push('"');
                chars.next();
            }
            '"' => in_quotes = !in_quotes,
            ',' if !in_quotes => {
                fields.push(current.clone());
                current.clear();
            }
            _ => current.push(ch),
        }
    }

    fields.push(current);
    fields
}

fn read_session_probe(agent: &AgentConfig, idle_timeout_secs: u64) -> SessionProbe {
    if !agent.id.eq_ignore_ascii_case("claude") {
        return empty_session_probe(false);
    }

    let Some(home) = dirs::home_dir() else {
        return empty_session_probe(false);
    };

    let projects_dir = home.join(".claude").join("projects");
    let files = recent_jsonl_files(&projects_dir, 6);
    let mut modified_recently = false;

    for file in files {
        let mut file_modified_recently = false;
        if let Ok(metadata) = fs::metadata(&file) {
            if let Ok(modified) = metadata.modified() {
                if modified.elapsed().unwrap_or_default()
                    <= Duration::from_secs(idle_timeout_secs.max(90))
                {
                    file_modified_recently = true;
                    modified_recently = true;
                }
            }
        }

        if let Ok(raw) = fs::read(&file) {
            let start = raw.len().saturating_sub(262_144);
            let tail = String::from_utf8_lossy(&raw[start..]);
            let mut probe = parse_claude_session_tail(&tail, file_modified_recently);
            if probe.pending_tool.is_some() || probe.state.is_some() {
                probe.modified_recently = probe.modified_recently || modified_recently;
                return probe;
            }
        }
    }

    empty_session_probe(modified_recently)
}

fn empty_session_probe(modified_recently: bool) -> SessionProbe {
    SessionProbe {
        state: None,
        detail: String::new(),
        modified_recently,
        pending_tool: None,
    }
}

fn parse_claude_session_tail(text: &str, modified_recently: bool) -> SessionProbe {
    let mut pending_tools: HashMap<String, String> = HashMap::new();
    let mut pending_order: Vec<String> = Vec::new();
    let mut latest_state: Option<(AgentState, String)> = None;

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let lower_line = line.to_lowercase();
        if contains_any_literal(&lower_line, WAITING_SESSION_PATTERNS) {
            latest_state = Some((
                AgentState::Waiting,
                "Claude is waiting for command approval".to_string(),
            ));
        } else if contains_any_literal(&lower_line, DONE_SESSION_PATTERNS) {
            latest_state = Some((
                AgentState::Idle,
                "Claude session reached an end turn".to_string(),
            ));
        } else if contains_any_literal(&lower_line, RUNNING_SESSION_PATTERNS) {
            latest_state = Some((
                AgentState::Running,
                "Claude session has active tool/task activity".to_string(),
            ));
        }

        let Ok(event) = serde_json::from_str::<Value>(line) else {
            continue;
        };

        let event_type = event
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if event_type == "permission-mode" {
            continue;
        }

        let Some(message) = event.get("message") else {
            continue;
        };

        if let Some(content) = message.get("content").and_then(Value::as_array) {
            for item in content {
                let item_type = item.get("type").and_then(Value::as_str).unwrap_or_default();
                match item_type {
                    "tool_use" => {
                        let Some(id) = item.get("id").and_then(Value::as_str) else {
                            continue;
                        };
                        let name = item
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("tool")
                            .to_string();
                        let id = id.to_string();
                        pending_tools.insert(id.clone(), name.clone());
                        if !pending_order.iter().any(|pending_id| pending_id == &id) {
                            pending_order.push(id);
                        }
                        latest_state =
                            Some((AgentState::Running, format!("Claude requested {name}")));
                    }
                    "tool_result" => {
                        if let Some(id) = item.get("tool_use_id").and_then(Value::as_str) {
                            pending_tools.remove(id);
                            pending_order.retain(|pending_id| pending_id != id);
                        }
                        latest_state = Some((
                            AgentState::Running,
                            "Claude is processing tool output".to_string(),
                        ));
                    }
                    "thinking" => {
                        latest_state =
                            Some((AgentState::Running, "Claude is thinking".to_string()));
                    }
                    _ => {}
                }
            }
        }

        if event_type == "assistant" {
            let stop_reason = message
                .get("stop_reason")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if matches!(stop_reason, "end_turn" | "stop_sequence") {
                latest_state = Some((
                    AgentState::Idle,
                    "Claude session reached an end turn".to_string(),
                ));
            }
        }
    }

    let pending_tool = pending_order
        .iter()
        .rev()
        .find_map(|id| pending_tools.get(id))
        .cloned();
    let (state, detail) = latest_state
        .map(|(state, detail)| (Some(state), detail))
        .unwrap_or_else(|| (None, String::new()));

    SessionProbe {
        state,
        detail,
        modified_recently,
        pending_tool,
    }
}

const WAITING_SESSION_PATTERNS: &[&str] = &[
    "do you want to proceed",
    "yes, and don't ask again",
    "esc to cancel",
    "tab to amend",
    "ctrl+e to explain",
    "allow command",
    "approve action",
    "approval",
    "approve",
    "allow",
    "confirm",
];

const RUNNING_SESSION_PATTERNS: &[&str] = &[
    r#""name":"bash""#,
    r#""type":"tool_use""#,
    r#""type":"thinking""#,
    "<status>running</status>",
    r#""status":"running""#,
    r#""operation":"enqueue""#,
    "not_ready",
];

const DONE_SESSION_PATTERNS: &[&str] = &[
    r#""stop_reason":"end_turn""#,
    r#""stop_reason":"stop_sequence""#,
    "task completed",
    "finished successfully",
];

fn contains_any_literal(haystack: &str, patterns: &[&str]) -> bool {
    patterns.iter().any(|pattern| haystack.contains(pattern))
}

fn recent_jsonl_files(root: &Path, limit: usize) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_jsonl_files(root, &mut files);
    files.sort_by(|left, right| right.0.cmp(&left.0));
    files
        .into_iter()
        .take(limit)
        .map(|(_, path)| path)
        .collect()
}

fn collect_jsonl_files(root: &Path, files: &mut Vec<(SystemTime, PathBuf)>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl_files(&path, files);
            continue;
        }

        if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
            continue;
        }

        let modified = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        files.push((modified, path));
    }
}
fn read_logs(agent: &AgentConfig, idle_timeout_secs: u64) -> LogProbe {
    let mut combined = String::new();
    let mut modified_recently = false;

    for path in &agent.log_files {
        let path = expand_env_path(path);
        if !path.exists() {
            continue;
        }

        if let Ok(metadata) = fs::metadata(&path) {
            if let Ok(modified) = metadata.modified() {
                if modified.elapsed().unwrap_or_default() <= Duration::from_secs(idle_timeout_secs)
                {
                    modified_recently = true;
                }
            }
        }

        if let Ok(raw) = fs::read(&path) {
            let start = raw.len().saturating_sub(8192);
            combined.push_str(&String::from_utf8_lossy(&raw[start..]));
            combined.push('\n');
        }
    }

    LogProbe {
        text: combined,
        modified_recently,
    }
}

fn expand_env_path(path: &str) -> PathBuf {
    let mut expanded = path.to_string();
    for (key, value) in std::env::vars() {
        let token = format!("%{key}%");
        if expanded.contains(&token) {
            expanded = expanded.replace(&token, &value);
        }
    }
    PathBuf::from(expanded)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claude_agent() -> AgentConfig {
        AgentConfig {
            id: "claude".to_string(),
            label: "CLAUDE".to_string(),
            process_names: vec!["claude.exe".to_string()],
            match_keywords: vec![],
            exclude_keywords: vec![],
            log_files: vec![],
            running_patterns: vec![],
            waiting_patterns: vec![],
            done_patterns: vec![],
        }
    }

    fn claude_hit(active_child: Option<&str>) -> ProcessHit {
        ProcessHit {
            name: "claude.exe".to_string(),
            pid: "42".to_string(),
            title: "C:\\Users\\liyang\\AppData\\Roaming\\npm\\claude.exe".to_string(),
            active_child: active_child.map(str::to_string),
        }
    }

    #[test]
    fn pending_tool_without_child_waits_for_approval() {
        let tail = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"call_1","name":"Bash","input":{"command":"npm install"}}],"stop_reason":"tool_use"}}"#;
        let probe = parse_claude_session_tail(tail, true);
        assert_eq!(probe.pending_tool.as_deref(), Some("Bash"));

        let (state, detail) =
            decide_state(&claude_agent(), &[claude_hit(None)], &probe, "", None, 30);

        assert_eq!(state, AgentState::Waiting);
        assert!(detail.contains("approval"));
    }

    #[test]
    fn pending_tool_with_child_is_running() {
        let tail = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"call_1","name":"Bash","input":{"command":"npm install"}}],"stop_reason":"tool_use"}}"#;
        let probe = parse_claude_session_tail(tail, true);
        let (state, detail) = decide_state(
            &claude_agent(),
            &[claude_hit(Some("bash.exe#99"))],
            &probe,
            "",
            None,
            30,
        );

        assert_eq!(state, AgentState::Running);
        assert!(detail.contains("executing"));
    }

    #[test]
    fn tool_result_keeps_session_running_until_end_turn() {
        let running_tail = r#"
{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"call_1","name":"Bash","input":{"command":"npm install"}}],"stop_reason":"tool_use"}}
{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"call_1","content":"installed"}]}}
"#;
        let probe = parse_claude_session_tail(running_tail, true);
        let (state, _) = decide_state(&claude_agent(), &[claude_hit(None)], &probe, "", None, 30);
        assert_eq!(state, AgentState::Running);

        let idle_tail = format!(
            "{}\n{}",
            running_tail,
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Done"}],"stop_reason":"end_turn"}}"#
        );
        let probe = parse_claude_session_tail(&idle_tail, true);
        let (state, _) = decide_state(&claude_agent(), &[claude_hit(None)], &probe, "", None, 30);
        assert_eq!(state, AgentState::Idle);
    }
}
