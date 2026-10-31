import type { Messages } from "..";

export const settings: Messages["settings"] = {
  title: "Settings",
  on: "On",
  off: "Off",
  language: {
    title: "Language",
    hint: "Language of the interface and of the folder names Sichter creates.",
    auto: "Automatic (system language)",
    de: "Deutsch",
    en: "English",
  },
  dryRun: {
    title: "Dry Run Mode",
    hint: "While it is on, plans can only be simulated as a dry run. No file is changed. On by default.",
    confirmMessage: "You will then be able to really execute plans. Every run still asks for confirmation with the number of files.",
    confirmTitle: "Turn Off Dry Run Mode",
    confirmOk: "Turn Off",
  },
  xmp: {
    title: "XMP Export of Ratings",
    hint:
      "Writes stars and rejects (rating -1) to XMP sidecars next to RAW files so Lightroom, Capture One and Bridge can see them. Never into the image file itself. Existing XMP files are extended and backed up first; everything goes through a plan and can be undone. Off by default.",
  },
  maintenance: "Maintenance",
  emptyDirs: {
    title: "Clean Up Empty Folders",
    hint:
      "Finds folders that contain nothing but .DS_Store or Thumbs.db, for example after reorganizing. You review the plan and confirm; the folders are moved to the Trash. Locked folders are not searched.",
    search: "Search …",
  },
  cache: {
    title: "Thumbnails",
    sizes: (grid, cull, logs) => `Grid ${grid} · Cull ${cull} · Logs ${logs}`,
    clearCull: "Clear Cull Cache",
    clearGrid: "Clear Grid Cache",
    confirmThumbs: "All thumbnails will be deleted and recreated on the next scan. Your photos are not affected.",
    confirmPreviews: "The large previews used for culling will be deleted and recreated when needed. Your photos are not affected.",
    confirmTitle: "Clear Cache",
    confirmOk: "Clear",
  },
  dataDir: {
    title: "Data Folder",
    show: "Show in Finder",
  },
  systemStatus: "System Status",
  credits: "Place names: GeoNames (geonames.org), CC BY 4.0 license. Metadata: ExifTool by Phil Harvey. Video and HEIC: FFmpeg (LGPL 2.1).",
};
