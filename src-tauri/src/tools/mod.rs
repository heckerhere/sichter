//! Auffinden der externen Werkzeuge exiftool, ffmpeg und ffprobe.
//!
//! Reihenfolge: Umgebungsvariable (`SICHTER_EXIFTOOL`, `SICHTER_FFMPEG`,
//! `SICHTER_FFPROBE`), dann ffmpeg/ffprobe als Sidecar neben der App-Datei
//! (so bündelt und signiert Tauri sie), dann gebündelt im
//! Ressourcenverzeichnis, in Debug-Builds zusätzlich `src-tauri/resources`
//! und `src-tauri/binaries`, zuletzt der `PATH`.
//!
//! exiftool wird als Perl-Distribution (`exiftool` + `lib/`) gebündelt und mit
//! dem System-Perl gestartet. Unter Windows wird später die exe-Variante
//! (`exiftool.exe` + `exiftool_files/`) erwartet.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolSource {
    Env,
    Bundled,
    Path,
}

/// Ein auflösbarer Programmaufruf: Programm plus feste Vorab-Argumente
/// (bei exiftool z. B. `perl /pfad/exiftool`).
#[derive(Debug, Clone)]
pub struct ToolCommand {
    pub program: PathBuf,
    pub prefix_args: Vec<OsString>,
    /// Pfad, der dem Nutzer angezeigt wird.
    pub display_path: PathBuf,
    pub source: ToolSource,
}

impl ToolCommand {
    pub fn command(&self) -> Command {
        let mut cmd = Command::new(&self.program);
        cmd.args(&self.prefix_args);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        cmd
    }

    fn direct(path: PathBuf, source: ToolSource) -> Self {
        Self {
            program: path.clone(),
            prefix_args: Vec::new(),
            display_path: path,
            source,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Tools {
    pub exiftool: Option<ToolCommand>,
    pub ffmpeg: Option<ToolCommand>,
    pub ffprobe: Option<ToolCommand>,
}

impl Tools {
    /// `resource_dir` ist das Ressourcenverzeichnis der gebündelten App.
    pub fn resolve(resource_dir: Option<&Path>) -> Self {
        let mut bundle_dirs: Vec<PathBuf> = resource_dir.map(Path::to_path_buf).into_iter().collect();
        if cfg!(debug_assertions) {
            bundle_dirs.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("resources"));
        }
        Self {
            exiftool: resolve_exiftool(&bundle_dirs),
            ffmpeg: resolve_binary("ffmpeg", "SICHTER_FFMPEG", &bundle_dirs),
            ffprobe: resolve_binary("ffprobe", "SICHTER_FFPROBE", &bundle_dirs),
        }
    }
}

fn exe_name(name: &str) -> String {
    format!("{name}{}", std::env::consts::EXE_SUFFIX)
}

fn resolve_binary(name: &str, env_var: &str, bundle_dirs: &[PathBuf]) -> Option<ToolCommand> {
    if let Some(path) = std::env::var_os(env_var).map(PathBuf::from).filter(|p| p.is_file()) {
        return Some(ToolCommand::direct(path, ToolSource::Env));
    }
    // Sidecar: Tauri legt sie ohne Target-Suffix neben die ausführbare Datei.
    let beside_exe = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join(exe_name(name)))).filter(|p| p.is_file());
    if let Some(p) = beside_exe {
        return Some(ToolCommand::direct(p, ToolSource::Bundled));
    }
    if cfg!(debug_assertions) {
        let dev = Path::new(env!("CARGO_MANIFEST_DIR")).join("binaries").join(exe_name(&format!("{name}-{}", env!("SICHTER_TARGET"))));
        if dev.is_file() {
            return Some(ToolCommand::direct(dev, ToolSource::Bundled));
        }
    }
    bundle_dirs
        .iter()
        .map(|dir| dir.join("bin").join(exe_name(name)))
        .find(|p| p.is_file())
        .map(|p| ToolCommand::direct(p, ToolSource::Bundled))
        .or_else(|| find_in_path(&exe_name(name)).map(|p| ToolCommand::direct(p, ToolSource::Path)))
}

