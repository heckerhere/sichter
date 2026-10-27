import { convertFileSrc, invoke } from "@tauri-apps/api/core";

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

export interface RootInfo {
  id: number;
  path: string;
  locked: boolean;
  reachable: boolean;
  file_count: number;
  total_size: number;
}

export interface AddRootResult {
  path: string;
  id: number | null;
  error: string | null;
}

export type Kind = "raw" | "jpeg" | "heic" | "png" | "tiff" | "webp" | "video" | "sidecar" | "other";
export type DateSource = "exif" | "filename" | "mtime";

export interface GridItem {
  group_id: number;
  asset_id: number;
  root_id: number;
  rel_path: string;
  kind: Kind;
  kinds: Kind[];
  thumb: string | null;
  taken_at: string | null;
  date_source: DateSource | null;
  duration_s: number | null;
  ext_mismatch: boolean;
}

export interface UnhandledFile {
  root_id: number;
  rel_path: string;
  size: number;
  kind: Kind;
}

export interface LibraryStats {
  files: number;
  groups: number;
  total_size: number;
  uncertain_dates: number;
}

export interface ScanProgress {
  phase: "crawl" | "detect" | "meta" | "group" | "hash" | "thumbs" | "done";
  done: number;
  total: number;
  current: string | null;
}

export interface ScanReport {
  files_total: number;
  new: number;
  changed: number;
  removed: number;
  unchanged: number;
  hashed: number;
  thumbs: number;
  thumbs_failed: number;
  skipped_packages: string[];
  unreachable_roots: string[];
  errors: [string, string][];
  cancelled: boolean;
  duration_ms: number;
}

export type ScanFinished = { status: "ok"; report: ScanReport } | { status: "error"; message: string };

export interface KindSize {
  kind: string;
  count: number;
  size: number;
}

export interface FolderRow {
  id: number;
  parent_id: number | null;
  rel_path: string;
  file_count: number;
  own_size: number;
  total_size: number;
  kinds: KindSize[];
}

export interface StorageSummary {
  total_size: number;
  file_count: number;
  kinds: KindSize[];
  raw_pair_jpeg_size: number;
  raw_pair_count: number;
  duplicate_size: number;
}

export interface LargeFile {
  root_id: number;
  rel_path: string;
  size: number;
  kind: Kind;
}

export interface FolderKey {
  root_id: number;
  rel_path: string;
}

export interface DupeFile {
  asset_id: number;
  root_id: number;
  rel_path: string;
  mtime: number;
  taken_at: string | null;
}

export interface FileGroup {
  hash: string;
  size: number;
  thumb: string | null;
  files: DupeFile[];
  suggestion_reason: string;
  keep_asset_id: number | null;
  redundant_size: number;
}

export interface FolderInfo {
  key: FolderKey;
  file_count: number;
  size: number;
}

export interface FolderPair {
  keep: FolderInfo;
  other: FolderInfo;
  suggestion_reason: string;
}

export interface SubsetPair {
  subset: FolderInfo;
  superset: FolderInfo;
  coverage: number;
}

export interface DupeAnalysis {
  file_groups: FileGroup[];
  identical_folders: FolderPair[];
  same_content_folders: FolderPair[];
  subset_folders: SubsetPair[];
  redundant_size: number;
}

export const api = {
  appInfo: () => invoke<AppInfo>("app_info"),
  listRoots: () => invoke<RootInfo[]>("list_roots"),
  addRoots: (paths: string[]) => invoke<AddRootResult[]>("add_roots", { paths }),
  removeRoot: (id: number) => invoke<void>("remove_root", { id }),
  listGroups: () => invoke<GridItem[]>("list_groups"),
  listUnhandled: () => invoke<UnhandledFile[]>("list_unhandled"),
  libraryStats: () => invoke<LibraryStats>("library_stats"),
  startScan: (rootIds?: number[]) => invoke<void>("start_scan", { rootIds: rootIds ?? null }),
  cancelScan: () => invoke<void>("cancel_scan"),
  scanRunning: () => invoke<boolean>("scan_running"),
  storageFolders: (rootId: number) => invoke<FolderRow[]>("storage_folders", { rootId }),
  storageSummary: () => invoke<StorageSummary>("storage_summary"),
  dupesAnalyze: () => invoke<DupeAnalysis>("dupes_analyze"),
  setDupeChoice: (hash: string, keepAssetId: number | null) => invoke<void>("set_dupe_choice", { hash, keepAssetId }),
  largestFiles: (limit: number) => invoke<LargeFile[]>("largest_files", { limit }),
};

export const thumbUrl = (hash: string) => convertFileSrc(hash, "thumb");
