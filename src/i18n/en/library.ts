import type { Messages } from "..";

const plural = (n: string, one: string, many: string) => `${n} ${n === "1" ? one : many}`;

export const library: Messages["library"] = {
  title: "Library",
  stats: (groups, files, size) => `${plural(groups, "item", "items")} · ${plural(files, "file", "files")} · ${size}`,
  rescan: "Rescan",
  importPhotos: "Import from Photos",
  addFolders: "Add Folders",
  addFoldersTitle: "Add Source Folders",
  removeRoot: {
    message: (path) =>
      `“${path}” will be removed from the library. The files in the folder stay untouched; only the index, ratings and event assignments for this folder are lost.`,
    title: "Remove Folder",
    ok: "Remove",
  },
  root: {
    unreachable: "unavailable",
    lockedTitle: "Locked: Sichter plans no file operations here. Click to unlock.",
    lockTitle: "Lock: no file operations in this folder",
    locked: "locked",
    lock: "lock",
    files: (n, size) => `${plural(n, "file", "files")} · ${size}`,
  },
  scan: {
    percent: (p) => `${p}%`,
    step: (step, of, label) => `Step ${step} of ${of}: ${label}`,
  },
  report: {
    cancelled: "Scan cancelled",
    finished: "Scan complete",
    new: (n) => `${n} new`,
    changed: (n) => `${n} changed`,
    removed: (n) => `${n} no longer present`,
    seconds: (s) => `${s} s`,
    resume: "the next scan will resume",
    unreachable: (root) => `Unavailable, index left unchanged: ${root}`,
    skippedPackage: (p) => `Skipped (another app’s library): ${p}`,
    errors: (n) => (n === 1 ? "1 file unreadable" : `${n} files unreadable`),
  },
  empty: {
    title: "No folders added yet",
    text: "Sichter only reads your folders. Changes happen exclusively through a plan that you review and confirm first.",
    photosQuestion: "Your pictures are in Apple Photos?",
    photosLink: "Import originals from the Photos library",
  },
  systemStatus: "System Status",
  filters: {
    all: "All",
    photo: "Photos",
    raw: "RAW",
    video: "Videos",
    uncertain: "Date uncertain",
  },
  unhandled: {
    title: "Unknown file types and sidecars without a matching image. They are never moved.",
    count: (n) => `${plural(n, "unsupported file", "unsupported files")}`,
    sidecar: "sidecar without image",
    unknown: "unknown type",
  },
  noItems: "Nothing matches this filter.",
  tile: {
    uncertain: "uncertain",
    uncertainTitle: "No capture date in the metadata",
  },
  photos: {
    title: "Import from the Photos Library",
    introBefore: "Sichter copies the originals (including RAW and Live Photo videos) into a folder, sorted by",
    introPattern: "year/date",
    introAfter:
      ". The Photos library stays untouched, and edits made in Photos stay in Photos. The folder is then added to the library and scanned.",
    blocked:
      "Sichter has no access to the Photos library. Allow it in System Settings → Privacy & Security → Photos, then restart Sichter.",
    openSettings: "Open System Settings",
    limited: "You have only shared selected photos. Only those will be imported.",
    pickTarget: "Choose Destination …",
    pickTargetTitle: "Destination Folder for the Import",
    noTarget: "none chosen yet",
    downloadOriginals: "Download originals from iCloud",
    downloadOriginalsHint: "With “Optimize Mac Storage”, many originals are only in iCloud. Without this option they are skipped.",
    includeHidden: "Include hidden photos",
    waiting: "Waiting for the Photos library …",
    reading: "Reading library",
    objects: (done, total) => `${done} / ${total} items`,
    percent: (p) => `${p}%`,
    planning: (total) => `${plural(total, "item", "items")} read, creating plan …`,
    createPlan: "Create Plan",
  },
};
