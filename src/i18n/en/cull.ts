import type { Messages } from "..";

const plural = (n: number, one: string, many: string) => (n === 1 ? one : many);

export const cull: Messages["cull"] = {
  title: "Cull",
  intro:
    "Rate photos full screen with the keyboard. Rejects stay where they are until you move them in a separate step. Ratings are stored only in Sichter, your files stay unchanged.",
  rejectsMarked: (n) => `${n} ${plural(n, "photo", "photos")} marked as reject.`,
  rejectsStay: "They are still in their original location.",
  moveToRejectsFolder: "Move to _Rejected …",
  toTrash: "Move to Trash …",
  xmpHint: "Save ratings of RAW photos as XMP sidecars for Lightroom and others.",
  xmpExport: "Export as XMP …",
  allPhotos: "All photos",
  totals: (rated, count, keep, reject) => `${rated} of ${count} rated · ${keep} kept · ${reject} rejected`,
  photos: (n) => `${n} ${n === "1" ? "photo" : "photos"}`,
  barTitle: (keep, reject, rated) => `${keep} kept, ${reject} rejected, ${rated} rated`,
  rated: (rated, count) => `${rated} / ${count} rated`,
  empty: "No photos yet. Add a folder in the Library.",
  filters: {
    all: "All",
    unrated: "Unrated",
    keep: "Picks",
    reject: "Rejects",
    stars: "3+ stars",
  },
  loupe: {
    back: "← Back",
    autoAdvance: "Advance after P/X",
    single: "Single photo",
    compare: (n) => `Compare ${n} photos`,
    noneForFilter: "No photos for this filter.",
    videoUnsupported: "This video format can't be played here.",
    openInPlayer: "Open in default player",
    noPreview: "No preview available",
    keep: "Pick",
    reject: "Reject",
    burstHint: "Burst: press B to show all photos side by side",
    burst: (pos) => `Burst ${pos}`,
    uncertainDate: " · Uncertain date",
    shortcuts:
      "← → browse · P pick · X reject · 1–5 stars · 0 no stars · U reset · C compare · B burst · Space video · Esc",
    flash: {
      reset: "Reset",
      keep: "Pick",
      reject: "Reject",
      stars: (n) => (n === 1 ? "1 star" : `${n} stars`),
    },
  },
  trash: {
    title: "Move rejects to Trash",
    intro:
      "Check each photo that should go to the Trash. All its related files go with it (RAW, JPEG, sidecars). You'll review the plan afterwards.",
    empty: "Nothing marked as reject.",
    selectNone: "Select none",
    selectAll: "Select all",
    approved: (n, size) => `${n} approved · ${size}`,
    plan: "Create plan",
  },
};
