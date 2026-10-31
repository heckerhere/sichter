import type { Messages } from "..";

const plural = (n: number, one: string, many: string) => (n === 1 ? one : many);

export const events: Messages["events"] = {
  title: "Events",
  summary: (events, confirmed) => `${events} events · ${confirmed} confirmed`,
  settingsToggle: "Suggestion settings",
  intro:
    "Sichter has grouped your photos into occasions by time and place. Rename, split or merge them and confirm what fits. This only affects Sichter, your files stay unchanged.",
  settings: {
    gapBefore: "New event after",
    gapAfter: "hour gap",
    gpsBefore: "or after",
    gpsAfter: "km distance",
    mergeMultiday: "Combine multi-day stays at the same place",
    recluster: "Recalculate suggestions",
    videoTzTitle: "Video time without time zone",
    videoTzHint:
      "By the standard this time is UTC, but many cameras write local time. Phones usually follow the standard. Choose per camera how Sichter should read it.",
    videos: (n) => `${n} ${plural(n, "video", "videos")}`,
    localTime: "Local time",
    utc: "UTC",
    isDefault: "Default",
    reset: "reset",
  },
  reclusterDialog: {
    message:
      "All unconfirmed events will be rebuilt from your settings. Renames and changes to unconfirmed events will be lost. Confirmed events stay as they are. Your files are not changed.",
    title: "Recalculate suggestions",
    ok: "Recalculate",
  },
  selection: {
    count: (n) => `${n} ${plural(n, "event", "events")} selected`,
    merge: "Merge",
    clear: "Deselect",
  },
  empty: "No events yet. They are created automatically when you scan in the Library.",
  unassigned: (n) =>
    `${n} ${n === "1" ? "photo" : "photos"} without an event (metadata not read yet). They will be sorted in on the next scan.`,
  card: {
    selectForMerge: "Select for merging",
    openEvent: "Open event",
    untitled: "Untitled",
    photos: (n) => `${n} ${n === "1" ? "photo" : "photos"}`,
    videosOf: (n) => `, ${n} of them ${plural(n, "video", "videos")}`,
    days: (n) => `${n} ${plural(n, "day", "days")}`,
    placeHint: "Nearest larger place to the GPS center (GeoNames)",
    suggestionHint: "Suggested from the previous folder name",
    useSuggestion: (title) => `Use “${title}”`,
    uncertainHint: "Date taken from the file name or modification date",
    uncertain: (n) => `${n} with uncertain date`,
    tzHint: "With the other time zone setting, video times match this camera's photos better",
    tzCheck: (camera) => `Check time zone: ${camera}`,
    confirmedHint: "Confirmed: not changed by “Recalculate”. Click to undo.",
    confirmHint: "Confirm event as correct",
    confirmed: "✓ Confirmed",
    confirm: "Confirm",
    open: "Open",
  },
  detail: {
    back: "← Back",
    photos: (n) => `${n} ${n === "1" ? "photo" : "photos"}`,
    notePlaceholder: "Event note",
    selected: (n) => `${n} selected`,
    selectHint: "Click photos to select them",
    splitHint: "The selected photo and all later ones become a separate event",
    split: "Split here",
    moveTo: "Move to:",
    previous: "Previous event",
    next: "Next event",
    newEvent: "New event",
    clearSelection: "Deselect",
    onlyInApp: "Only in Sichter, your files stay unchanged.",
  },
};
