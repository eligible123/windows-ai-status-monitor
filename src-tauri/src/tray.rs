use tauri::{CustomMenuItem, Icon, SystemTray, SystemTrayMenu, SystemTrayMenuItem};

pub const MENU_SHOW: &str = "show";
pub const MENU_HIDE: &str = "hide";
pub const MENU_AGENT_CLAUDE: &str = "agent:claude";
pub const MENU_AGENT_CODEX: &str = "agent:codex";
pub const MENU_AGENT_CODEX_DESKTOP: &str = "agent:codex_desktop";
pub const MENU_RESTART: &str = "restart";
pub const MENU_LOGS: &str = "logs";
pub const MENU_CONFIG: &str = "config";
pub const MENU_QUIT: &str = "quit";

pub fn build_tray() -> SystemTray {
    let menu = SystemTrayMenu::new()
        .add_item(CustomMenuItem::new(MENU_SHOW, "Show Panel"))
        .add_item(CustomMenuItem::new(MENU_HIDE, "Hide Panel"))
        .add_native_item(SystemTrayMenuItem::Separator)
        .add_item(CustomMenuItem::new(MENU_AGENT_CLAUDE, "Monitor Claude"))
        .add_item(CustomMenuItem::new(MENU_AGENT_CODEX, "Monitor Codex CLI"))
        .add_item(CustomMenuItem::new(
            MENU_AGENT_CODEX_DESKTOP,
            "Monitor Codex Desktop",
        ))
        .add_native_item(SystemTrayMenuItem::Separator)
        .add_item(CustomMenuItem::new(MENU_RESTART, "Restart Monitor"))
        .add_item(CustomMenuItem::new(MENU_LOGS, "View Logs"))
        .add_item(CustomMenuItem::new(MENU_CONFIG, "Open Config"))
        .add_native_item(SystemTrayMenuItem::Separator)
        .add_item(CustomMenuItem::new(MENU_QUIT, "Exit"));

    SystemTray::new()
        .with_tooltip("Windows AI Status Monitor")
        .with_icon(traffic_light_icon())
        .with_menu(menu)
}

fn traffic_light_icon() -> Icon {
    let width = 16;
    let height = 16;
    let mut rgba = vec![0; width * height * 4];

    draw_circle(&mut rgba, width, 4, 8, 3, [239, 68, 68, 255]);
    draw_circle(&mut rgba, width, 8, 8, 3, [245, 158, 11, 255]);
    draw_circle(&mut rgba, width, 12, 8, 3, [34, 197, 94, 255]);

    Icon::Rgba {
        rgba,
        width: width as u32,
        height: height as u32,
    }
}

fn draw_circle(
    buffer: &mut [u8],
    width: usize,
    center_x: i32,
    center_y: i32,
    radius: i32,
    color: [u8; 4],
) {
    let height = buffer.len() / width / 4;
    for y in 0..height as i32 {
        for x in 0..width as i32 {
            let dx = x - center_x;
            let dy = y - center_y;
            if dx * dx + dy * dy <= radius * radius {
                let idx = ((y as usize * width) + x as usize) * 4;
                buffer[idx..idx + 4].copy_from_slice(&color);
            }
        }
    }
}
