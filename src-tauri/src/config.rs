use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

const APP_DIR_NAME: &str = "WindowsAIStatusMonitor";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub active_agent: String,
    pub agents: Vec<AgentConfig>,
    pub poll_interval_ms: u64,
    pub idle_timeout_secs: u64,
    pub autostart: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub id: String,
    pub label: String,
    pub process_names: Vec<String>,
    pub match_keywords: Vec<String>,
    #[serde(default)]
    pub exclude_keywords: Vec<String>,
    pub log_files: Vec<String>,
    pub running_patterns: Vec<String>,
    pub waiting_patterns: Vec<String>,
    pub done_patterns: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AppPaths {
    pub data_dir: String,
    pub config_file: String,
    pub log_file: String,
}

impl AppConfig {
    pub fn active_agent(&self) -> AgentConfig {
        self.agents
            .iter()
            .find(|agent| agent.id.eq_ignore_ascii_case(&self.active_agent))
            .cloned()
            .or_else(|| self.agents.first().cloned())
            .unwrap_or_else(default_agent)
    }
}

pub fn app_paths() -> Result<AppPaths> {
    let data_dir = data_dir()?;
    Ok(AppPaths {
        config_file: data_dir.join("config.json").display().to_string(),
        log_file: data_dir.join("status.log").display().to_string(),
        data_dir: data_dir.display().to_string(),
    })
}

pub fn data_dir() -> Result<PathBuf> {
    let base = dirs::data_dir().context("could not resolve %APPDATA% data directory")?;
    Ok(base.join(APP_DIR_NAME))
}

pub fn config_path() -> Result<PathBuf> {
    Ok(data_dir()?.join("config.json"))
}

pub fn log_path() -> Result<PathBuf> {
    Ok(data_dir()?.join("status.log"))
}

pub fn ensure_data_files() -> Result<AppConfig> {
    let data_dir = data_dir()?;
    fs::create_dir_all(&data_dir).with_context(|| format!("create {}", data_dir.display()))?;

    let path = config_path()?;
    if !path.exists() {
        save_config(&default_config())?;
    }

    load_config()
}

pub fn load_config() -> Result<AppConfig> {
    let path = config_path()?;
    let raw = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let mut config: AppConfig =
        serde_json::from_str(&raw).with_context(|| format!("parse {}", path.display()))?;

    let changed = normalize_config(&mut config);
    if changed {
        save_config(&config)?;
    }

    Ok(config)
}

pub fn save_config(config: &AppConfig) -> Result<()> {
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(config)?;
    fs::write(&path, json).with_context(|| format!("write {}", path.display()))
}

pub fn set_active_agent(agent_id: &str) -> Result<AppConfig> {
    let mut config = load_config()?;
    let exists = config
        .agents
        .iter()
        .any(|agent| agent.id.eq_ignore_ascii_case(agent_id));

    if !exists {
        bail!("unknown agent id: {agent_id}");
    }

    config.active_agent = agent_id.to_string();
    save_config(&config)?;
    Ok(config)
}

fn normalize_config(config: &mut AppConfig) -> bool {
    let mut changed = false;

    if config.poll_interval_ms < 500 {
        config.poll_interval_ms = 500;
        changed = true;
    }
    if config.idle_timeout_secs < 5 {
        config.idle_timeout_secs = 5;
        changed = true;
    }
    if config.agents.is_empty() {
        config.agents.push(default_agent());
        changed = true;
    }

    let defaults = default_config();
    for default_agent in defaults.agents {
        if let Some(agent) = config
            .agents
            .iter_mut()
            .find(|agent| agent.id.eq_ignore_ascii_case(&default_agent.id))
        {
            changed |= merge_unique(&mut agent.process_names, &default_agent.process_names);
            changed |= merge_unique(&mut agent.match_keywords, &default_agent.match_keywords);
            changed |= merge_unique(&mut agent.exclude_keywords, &default_agent.exclude_keywords);
            changed |= merge_unique(&mut agent.running_patterns, &default_agent.running_patterns);
            changed |= merge_unique(&mut agent.waiting_patterns, &default_agent.waiting_patterns);
            changed |= merge_unique(&mut agent.done_patterns, &default_agent.done_patterns);
            if agent.label.trim().is_empty() {
                agent.label = default_agent.label;
                changed = true;
            }
        } else {
            config.agents.push(default_agent);
            changed = true;
        }
    }

    if !config
        .agents
        .iter()
        .any(|agent| agent.id.eq_ignore_ascii_case(&config.active_agent))
    {
        config.active_agent = "claude".to_string();
        changed = true;
    }

    changed
}

