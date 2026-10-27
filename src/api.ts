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
  reject_count: number;
  reject_size: number;
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

export interface EventSettings {
  gap_hours: number;
  gps_km: number;
  merge_multiday: boolean;
}

export type TzMode = "utc" | "local";

export interface CameraTz {
  camera: string;
  video_count: number;
  mode: TzMode;
  is_default: boolean;
}

export interface EventCard {
  id: number;
  title: string;
  suggested_title: string;
  date_start: string | null;
  date_end: string | null;
  confirmed: boolean;
  notes: string;
  count: number;
  video_count: number;
  uncertain_count: number;
  rated_count: number;
  keep_count: number;
  reject_count: number;
  thumbs: string[];
  tz_hint_cameras: string[];
}

export interface EventsOverview {
  events: EventCard[];
  unassigned: number;
  settings: EventSettings;
  cameras: CameraTz[];
}

/** Aufnahme in einem Event; `time` ist bereits zeitzonenkorrigiert. */
export interface EventItem {
  group_id: number;
  event_id: number | null;
  root_id: number;
  rel_path: string;
  kind: Kind;
  kinds: Kind[];
  thumb: string | null;
  time: string | null;
  date_source: DateSource | null;
  duration_s: number | null;
  stars: number;
  flag: Flag;
  preview_hash: string | null;
  video_asset_id: number | null;
}

export type Flag = "none" | "keep" | "reject";

export type PlanMode = "move" | "copy";

export interface ReorganizeParams {
  source_root_ids: number[];
  target_root_id: number;
  template: string;
  video_template: string | null;
  mode: PlanMode;
  date_range: boolean;
  only_confirmed: boolean;
}

export type ConflictKind = "duplicate_at_target" | "renamed" | "not_hashed" | "group_partial";

export interface PlanConflict {
  kind: ConflictKind;
  path: string;
  target: string | null;
}

export interface PlanSummary {
  files: number;
  bytes: number;
  mkdirs: number;
  groups: number;
  in_place: number;
  skipped_unconfirmed: number;
  conflicts: PlanConflict[];
  bytes_to_copy: number;
  free_bytes: number | null;
}

export type RunStatus = "planned" | "running" | "done" | "aborted" | "undone";

export interface RunOp {
  seq: number;
  op: "mkdir" | "move" | "copy" | "trash";
  src: string | null;
  dst: string | null;
  size: number | null;
  status: "planned" | "done" | "failed" | "undone";
  error: string | null;
  keep_path: string | null;
}

export interface RunDetails {
  id: number;
  kind: string;
  title: string;
  status: RunStatus;
  dry_run: boolean;
  created_at: string;
  finished_at: string | null;
  log_path: string | null;
  summary: PlanSummary;
  files: number;
  mkdirs: number;
  counts: Record<string, number>;
  before: Record<string, number>;
  after: Record<string, number>;
  ops: RunOp[];
  total_ops: number;
}

export interface RunListItem {
  id: number;
  kind: string;
  title: string;
  status: RunStatus;
  dry_run: boolean;
  created_at: string;
  finished_at: string | null;
  files: number;
  done: number;
  failed: number;
  undone: number;
  log_path: string | null;
}

export interface RunProgress {
  run_id: number;
  done: number;
  total: number;
  current: string | null;
  paused: boolean;
}

export interface RunReport {
  run_id: number;
  dry_run: boolean;
  done: number;
  failed: number;
  undone: number;
  skipped: number;
  cancelled: boolean;
  messages: [string, string][];
  log_path: string | null;
  manual_restore: string[];
}

export type RunAction = "dry" | "execute" | "undo";
export type RunFinished =
  | { status: "ok"; run_id: number; action: RunAction; report: RunReport }
  | { status: "error"; run_id: number; action: RunAction; message: string };

export interface RunState {
  running: number | null;
  paused: boolean;
  open_plans: number;
  last_run: string | null;
}

