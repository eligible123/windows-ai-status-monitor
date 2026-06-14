#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autostart;
mod config;
mod logbook;
mod monitor;
mod tray;

use anyhow::{Context, Result};
use monitor::{MonitorRuntime, StatusPayload};
use std::{
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};
use tauri::{api::shell, AppHandle, Manager, PhysicalPosition, State, SystemTrayEvent};

type SharedMonitor = Arc<Mutex<MonitorRuntime>>;
const WINDOW_WIDTH: i32 = 250;

#[tauri::command]
fn get_status(monitor: State<SharedMonitor>) -> Result<StatusPayload, String> {
    monitor
        .lock()
        .map_err(|_| "monitor lock poisoned".to_string())
        .map(|runtime| runtime.status.clone())
}

#[tauri::command]
fn get_paths() -> Result<config::AppPaths, String> {
    config::app_paths().map_err(|err| err.to_string())
}

#[tauri::command]
fn start_drag(window: tauri::Window) -> Result<(), String> {
    window.start_dragging().map_err(|err| err.to_string())
}

#[tauri::command]
fn set_active_agent(
    agent_id: String,
    monitor: State<SharedMonitor>,
    app: AppHandle,
) -> Result<StatusPayload, String> {
    switch_active_agent(&app, &monitor, &agent_id)
}

#[tauri::command]
fn open_log_file(app: AppHandle) -> Result<(), String> {
    open_path(&app, config::log_path().map_err(|err| err.to_string())?)
}

fn main() {
    if let Err(err) = run() {
        eprintln!("Windows AI Status Monitor failed: {err:?}");
    }
}

fn run() -> Result<()> {
    let config = config::ensure_data_files()?;
    autostart::apply(config.autostart).ok();

    let monitor = Arc::new(Mutex::new(MonitorRuntime::new(config)));
    let tray = tray::build_tray();

    tauri::Builder::default()
        .manage(monitor.clone())
        .system_tray(tray)
        .on_system_tray_event(move |app, event| handle_tray_event(app, event, monitor.clone()))
        .invoke_handler(tauri::generate_handler![
            get_status,
            get_paths,
            start_drag,
            set_active_agent,
            open_log_file
        ])
        .setup(|app| {
            let handle = app.handle();
            position_main_window(&handle);
            start_monitor_loop(handle, app.state::<SharedMonitor>().inner().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .context("run tauri application")
}

fn handle_tray_event(app: &AppHandle, event: SystemTrayEvent, monitor: SharedMonitor) {
    let SystemTrayEvent::MenuItemClick { id, .. } = event else {
        return;
    };

    match id.as_str() {
        tray::MENU_SHOW => {
            if let Some(window) = app.get_window("main") {
                window.show().ok();
                window.set_focus().ok();
            }
        }
        tray::MENU_HIDE => {
            if let Some(window) = app.get_window("main") {
                window.hide().ok();
            }
        }
        tray::MENU_RESTART => reload_monitor(app, &monitor),
        tray::MENU_AGENT_CLAUDE => {
            switch_active_agent(app, &monitor, "claude").ok();
        }
        tray::MENU_AGENT_CODEX => {
            switch_active_agent(app, &monitor, "codex").ok();
        }
        tray::MENU_AGENT_CODEX_DESKTOP => {
            switch_active_agent(app, &monitor, "codex_desktop").ok();
        }
        tray::MENU_LOGS => {
            if let Ok(path) = config::log_path() {
                open_path(app, path).ok();
            }
        }
        tray::MENU_CONFIG => {
            if let Ok(path) = config::config_path() {
                open_path(app, path).ok();
            }
        }
        tray::MENU_QUIT => app.exit(0),
        _ => {}
    }
}

fn reload_monitor(app: &AppHandle, monitor: &SharedMonitor) {
    if let Ok(config) = config::load_config() {
        autostart::apply(config.autostart).ok();
        if let Ok(mut runtime) = monitor.lock() {
            runtime.reload_config(config);
            emit_current_status(app, &mut runtime);
        }
    }
}

fn switch_active_agent(
    app: &AppHandle,
    monitor: &SharedMonitor,
    agent_id: &str,
) -> Result<StatusPayload, String> {
    let config = config::set_active_agent(agent_id).map_err(|err| err.to_string())?;
    autostart::apply(config.autostart).ok();
    let mut runtime = monitor
        .lock()
        .map_err(|_| "monitor lock poisoned".to_string())?;
    runtime.reload_config(config);
    Ok(emit_current_status(app, &mut runtime))
}

fn emit_current_status(app: &AppHandle, runtime: &mut MonitorRuntime) -> StatusPayload {
    match runtime.poll() {
        Ok((status, changed)) => {
            if changed {
                logbook::append_status(&status).ok();
            }
            app.emit_all("status-change", status.clone()).ok();
            status
        }
        Err(err) => {
            runtime.status.detail = format!("Monitor error: {err}");
            let status = runtime.status.clone();
            app.emit_all("status-change", status.clone()).ok();
            status
        }
    }
}

fn start_monitor_loop(app: AppHandle, monitor: SharedMonitor) {
    thread::spawn(move || loop {
        let interval = monitor
            .lock()
            .map(|runtime| runtime.config.poll_interval_ms)
            .unwrap_or(1200);

        let poll_result = monitor
            .lock()
            .ok()
            .and_then(|mut runtime| runtime.poll().ok());

        if let Some((status, changed)) = poll_result {
            if changed {
                logbook::append_status(&status).ok();
            }
            app.emit_all("status-change", status).ok();
        }

        thread::sleep(Duration::from_millis(interval));
    });
}

fn position_main_window(app: &AppHandle) {
    let Some(window) = app.get_window("main") else {
        return;
    };

    if let Ok(Some(monitor)) = window.current_monitor() {
        let size = monitor.size();
        let origin = monitor.position();
        let x = origin.x + size.width as i32 - WINDOW_WIDTH - 18;
        let y = origin.y + 18;
        window.set_position(PhysicalPosition::new(x, y)).ok();
    }
}

fn open_path(app: &AppHandle, path: std::path::PathBuf) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }

    if !path.exists() {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|err| err.to_string())?;
    }

    shell::open(&app.shell_scope(), path.display().to_string(), None).map_err(|err| err.to_string())
}
