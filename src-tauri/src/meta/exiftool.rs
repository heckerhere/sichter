//! Langlebiger exiftool-Prozess im `-stay_open`-Modus.
//!
//! Argumente gehen zeilenweise über stdin. Nach jedem Auftrag schreibt
//! exiftool `{readyN}` auf stdout; per `-echo4` setzen wir dieselbe Marke auf
//! stderr, damit beide Ströme pro Auftrag sauber getrennt gelesen werden.
//! stderr wird in einem eigenen Thread geleert, damit ein voller Pipe-Puffer
//! exiftool nie blockiert. exiftool wird hier nur lesend verwendet.

use std::io::{self, BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Stdio};
use std::sync::mpsc::{self, Receiver};

use crate::tools::ToolCommand;

#[derive(Debug, thiserror::Error)]
pub enum ExifToolError {
    #[error("exiftool konnte nicht gestartet werden: {0}")]
    Spawn(io::Error),
    #[error("Kommunikation mit exiftool fehlgeschlagen: {0}")]
    Io(#[from] io::Error),
    #[error("exiftool wurde unerwartet beendet")]
    Exited,
    #[error("Argument enthält einen Zeilenumbruch: {0:?}")]
    InvalidArgument(String),
}

#[derive(Debug)]
pub struct ExifOutput {
    pub stdout: String,
    pub stderr: String,
}

pub struct ExifTool {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    stderr_lines: Receiver<String>,
    counter: u64,
}

impl ExifTool {
    pub fn spawn(tool: &ToolCommand) -> Result<Self, ExifToolError> {
        let mut child = tool
            .command()
            .args(["-stay_open", "True", "-@", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(ExifToolError::Spawn)?;
        let stdin = child.stdin.take().ok_or(ExifToolError::Exited)?;
        let stdout = BufReader::new(child.stdout.take().ok_or(ExifToolError::Exited)?);
        let stderr = BufReader::new(child.stderr.take().ok_or(ExifToolError::Exited)?);
        let (tx, stderr_lines) = mpsc::channel();
        std::thread::Builder::new()
            .name("exiftool-stderr".into())
            .spawn(move || {
                for line in stderr.lines().map_while(Result::ok) {
                    if tx.send(line).is_err() {
                        break;
                    }
                }
            })
            .map_err(ExifToolError::Spawn)?;
        Ok(Self { child, stdin, stdout, stderr_lines, counter: 0 })
    }

    /// Führt einen Auftrag aus. Dateinamen werden als UTF-8 übergeben.
    pub fn execute<S: AsRef<str>>(&mut self, args: &[S]) -> Result<ExifOutput, ExifToolError> {
        self.counter += 1;
        let marker = format!("{{ready{}}}", self.counter);

        let mut request = String::from("-charset\nfilename=utf8\n");
        for arg in args {
            let arg = arg.as_ref();
            if arg.contains('\n') || arg.contains('\r') {
                return Err(ExifToolError::InvalidArgument(arg.to_string()));
            }
            request.push_str(arg);
            request.push('\n');
        }
        request.push_str(&format!("-echo4\n{marker}\n-execute{}\n", self.counter));
        self.stdin.write_all(request.as_bytes())?;
        self.stdin.flush()?;

        let stdout = read_until_marker(&mut self.stdout, &marker)?;
        let mut stderr = String::new();
        loop {
            let line = self.stderr_lines.recv().map_err(|_| ExifToolError::Exited)?;
            if line.trim_end() == marker {
                break;
            }
            stderr.push_str(&line);
            stderr.push('\n');
        }
        Ok(ExifOutput { stdout, stderr })
    }

    pub fn version(&mut self) -> Result<String, ExifToolError> {
        Ok(self.execute(&["-ver"])?.stdout.trim().to_string())
    }
}

fn read_until_marker(reader: &mut impl BufRead, marker: &str) -> Result<String, ExifToolError> {
    let mut collected = String::new();
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            return Err(ExifToolError::Exited);
        }
        if line.trim_end() == marker {
            return Ok(collected);
        }
        collected.push_str(&line);
    }
}

impl Drop for ExifTool {
    fn drop(&mut self) {
        let _ = self.stdin.write_all(b"-stay_open\nFalse\n");
        let _ = self.stdin.flush();
        if self.child.wait().is_err() {
            let _ = self.child.kill();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::Tools;
    use std::path::Path;

    fn exiftool() -> ExifTool {
        let tool = Tools::resolve(None).exiftool.expect("exiftool fehlt");
        ExifTool::spawn(&tool).unwrap()
    }

    fn fixture(rel: &str) -> String {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/library")
            .join(rel)
            .to_string_lossy()
            .into_owned()
    }

    #[test]
    fn reports_version_repeatedly() {
        let mut et = exiftool();
        let v1 = et.version().unwrap();
        let v2 = et.version().unwrap();
        assert_eq!(v1, v2);
        assert!(v1.parse::<f64>().is_ok(), "{v1}");
    }

    #[test]
    fn reads_raw_and_video_dates() {
        let mut et = exiftool();
        let out = et
            .execute(&[
                "-j",
                "-DateTimeOriginal",
                "-QuickTime:CreationDate",
                "-Model",
                &fixture("raw/CR3_0001.CR3"),
                &fixture("iPhone/IMG_0100.MOV"),
                &fixture("2023-05-12 Geburtstag/IMG_0001.JPG"),
            ])
            .unwrap();
        let json: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
        assert_eq!(json[0]["Model"], "Canon EOS M50");
        assert_eq!(json[1]["CreationDate"], "2024:07:14 18:05:00+02:00");
        assert_eq!(json[2]["DateTimeOriginal"], "2023:05:12 15:01:10");
    }

    #[test]
    fn errors_go_to_stderr_without_blocking() {
        let mut et = exiftool();
        let out = et.execute(&["-j", "/gibt/es/nicht.jpg"]).unwrap();
        assert!(out.stderr.contains("not found") || out.stderr.contains("Error"), "{out:?}");
        // Prozess bleibt nutzbar.
        assert!(!et.version().unwrap().is_empty());
    }

    #[test]
    fn handles_unicode_paths() {
        let mut et = exiftool();
        let out = et.execute(&["-j", "-FileName", &fixture("2023-05-12 Geburtstag/IMG_0002.JPG")]).unwrap();
        assert!(out.stdout.contains("IMG_0002.JPG"));
    }

    #[test]
    fn rejects_newlines_in_arguments() {
        let mut et = exiftool();
        assert!(matches!(et.execute(&["a\nb"]), Err(ExifToolError::InvalidArgument(_))));
    }
}
