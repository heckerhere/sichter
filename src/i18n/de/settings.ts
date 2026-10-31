export const settings = {
  title: "Einstellungen",
  on: "An",
  off: "Aus",
  language: {
    title: "Sprache",
    hint: "Sprache der Oberfläche und der Ordnernamen, die Sichter anlegt.",
    auto: "Automatisch (Systemsprache)",
    de: "Deutsch",
    en: "English",
  },
  dryRun: {
    title: "Dry-Run-Modus",
    hint: "Solange er an ist, lassen sich Pläne nur als Probelauf durchspielen. Es wird keine Datei verändert. Standard ist an.",
    confirmMessage: "Danach kannst du Pläne wirklich ausführen. Jede Ausführung fragt weiterhin mit Dateianzahl nach.",
    confirmTitle: "Dry-Run-Modus ausschalten",
    confirmOk: "Ausschalten",
  },
  xmp: {
    title: "XMP-Export der Bewertungen",
    hint:
      "Schreibt Sterne und Ausschuss (Bewertung -1) in XMP-Sidecars neben RAW-Dateien, damit Lightroom, Capture One und Bridge sie sehen. Nie in die Bilddatei selbst. Bestehende XMP-Dateien werden ergänzt und vorher gesichert, alles läuft über einen Plan und lässt sich rückgängig machen. Standard ist aus.",
  },
  maintenance: "Wartung",
  emptyDirs: {
    title: "Leere Ordner bereinigen",
    hint:
      "Findet Ordner, die nichts außer .DS_Store oder Thumbs.db enthalten, etwa nach dem Umsortieren. Du siehst den Plan und bestätigst, die Ordner kommen in den Papierkorb. Gesperrte Ordner werden nicht durchsucht.",
    search: "Suchen …",
  },
  cache: {
    title: "Vorschaubilder",
    sizes: (grid: string, cull: string, logs: string) => `Grid ${grid} · Sichten ${cull} · Protokolle ${logs}`,
    clearCull: "Sichten-Cache leeren",
    clearGrid: "Grid-Cache leeren",
    confirmThumbs: "Alle Vorschaubilder werden gelöscht und beim nächsten Scan neu erzeugt. Deine Fotos sind davon nicht betroffen.",
    confirmPreviews: "Die großen Vorschauen für das Sichten werden gelöscht und bei Bedarf neu erzeugt. Deine Fotos sind davon nicht betroffen.",
    confirmTitle: "Cache leeren",
    confirmOk: "Leeren",
  },
  dataDir: {
    title: "Datenordner",
    show: "Im Finder zeigen",
  },
  systemStatus: "Systemstatus",
  credits: "Ortsnamen: GeoNames (geonames.org), Lizenz CC BY 4.0. Metadaten: ExifTool von Phil Harvey. Video und HEIC: FFmpeg (LGPL 2.1).",
};
