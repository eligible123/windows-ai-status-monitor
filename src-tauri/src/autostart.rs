use anyhow::{Context, Result};
use std::env;
use winreg::{enums::HKEY_CURRENT_USER, RegKey};

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const APP_VALUE: &str = "WindowsAIStatusMonitor";

pub fn apply(enabled: bool) -> Result<()> {
    if enabled {
        enable()
    } else {
        disable()
    }
}

pub fn enable() -> Result<()> {
    let exe = env::current_exe().context("resolve current executable")?;
    let command = format!("\"{}\"", exe.display());
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu.create_subkey(RUN_KEY).context("open HKCU Run key")?;
    key.set_value(APP_VALUE, &command)
        .context("write autostart registry value")
}

pub fn disable() -> Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let key = hkcu
        .open_subkey_with_flags(RUN_KEY, winreg::enums::KEY_SET_VALUE)
        .context("open HKCU Run key")?;
    match key.delete_value(APP_VALUE) {
        Ok(_) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err).context("delete autostart registry value"),
    }
}
