import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/tauri";
import { listen } from "@tauri-apps/api/event";
import type { AppPaths, AgentState, StatusPayload } from "./types";

const fallbackStatus: StatusPayload = {
  agent: "CLAUDE",
  state: "IDLE",
  detail: "Initializing monitor",
  started_at: null,
  updated_at: new Date().toISOString(),
  duration_secs: 0,
  pid_count: 0
};

function formatDuration(totalSeconds: number): string {
  const seconds = Math.max(0, Math.floor(totalSeconds));
  const mins = Math.floor(seconds / 60);
  const secs = seconds % 60;
  if (mins >= 60) {
    const hours = Math.floor(mins / 60);
    const remainMins = mins % 60;
    return `${hours}h ${remainMins}m`;
  }
  return `${mins}:${secs.toString().padStart(2, "0")}`;
}

function lightClass(color: "red" | "yellow" | "green", state: AgentState): string {
  const active =
    (color === "green" && state === "RUNNING") ||
    (color === "yellow" && state === "WAITING") ||
    (color === "red" && (state === "IDLE" || state === "OFFLINE"));

  return ["light", color, active ? "active" : "dim"].join(" ");
}

export default function App() {
  const [status, setStatus] = useState<StatusPayload>(fallbackStatus);
  const [paths, setPaths] = useState<AppPaths | null>(null);

  useEffect(() => {
    invoke<StatusPayload>("get_status")
      .then(setStatus)
      .catch(() => setStatus(fallbackStatus));

    invoke<AppPaths>("get_paths")
      .then(setPaths)
      .catch(() => setPaths(null));

    const unlistenPromise = listen<StatusPayload>("status-change", (event) => {
      setStatus(event.payload);
    });

    return () => {
      unlistenPromise.then((unlisten) => unlisten()).catch(() => undefined);
    };
  }, []);

  const statusTone = useMemo(() => status.state.toLowerCase(), [status.state]);

  async function dragWindow() {
    await invoke("start_drag");
  }

  async function refreshStatus(event: React.MouseEvent<HTMLButtonElement>) {
    event.stopPropagation();
    const next = await invoke<StatusPayload>("get_status");
    setStatus(next);
  }

  async function openLogs(event: React.MouseEvent<HTMLButtonElement>) {
    event.stopPropagation();
    await invoke("open_log_file");
  }

  return (
    <main className={`panel ${statusTone}`} onMouseDown={dragWindow}>
      <section className="panelShell">
        <div className="topRow">
          <button className="agent" title={paths?.config_file ?? "Open config from tray"} onClick={refreshStatus}>
            {status.agent}
          </button>
          <button className="state" title={status.detail} onClick={openLogs}>
            {status.state}
          </button>
        </div>

        <div className="lights" aria-label={`AI agent status is ${status.state}`}>
          <span className={lightClass("red", status.state)} />
          <span className={lightClass("yellow", status.state)} />
          <span className={lightClass("green", status.state)} />
        </div>

        <div className="bottomRow">
          <span>{status.pid_count > 0 ? `${status.pid_count} PROC` : "NO PROC"}</span>
          <span>{formatDuration(status.duration_secs)}</span>
        </div>
      </section>
    </main>
  );
}
