# Windows AI Status Monitor

Windows AI Status Monitor is a tiny always-on-top Windows desktop panel that shows the current state of AI coding agents with a traffic-light display.

It was built for Claude Code, Codex CLI, Codex Desktop and Gemini CLI. The app stays in the top-right corner, hides from the taskbar, runs from the system tray, and helps you see whether an agent is running, waiting for approval, or idle.

## Features

- 220 x 80 px floating status panel.
- Dark frosted-glass hardware-panel style.
- Draggable, fixed-size, always on top, and hidden from the taskbar.
- System tray menu for show, hide, restart monitor, switch agent, open logs, open config, and exit.
- Startup registration through the current user's Windows registry.
- Claude Code session log detection from `%USERPROFILE%\.claude\projects\*.jsonl`.
- Process detection for Claude Code, Codex CLI, Codex Desktop and Gemini CLI.
- Configurable process names, match keywords, exclude keywords and log keyword rules.
- Status log written to `%APPDATA%\WindowsAIStatusMonitor\status.log`.
- NSIS `.exe` installer build target.

## Status Lights

| Light | State | Meaning |
| --- | --- | --- |
| Green | `RUNNING` | The agent is thinking, using tools, running a command, or processing tool output. |
| Yellow | `WAITING` | The agent is waiting for user confirmation, permission, or command approval. |
| Red | `IDLE` | The agent process exists, but the current task has ended or is idle. |
| Red | `OFFLINE` | No matching agent process was found. |

For Claude Code, the monitor reads recent `.jsonl` session events. A pending `tool_use` without an active child process is treated as `WAITING`, which covers prompts such as `Do you want to proceed?`. When the command starts, active child processes such as `bash.exe`, `cmd.exe`, `npm.exe`, `git.exe` or `node.exe` keep the state green until the task reaches an end turn.

## Included Agents

The default config includes these monitor targets:

- `claude` - Claude Code.
- `codex` - Codex CLI.
- `codex_desktop` - Codex Desktop app and its local command runner processes.
- `gemini` - Gemini CLI.

Use the tray menu to switch the active monitor target.

## Download

The generated Windows installer is included in this repository under:

```text
release/Windows AI Status Monitor_0.1.4_x64-setup.exe
```

After installation, the app starts in the background and shows the floating panel near the top-right corner of the desktop.

## Requirements For Development

- Windows 10 or Windows 11.
- Node.js 20 or newer.
- Rust stable for `x86_64-pc-windows-msvc`.
- Microsoft Visual Studio Build Tools 2022.
- Visual Studio workload: `Desktop development with C++`.
- Windows 10/11 SDK.

If Rust reports `link.exe not found`, install Visual Studio Build Tools and select the C++ workload. If it reports missing Windows libraries such as `kernel32.lib`, install the Windows SDK component from the same installer.

## Run In Development

```powershell
cd D:\WindowsAIStatusMonitor
npm.cmd install
npm.cmd run tauri:dev
```

PowerShell may block `npm.ps1` depending on execution policy, so `npm.cmd` is used in the examples.

## Build The Installer

```powershell
cd D:\WindowsAIStatusMonitor
npm.cmd install
npm.cmd run package:exe
```

The NSIS installer is generated at:

```text
src-tauri\target\release\bundle\nsis\Windows AI Status Monitor_0.1.4_x64-setup.exe
```

## Project Structure

```text
src/                  React UI
src-tauri/src/        Rust backend, tray, autostart, logging and monitor logic
src-tauri/icons/      App icons
scripts/              Helper scripts
release/              Packaged installer copied for distribution
```

Important files:

- `src/App.tsx` - floating panel UI.
- `src-tauri/src/monitor.rs` - process and session-state detection.
- `src-tauri/src/config.rs` - runtime config defaults and config file handling.
- `src-tauri/src/tray.rs` - system tray menu.
- `src-tauri/src/autostart.rs` - Windows startup registration.
- `src-tauri/src/logbook.rs` - status log writer.

## Runtime Files

On first launch, the app creates:

```text
%APPDATA%\WindowsAIStatusMonitor\config.json
%APPDATA%\WindowsAIStatusMonitor\status.log
```

Example config shape:

```json
{
  "active_agent": "claude",
  "poll_interval_ms": 1200,
  "idle_timeout_secs": 30,
  "autostart": true,
  "agents": [
    {
      "id": "claude",
      "label": "CLAUDE",
      "process_names": ["claude.exe", "claude.cmd"],
      "match_keywords": ["claude", "anthropic"],
      "exclude_keywords": [],
      "log_files": [],
      "running_patterns": ["thinking", "executing", "running", "tool call", "processing", "generating"],
      "waiting_patterns": ["allow", "approve", "confirm", "continue", "press enter", "permission"],
      "done_patterns": ["task completed", "done", "finished", "success"]
    }
  ]
}
```

The app normalizes old config files on startup and merges in new default agents when the app is updated.

## Notes

- The app does not inject into terminals or AI tools.
- Claude Code status is inferred from process state plus Claude's local JSONL session files.
- Generic terminal output from another process is not always available on Windows, so structured session files are more reliable than screen scraping.
- Codex Desktop detection currently focuses on the desktop app process, local app server, `node_repl.exe`, and `codex-command-runner*.exe` processes.

## License

MIT
