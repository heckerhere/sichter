import type { Messages } from "..";

const files = (n: string | number) => (String(n) === "1" ? "1 file" : `${n} files`);

export const plan: Messages["plan"] = {
  back: "← History",
  runStatus: { planned: "planned", running: "running", done: "done", aborted: "cancelled", undone: "undone" },
  opStatus: { planned: "planned", done: "done", failed: "failed", undone: "undone" },
  opLabel: { mkdir: "Create folder", move: "Move", copy: "Copy", trash: "Trash", xmp: "Write XMP", export: "Import" },
  conflict: {
    duplicate_at_target: "Already identical at the target, stays at the source",
    renamed: "Name taken at the target, renamed with (2), (3) …",
    not_hashed: "Not fully scanned yet, skipped",
    group_partial: "Part of a RAW+JPEG group that is not fully duplicated, skipped",
  },
  meta: (id, created) => `Run ${id} · created ${created} UTC`,
  dryRunActive: "Dry-run mode is on: you can do a dry run, nothing will be changed.",
  dryRunDisableButton: "Turn off to run",
  counters: {
    files: "Files",
    units: (bytes, n) => `${bytes} · ${n} ${n === "1" ? "item" : "items"}`,
    newFolders: "New folders",
    alreadyImported: "Already imported / no original",
    inPlaceSkipped: "Already in place / skipped",
    space: "Space at target",
    noCopy: "no copying",
    free: (bytes, notEnough) => `${bytes} free${notEnough ? " – not enough" : ""}`,
    sameDrive: "Moving on the same drive",
  },
  notes: (n) => (n === 1 ? "1 note" : `${n} notes`),
  sides: {
    fromPhotos: "Arriving from the Photos library",
    before: "Before: files leaving",
    after: "After: files arriving",
  },
  relativeTo: (p) => `Paths relative to ${p}`,
  progress: {
    paused: "Paused",
    running: "Running",
    resume: "Resume",
    pause: "Pause",
    cancelHint: "Cancel stops after the current group. The run can then be resumed or undone.",
  },
  result: {
    dry: (done, failed) => `Dry run: ${files(done)} ready, ${failed} ${failed === 1 ? "problem" : "problems"}. Nothing was changed.`,
    undo: (undone, failed) => `Undo: ${files(undone)} restored, ${failed} ${failed === 1 ? "problem" : "problems"}.`,
    executed: (cancelled, done, failed, undone) =>
      `${cancelled ? "Cancelled" : "Done"}: ${files(done)} done, ${failed} failed${undone ? `, ${undone} reverted to keep related files together` : ""}.`,
    manualRestore: (n) =>
      `${n === 1 ? "1 file is" : `${n} files are`} in the Trash. Open the Trash and choose “Put Back” to restore ${n === 1 ? "it" : "them"}.`,
  },
  actions: {
    dry: "Dry Run",
    dryRunActive: "Dry-run mode is on",
    resume: "Resume …",
    execute: "Run …",
    undo: "Undo …",
    discard: "Discard plan",
    scanTarget: "Scan target folder",
    openLog: "Open log",
  },
  ops: {
    title: (total) => `Operations (${total}`,
    firstN: (n) => `, first ${n}; full list in the log`,
    keeps: (p) => `keeps: ${p}`,
  },
  trash: {
    title: (n) => `Move ${files(n)} to the Trash?`,
    body: "The files go to the system Trash and can be restored there with “Put Back”. For duplicates, Sichter checks right before that the copy being kept is present and unchanged.",
    confirm: (n) => `Move ${files(n)} to Trash`,
  },
  dialogs: {
    importMessage: (count, bytes, target, mkdirs) =>
      `${count} ${count === "1" ? "original" : "originals"} (${bytes}) will be copied from the Photos library to “${target}”, ${mkdirs} ${mkdirs === "1" ? "folder" : "folders"} created. The library stays unchanged, nothing is overwritten. Originals stored only in iCloud may take a while.`,
    importTitle: "Run Import",
    importOk: (count) => `Import ${files(count)}`,
    executeMessage: (count, bytes, copy, mkdirs) =>
      `${files(count)} (${bytes}) will be ${copy ? "copied" : "moved"}, ${mkdirs} ${mkdirs === "1" ? "folder" : "folders"} created. Nothing is overwritten. The run can be undone afterwards.`,
    executeTitle: "Run Plan",
    executeOk: (count, copy) => `${copy ? "Copy" : "Move"} ${files(count)}`,
    undoMessage:
      "All files of this run are returned to their previous location, created folders are removed if empty. Copies go to the Trash. Files in the Trash must be restored yourself with “Put Back”.",
    undoTitle: "Undo Run",
    undoOk: "Undo",
    dryRunMessage: "You can then really run plans. Every run still asks for confirmation first.",
    dryRunTitle: "Turn Off Dry-Run Mode",
    dryRunOk: "Turn Off",
  },
  history: {
    title: "History",
    intro: "All plans and runs. Executed runs can be undone as a whole, and each one has a log.",
    emptyBefore: "No plans yet. Create one in ",
    emptyLink: "Reorganize",
    emptyAfter: ", in Duplicates or while culling.",
    status: {
      planned: "Plan, not run yet",
      running: "running",
      done: "done",
      aborted: "cancelled, resumable",
      undone: "undone",
    },
    files: (n) => files(n),
    done: (n) => `${n} done`,
    failed: (n) => `${n} failed`,
    undone: (n) => `${n} restored`,
  },
};
