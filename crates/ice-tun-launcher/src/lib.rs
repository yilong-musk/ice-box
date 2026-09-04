//! Host-free helpers for the Windows TUN scheduled-task binary pin.
//!
//! The per-user install directory is writable, so `ice-tun-launcher.exe` and
//! `sing-box.exe` can be replaced by the same account. The scheduled task is
//! created elevated and stores SHA-256 hashes in its description (`/D`);
//! `schtasks /Run` and this launcher refuse to start when the on-disk files
//! do not match.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// Fixed name of the scheduled task that runs this launcher elevated.
pub const TUN_TASK_NAME: &str = "ice-box-tun";

/// Prefix of the scheduled-task description that carries the binary pin.
pub const TUN_TASK_PIN_PREFIX: &str = "ice-box-pin:";

/// SHA-256 pin of the launcher and the sibling `sing-box.exe`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TunTaskPin {
    pub launcher_sha256: String,
    pub core_sha256: String,
}

/// Render the `/D` description stored on the TUN scheduled task.
pub fn format_tun_task_pin(launcher_sha256: &str, core_sha256: &str) -> String {
    format!("{TUN_TASK_PIN_PREFIX}{launcher_sha256}:{core_sha256}")
}

/// Parse a task description produced by [`format_tun_task_pin`].
pub fn parse_tun_task_pin(description: &str) -> Option<TunTaskPin> {
    let rest = description.trim().strip_prefix(TUN_TASK_PIN_PREFIX)?;
    let (launcher, core) = rest.split_once(':')?;
    if !is_sha256_hex(launcher) || !is_sha256_hex(core) {
        return None;
    }
    Some(TunTaskPin {
        launcher_sha256: launcher.to_ascii_lowercase(),
        core_sha256: core.to_ascii_lowercase(),
    })
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64 && value.chars().all(|c| c.is_ascii_hexdigit())
}

/// Read the pin out of `schtasks /Query /XML` output (`Description` or `Comment`).
pub fn extract_tun_task_pin_from_xml(xml: &str) -> Option<TunTaskPin> {
    for tag in ["Description", "Comment"] {
        if let Some(value) = xml_tag_value(xml, tag) {
            if let Some(pin) = parse_tun_task_pin(&value) {
                return Some(pin);
            }
        }
    }
    None
}

fn xml_tag_value(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = xml.find(&open)?;
    let rest = &xml[start + open.len()..];
    let end = rest.find(&close)?;
    Some(rest[..end].trim().to_string())
}

/// SHA-256 of a file, lowercase hex (pinned in the scheduled-task description).
pub fn sha256_of_file(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|err| format!("read {}: {err}", path.display()))?;
    Ok(sha256_hex(&bytes))
}

/// SHA-256 of bytes, lowercase hex.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// Decode `schtasks` stdout, which is often UTF-16 LE with a BOM on Windows.
pub fn decode_schtasks_output(bytes: &[u8]) -> String {
    if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE {
        let units: Vec<u16> = bytes[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&chunk| u16::from_le_bytes(chunk))
            .collect();
        String::from_utf16_lossy(&units)
    } else if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
        let units: Vec<u16> = bytes[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&chunk| u16::from_be_bytes(chunk))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

/// The bundled sing-box next to `ice-tun-launcher.exe`.
pub fn core_beside_launcher(launcher: &Path) -> Option<PathBuf> {
    Some(launcher.parent()?.join("sing-box.exe"))
}

/// Whether both on-disk binaries match the scheduled-task pin.
pub fn pin_matches_files(pin: &TunTaskPin, launcher: &Path, core: &Path) -> Result<bool, String> {
    let launcher_sha = sha256_of_file(launcher)?;
    let core_sha = sha256_of_file(core)?;
    Ok(launcher_sha == pin.launcher_sha256 && core_sha == pin.core_sha256)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LAUNCHER: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const CORE: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    #[test]
    fn pin_roundtrip_is_stable_and_rejects_junk() {
        let rendered = format_tun_task_pin(LAUNCHER, CORE);
        let parsed = parse_tun_task_pin(&rendered).expect("pin");
        assert_eq!(parsed.launcher_sha256, LAUNCHER);
        assert_eq!(parsed.core_sha256, CORE);
        assert!(parse_tun_task_pin("not-a-pin").is_none());
        assert!(parse_tun_task_pin(&format!("{TUN_TASK_PIN_PREFIX}short:hash")).is_none());
        assert!(parse_tun_task_pin(&format_tun_task_pin(LAUNCHER, "zzzz")).is_none());
        let mixed = format_tun_task_pin(&LAUNCHER.to_ascii_uppercase(), &CORE.to_ascii_uppercase());
        let parsed = parse_tun_task_pin(&mixed).expect("uppercase hex");
        assert_eq!(parsed.launcher_sha256, LAUNCHER);
        assert_eq!(parsed.core_sha256, CORE);
    }

    #[test]
    fn xml_extracts_description_or_comment() {
        let pin = format_tun_task_pin(LAUNCHER, CORE);
        let description = format!(
            "<Task><RegistrationInfo><Description>{pin}</Description></RegistrationInfo></Task>"
        );
        assert_eq!(
            extract_tun_task_pin_from_xml(&description).expect("description"),
            parse_tun_task_pin(&pin).expect("parsed")
        );
        let comment =
            format!("<Task><RegistrationInfo><Comment>{pin}</Comment></RegistrationInfo></Task>");
        assert_eq!(
            extract_tun_task_pin_from_xml(&comment).expect("comment"),
            parse_tun_task_pin(&pin).expect("parsed")
        );
        assert!(extract_tun_task_pin_from_xml("<Task/>").is_none());
    }

    #[test]
    fn sha256_of_hello_is_stable() {
        let dir = std::env::temp_dir().join(format!(
            "ice-box-tun-pin-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("hello");
        std::fs::write(&file, b"hello").unwrap();
        assert_eq!(
            sha256_of_file(&file).expect("sum"),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pin_matches_files_detects_replacement() {
        let dir = std::env::temp_dir().join(format!(
            "ice-box-tun-pin-files-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let launcher = dir.join("ice-tun-launcher.exe");
        let core = dir.join("sing-box.exe");
        std::fs::write(&launcher, b"launcher-v1").unwrap();
        std::fs::write(&core, b"core-v1").unwrap();
        let pin = TunTaskPin {
            launcher_sha256: sha256_of_file(&launcher).unwrap(),
            core_sha256: sha256_of_file(&core).unwrap(),
        };
        assert!(pin_matches_files(&pin, &launcher, &core).expect("match"));
        std::fs::write(&core, b"core-replaced").unwrap();
        assert!(!pin_matches_files(&pin, &launcher, &core).expect("mismatch"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn decode_schtasks_output_handles_utf16le_bom() {
        let text = "hello";
        let mut bytes = vec![0xFF, 0xFE];
        for unit in text.encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        assert_eq!(decode_schtasks_output(&bytes), "hello");
        assert_eq!(decode_schtasks_output(b"ascii"), "ascii");
    }

    #[test]
    fn core_beside_launcher_joins_sing_box_exe() {
        let launcher = PathBuf::from("opt")
            .join("ice-box")
            .join("ice-tun-launcher.exe");
        let path = core_beside_launcher(&launcher).expect("parent");
        assert_eq!(path.file_name().unwrap(), "sing-box.exe");
        assert_eq!(path.parent(), launcher.parent());
    }
}
