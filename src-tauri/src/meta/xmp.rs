//! Inhalte für den XMP-Export. Nur Erzeugen von Bytes; geschrieben wird
//! ausschließlich über `fsops::write_sidecar`, ausgelöst vom Journal.
//!
//! Bewertung nach Lightroom/Bridge-Konvention: `xmp:Rating` 1–5, Ausschuss -1.
//! "Behalten" hat in XMP keine Entsprechung und wird nicht geschrieben.

use std::path::Path;
use std::process::Stdio;

use crate::tools::ToolCommand;

/// Minimales XMP-Sidecar mit nur der Bewertung.
pub fn new_sidecar(rating: i64) -> Vec<u8> {
    format!(
        "<?xpacket begin='\u{feff}' id='W5M0MpCehiHzreSzNTczkc9d'?>\n\
         <x:xmpmeta xmlns:x='adobe:ns:meta/' x:xmptk='Sichter'>\n\
         <rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'>\n\
         \x20<rdf:Description rdf:about='' xmlns:xmp='http://ns.adobe.com/xap/1.0/'>\n\
         \x20 <xmp:Rating>{rating}</xmp:Rating>\n\
         \x20</rdf:Description>\n\
         </rdf:RDF>\n\
         </x:xmpmeta>\n\
         <?xpacket end='w'?>\n"
    )
    .into_bytes()
}

/// Bestehendes Sidecar mit neuer Bewertung, alle übrigen Inhalte bleiben.
/// exiftool gibt das Ergebnis nur auf stdout aus, die Datei bleibt unberührt.
pub fn merge(exiftool: &ToolCommand, path: &Path, rating: i64) -> Result<Vec<u8>, String> {
    let output = exiftool
        .command()
        .args(["-q", "-m", "-o", "-", &format!("-XMP-xmp:Rating={rating}")])
        .arg(path)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() || !text.contains("xmpmeta") {
        return Err(format!("XMP konnte nicht gelesen werden: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(output.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::Tools;

    fn rating_of(et: &ToolCommand, bytes: &[u8]) -> String {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.xmp");
        crate::fsops::test_support::write(&p, bytes);
        let out = et.command().args(["-s3", "-XMP-xmp:Rating"]).arg(&p).output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    #[test]
    fn new_and_merged_sidecars_carry_the_rating() {
        let et = Tools::resolve(None).exiftool.unwrap();
        assert_eq!(rating_of(&et, &new_sidecar(3)), "3");
        assert_eq!(rating_of(&et, &new_sidecar(-1)), "-1");

        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/library/2023-05-12 Geburtstag/IMG_0001.xmp");
        let before = std::fs::read(&fixture).unwrap();
        let merged = merge(&et, &fixture, 4).unwrap();
        assert_eq!(std::fs::read(&fixture).unwrap(), before, "Original unberührt");
        assert_eq!(rating_of(&et, &merged), "4");
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("m.xmp");
        crate::fsops::test_support::write(&p, &merged);
        let model = et.command().args(["-s3", "-Model"]).arg(&p).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&model.stdout).trim(), "Canon DIGITAL IXUS 40", "übrige Inhalte bleiben");
    }
}
