export type AgentState = "RUNNING" | "WAITING" | "IDLE" | "OFFLINE";

export interface StatusPayload {
  agent: string;
  state: AgentState;
  detail: string;
  started_at?: string | null;
  updated_at: string;
  duration_secs: number;
  pid_count: number;
}

export interface AppPaths {
  data_dir: string;
  config_file: string;
  log_file: string;
}
