import type { Messages } from "..";

export const reorganize: Messages["reorganize"] = {
  placeholders: ["{Year}", "{Month}", "{Date}", "{Event}", "{Place}", "{Camera}", "{Original-Filename}"],
  defaultTemplate: "{Year}/{Date} {Event}/{Original-Filename}",
  defaultVideoTemplate: "{Year}/{Date} {Event}/Videos/{Original-Filename}",
  title: "Reorganize",
  intro:
    "Choose how your photos should be organized. Sichter turns this into a plan that you see in full before anything happens. Nothing is overwritten, and every run can be undone.",
  noRoots: "No reachable, unlocked folder in the library.",
  source: "Source",
  locked: "locked",
  unreachable: "not reachable",
  target: "Target",
  targetHint: "The target must be a library folder and may be the source itself.",
  move: "Move (default)",
  copy: "Copy, keep source",
  template: "Template",
  standard: "Default",
  dateRange: "Multi-day events as “2024-07-12 to 2024-07-20”",
  separateVideos: "Separate template for videos",
  onlyConfirmedHint: "Shots in unconfirmed events stay where they are",
  onlyConfirmed: "Confirmed events only",
  note: "Shots without an event go to {Year}/_Unsorted. RAW+JPEG, sidecars and Live Photos always stay together. Unknown file types are not moved.",
  preview: "Preview",
  nothingToDo: "Everything already matches the template.",
  creating: "Creating plan …",
  create: "Create and review plan",
};