fn resolve_exiftool(bundle_dirs: &[PathBuf]) -> Option<ToolCommand> {
    if let Some(path) = std::env::var_os("SICHTER_EXIFTOOL").map(PathBuf::from).filter(|p| p.is_file()) {
        return Some(exiftool_command(path, ToolSource::Env));
    }
    for dir in bundle_dirs {
        let exe = dir.join("exiftool").join("exiftool.exe");
        if cfg!(windows) && exe.is_file() {
            return Some(ToolCommand::direct(exe, ToolSource::Bundled));
        }
        let script = dir.join("exiftool").join("exiftool");
        if script.is_file() {
            return Some(exiftool_command(script, ToolSource::Bundled));
        }
    }
    find_in_path(&exe_name("exiftool")).map(|p| exiftool_command(p, ToolSource::Path))
}

/// Wächter für exiftool im `-stay_open`-Modus: exiftool beendet sich bei
/// geschlossenem stdin nicht von selbst. Stirbt die App hart (Absturz,
/// Neustart), würde es als verwaister Prozess weiterlaufen. Der Wächter startet
/// exiftool als Kindprozess und beendet es, sobald die App nicht mehr lebt
/// (anderer Elternprozess bzw. PID 1, auch wenn die App schon vor dem ersten
/// Blick verschwunden war).
const EXIFTOOL_WATCHDOG: &str = r#"use POSIX ":sys_wait_h";
my $script = shift;
my $pid = fork();
die "fork: $!" unless defined $pid;
if (!$pid) { exec($^X, $script, @ARGV) or exit 127; }
my $parent = getppid();
while (1) {
    my $done = waitpid($pid, WNOHANG);
    exit($? >> 8) if $done == $pid;
    my $ppid = getppid();
    if ($ppid != $parent || $ppid == 1) { kill "TERM", $pid; waitpid($pid, 0); exit 1; }
    select(undef, undef, undef, 0.5);
}"#;

/// Perl-Skripte explizit über Perl starten, damit Ausführungsrechte und
/// Shebang im Bundle keine Rolle spielen.
fn exiftool_command(path: PathBuf, source: ToolSource) -> ToolCommand {
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe")) {
        // Windows: Absicherung über Job-Objekt folgt in Phase 7.
        return ToolCommand::direct(path, source);
    }
    let perl = find_in_path(&exe_name("perl")).unwrap_or_else(|| PathBuf::from("perl"));
    ToolCommand {
        program: perl,
        prefix_args: vec!["-e".into(), EXIFTOOL_WATCHDOG.into(), path.clone().into_os_string()],
        display_path: path,
        source,
    }
}

fn find_in_path(file_name: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    let mut dirs: Vec<PathBuf> = std::env::split_paths(&paths).collect();
    // Aus dem Finder gestartete macOS-Apps erben einen minimalen PATH.
    if cfg!(target_os = "macos") {
        dirs.extend(["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"].map(PathBuf::from));
    }
    dirs.into_iter().map(|d| d.join(file_name)).find(|p| p.is_file())
}

/// Erste Zeile von `<tool> -version`, z. B. "ffmpeg version 8.1 ...".
pub fn ffmpeg_style_version(tool: &ToolCommand) -> Result<String, String> {
    let output = tool
        .command()
        .args(["-hide_banner", "-version"])
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let first = stdout.lines().next().unwrap_or_default();
    // "ffmpeg version 8.1 Copyright ..." -> "8.1"
    Ok(first
        .split_whitespace()
        .nth(2)
        .unwrap_or(first)
        .to_string())
}

/// Ob der Build GPL-Komponenten enthält (für den Lizenzhinweis im UI; für
/// Releases ist ein LGPL-Build vorgesehen).
pub fn ffmpeg_is_gpl(tool: &ToolCommand) -> Option<bool> {
    let output = tool.command().args(["-hide_banner", "-buildconf"]).output().ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    Some(text.contains("--enable-gpl"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_dev_exiftool_bundle() {
        let tools = Tools::resolve(None);
        let et = tools.exiftool.expect("exiftool nicht gefunden – scripts/fetch-sidecars.sh ausführen");
        if et.source == ToolSource::Bundled {
            let name = et.display_path.file_name().unwrap().to_string_lossy().into_owned();
            assert!(name == "exiftool" || name == "exiftool.exe", "{name}");
        }
    }

    #[test]
    fn reports_ffmpeg_versions() {
        let tools = Tools::resolve(None);
        for tool in [tools.ffmpeg, tools.ffprobe] {
            let tool = tool.expect("ffmpeg/ffprobe nicht gefunden");
            let version = ffmpeg_style_version(&tool).unwrap();
            assert!(version.chars().next().is_some_and(|c| c.is_ascii_digit() || c == 'n'), "{version}");
        }
    }
}
