use crate::{config, monitor::StatusPayload};
use anyhow::{Context, Result};
use std::{fs::OpenOptions, io::Write};

pub fn append_status(status: &StatusPayload) -> Result<()> {
    let path = config::log_path()?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("open {}", path.display()))?;

    let line = format!(
        "{} {} {} duration={}s pids={} detail={}\n",
        status.updated_at,
        status.agent,
        status.state,
        status.duration_secs,
        status.pid_count,
        status.detail.replace('\n', " ")
    );

    file.write_all(line.as_bytes())
        .with_context(|| format!("write {}", path.display()))
}
