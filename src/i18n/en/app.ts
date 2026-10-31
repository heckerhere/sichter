import type { Messages } from "..";

export const app: Messages["app"] = {
  nav: {
    library: "Library",
    storage: "Storage",
    events: "Events",
    cull: "Cull",
    dupes: "Duplicates",
    reorganize: "Reorganize",
    history: "History",
    settings: "Settings",
  },
  footer: {
    files: (n) => `${n} files`,
    run: (id, paused) => `Run ${id} ${paused ? "paused" : "running"}`,
    openPlans: (n) => (n === 0 ? "No open plans" : n === 1 ? "1 open plan" : `${n} open plans`),
    lastRun: (when) => (when ? `Last run ${when} UTC` : "No runs yet"),
    dryRun: "Dry run on",
    dryRunHint: "Plans are only simulated, no file is changed.",
  },
  scan: {
    percent: (p) => `Scan ${p}%`,
    cancel: "Cancel",
    starting: "Scan starting",
    phases: {
      crawl: "Finding files",
      detect: "Checking file types",
      meta: "Reading metadata",
      group: "Detecting related files",
      hash: "Computing checksums",
      thumbs: "Creating thumbnails",
      done: "Finishing",
    },
  },
  system: {
    sources: { bundled: "bundled", path: "system PATH", env: "environment variable" },
    checking: "Checking tools …",
    gplHint: "Releases bundle an LGPL build.",
    gpl: "GPL build, development only",
    database: "Database",
    schema: (v) => `Schema v${v}`,
  },
};
