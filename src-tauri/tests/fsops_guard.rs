//! Erzwingt PLAN.md, Abschnitt 4/5.3: Außerhalb von `src/fsops/` gibt es keinen
//! schreibenden Dateisystemzugriff. Läuft mit `cargo test` und damit in der CI.
//!
//! Die Prüfung ist absichtlich grob (Textsuche): Ein falscher Alarm ist
//! harmlos, ein übersehener Schreibzugriff nicht.

use std::path::{Path, PathBuf};

const FORBIDDEN: &[&str] = &[
    "fs::write",
    "fs::rename",
    "fs::copy",
    "fs::remove_file",
    "fs::remove_dir",
    "fs::create_dir",
    "fs::hard_link",
    "fs::soft_link",
    "fs::set_permissions",
    "File::create",
    "OpenOptions",
    "File::options",
    "create_new(",
    ".write(true)",
    ".append(true)",
    ".truncate(true)",
    "set_modified",
    "set_len(",
    "symlink(",
    "trash::",
    "remove_file",
    "remove_dir",
    "create_dir",
    "renamex_np",
    "MoveFileEx",
];

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn no_filesystem_writes_outside_fsops() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let fsops = src.join("fsops");
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    assert!(files.len() > 5, "Quellcode nicht gefunden");

    let mut violations = Vec::new();
    for file in files.iter().filter(|f| !f.starts_with(&fsops)) {
        let text = std::fs::read_to_string(file).unwrap();
        for (number, line) in text.lines().enumerate() {
            let code = line.split("//").next().unwrap_or_default();
            for pattern in FORBIDDEN {
                if code.contains(pattern) {
                    violations.push(format!(
                        "{}:{}: `{pattern}` in: {}",
                        file.strip_prefix(&src).unwrap().display(),
                        number + 1,
                        line.trim()
                    ));
                }
            }
            // `use std::fs::{write, ...}` o. Ä. würde die Muster oben umgehen.
            if code.contains("use std::fs::") && ["write", "rename", "copy", "remove", "create"].iter().any(|w| code.contains(w)) {
                violations.push(format!("{}:{}: Import schreibender fs-Funktion", file.display(), number + 1));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "Schreibender Dateizugriff außerhalb von fsops:\n{}",
        violations.join("\n")
    );
}

#[test]
fn guard_detects_violations() {
    // Selbsttest: Die Muster greifen bei typischen Schreibaufrufen.
    let samples = ["std::fs::write(p, b)", "File::create(p)", "fs::remove_file(p)", "trash::delete(p)"];
    for sample in samples {
        assert!(FORBIDDEN.iter().any(|p| sample.contains(p)), "{sample}");
    }
}