fn merge_unique(target: &mut Vec<String>, defaults: &[String]) -> bool {
    let mut changed = false;
    for value in defaults {
        if !target
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(value))
        {
            target.push(value.clone());
            changed = true;
        }
    }
    changed
}

fn default_config() -> AppConfig {
    AppConfig {
        active_agent: "claude".to_string(),
        poll_interval_ms: 1200,
        idle_timeout_secs: 30,
        autostart: true,
        agents: vec![
            AgentConfig {
                id: "claude".to_string(),
                label: "CLAUDE".to_string(),
                process_names: vec!["claude.exe".to_string(), "claude.cmd".to_string()],
                match_keywords: vec!["claude".to_string(), "anthropic".to_string()],
                exclude_keywords: vec![],
                log_files: vec![],
                running_patterns: vec![
                    "thinking".to_string(),
                    "executing".to_string(),
                    "running".to_string(),
                    "tool call".to_string(),
                    "processing".to_string(),
                    "generating".to_string(),
                ],
                waiting_patterns: vec![
                    "allow".to_string(),
                    "approve".to_string(),
                    "confirm".to_string(),
                    "continue".to_string(),
                    "press enter".to_string(),
                    "permission".to_string(),
                ],
                done_patterns: vec![
                    "task completed".to_string(),
                    "done".to_string(),
                    "finished".to_string(),
                    "success".to_string(),
                ],
            },
            AgentConfig {
                id: "codex".to_string(),
                label: "CODEX CLI".to_string(),
                process_names: vec!["codex.exe".to_string(), "codex.cmd".to_string()],
                match_keywords: vec![
                    "codex".to_string(),
                    "openai".to_string(),
                    "roaming\\npm".to_string(),
                    "node_modules".to_string(),
                    ".codex".to_string(),
                ],
                exclude_keywords: vec![
                    "windowsapps\\openai.codex_".to_string(),
                    "appdata\\local\\openai\\codex".to_string(),
                    "\\app\\codex.exe".to_string(),
                    "\\app\\resources\\codex.exe".to_string(),
                ],
                log_files: vec![],
                running_patterns: vec![
                    "thinking".to_string(),
                    "running".to_string(),
                    "tool".to_string(),
                    "exec".to_string(),
                ],
                waiting_patterns: vec![
                    "allow".to_string(),
                    "approve".to_string(),
                    "confirm".to_string(),
                    "continue".to_string(),
                    "permission".to_string(),
                ],
                done_patterns: vec![
                    "done".to_string(),
                    "complete".to_string(),
                    "success".to_string(),
                ],
            },
            AgentConfig {
                id: "codex_desktop".to_string(),
                label: "CODEX DESK".to_string(),
                process_names: vec![
                    "Codex.exe".to_string(),
                    "codex.exe".to_string(),
                    "node_repl.exe".to_string(),
                    "codex-command-runner*.exe".to_string(),
                ],
                match_keywords: vec![
                    "openai.codex".to_string(),
                    "appdata\\local\\openai\\codex".to_string(),
                    "codex-command-runner".to_string(),
                ],
                exclude_keywords: vec!["windows ai status monitor".to_string()],
                log_files: vec![],
                running_patterns: vec![
                    "thinking".to_string(),
                    "running".to_string(),
                    "tool".to_string(),
                    "exec".to_string(),
                    "app-server".to_string(),
                ],
                waiting_patterns: vec![
                    "allow".to_string(),
                    "approve".to_string(),
                    "confirm".to_string(),
                    "continue".to_string(),
                    "permission".to_string(),
                ],
                done_patterns: vec![
                    "done".to_string(),
                    "complete".to_string(),
                    "success".to_string(),
                ],
            },
            AgentConfig {
                id: "gemini".to_string(),
                label: "GEMINI".to_string(),
                process_names: vec!["gemini.exe".to_string(), "node.exe".to_string()],
                match_keywords: vec!["gemini".to_string(), "google".to_string()],
                exclude_keywords: vec![],
                log_files: vec![],
                running_patterns: vec![
                    "thinking".to_string(),
                    "running".to_string(),
                    "executing".to_string(),
                ],
                waiting_patterns: vec![
                    "allow".to_string(),
                    "approve".to_string(),
                    "confirm".to_string(),
                    "continue".to_string(),
                ],
                done_patterns: vec![
                    "done".to_string(),
                    "complete".to_string(),
                    "success".to_string(),
                ],
            },
        ],
    }
}

fn default_agent() -> AgentConfig {
    default_config().agents.remove(0)
}
