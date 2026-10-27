import { invoke } from "@tauri-apps/api/core";

export type ToolSource = "env" | "bundled" | "path";

export interface ToolInfo {
  name: string;
  version: string | null;
  path: string | null;
  source: ToolSource | null;
  error: string | null;
  gpl: boolean | null;
}

export interface AppInfo {
  app_version: string;
  data_dir: string;
  db_schema_version: number;
  tools: ToolInfo[];
}

export const api = {
  appInfo: () => invoke<AppInfo>("app_info"),
};