export interface RejectGroup {
  group_id: number;
  thumb: string | null;
  files: [string, number][];
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
  eventsOverview: () => invoke<EventsOverview>("events_overview"),
  eventItems: (eventId: number) => invoke<EventItem[]>("event_items", { eventId }),
  eventsSaveSettings: (settings: EventSettings) => invoke<void>("events_save_settings", { settings }),
  eventsRecluster: () => invoke<number>("events_recluster"),
  eventRename: (eventId: number, title: string) => invoke<void>("event_rename", { eventId, title }),
  eventSetNotes: (eventId: number, notes: string) => invoke<void>("event_set_notes", { eventId, notes }),
  eventConfirm: (eventId: number, confirmed: boolean) => invoke<void>("event_confirm", { eventId, confirmed }),
  eventsMerge: (eventIds: number[]) => invoke<number | null>("events_merge", { eventIds }),
  eventSplit: (eventId: number, groupId: number) => invoke<number | null>("event_split", { eventId, groupId }),
  eventsMove: (groupIds: number[], target: number | null) => invoke<number>("events_move", { groupIds, target }),
  cameraTzSet: (camera: string, mode: TzMode | null) => invoke<void>("camera_tz_set", { camera, mode }),
  cullItems: (eventId: number | null) => invoke<EventItem[]>("cull_items", { eventId }),
  setRating: (groupId: number, stars: number | null, flag: Flag | null) => invoke<void>("set_rating", { groupId, stars, flag }),
  resetRating: (groupId: number) => invoke<void>("reset_rating", { groupId }),
  previewsPrepare: (hashes: string[]) => invoke<void>("previews_prepare", { hashes }),
  openInDefaultApp: (assetId: number) => invoke<void>("open_in_default_app", { assetId }),
  runsDryRunMode: () => invoke<boolean>("runs_dry_run_mode"),
  runsSetDryRunMode: (enabled: boolean) => invoke<void>("runs_set_dry_run_mode", { enabled }),
  planTemplatePreview: (params: ReorganizeParams) => invoke<{ error: string | null; samples: [string, string][] }>("plan_template_preview", { params }),
  planReorganize: (params: ReorganizeParams) => invoke<number>("plan_reorganize", { params }),
  planRejects: (toTrash: boolean, approvedGroups: number[]) => invoke<number>("plan_rejects", { toTrash, approvedGroups }),
  planDupes: (hashes: string[], toTrash: boolean) => invoke<number>("plan_dupes", { hashes, toTrash }),
  runDiscard: (runId: number) => invoke<void>("run_discard", { runId }),
  runDetails: (runId: number) => invoke<RunDetails>("run_details", { runId }),
  runsList: () => invoke<RunListItem[]>("runs_list"),
  runDry: (runId: number) => invoke<void>("run_dry", { runId }),
  runExecute: (runId: number, confirmedFiles: number) => invoke<void>("run_execute", { runId, confirmedFiles }),
  runUndo: (runId: number) => invoke<void>("run_undo", { runId }),
  runPause: (paused: boolean) => invoke<void>("run_pause", { paused }),
  runCancel: () => invoke<void>("run_cancel"),
  runState: () => invoke<RunState>("run_state"),
  runOpenLog: (runId: number) => invoke<void>("run_open_log", { runId }),
  rejectsList: () => invoke<RejectGroup[]>("rejects_list"),
  dupesAnalyze: () => invoke<DupeAnalysis>("dupes_analyze"),
  setDupeChoice: (hash: string, keepAssetId: number | null) => invoke<void>("set_dupe_choice", { hash, keepAssetId }),
  largestFiles: (limit: number) => invoke<LargeFile[]>("largest_files", { limit }),
};

export const thumbUrl = (hash: string) => convertFileSrc(hash, "thumb");
export const previewUrl = (hash: string) => convertFileSrc(hash, "preview");
export const mediaUrl = (assetId: number) => convertFileSrc(String(assetId), "media");
