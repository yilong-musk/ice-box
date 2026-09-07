// SPDX-License-Identifier: GPL-3.0-or-later

//! Host-free helpers for the Windows TUN scheduled-task binary pin.
//!
//! The per-user install directory is writable, so `ice-tun-launcher.exe` and
//! `sing-box.exe` can be replaced by the same account. The scheduled task is
//! created elevated from UTF-16 XML so the SHA-256 pin lives in
//! `RegistrationInfo/Description` (`schtasks /D` is a day-of-week flag and
//! cannot store a description). `schtasks /Run` and this launcher refuse to
//! start when the on-disk files do not match.

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

/// argv modes of `ice-tun-launcher.exe`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LauncherCommand {
    /// Run the elevated TUN core (`--data <app-data-dir>`).
    Run { data_dir: PathBuf },
    /// Import the UTF-16 task XML (`--install-task --xml <path>`).
    InstallTask { xml: PathBuf },
    /// Delete the `ice-box-tun` scheduled task (`--delete-task`).
    DeleteTask,
}

/// Parse launcher argv (without argv0). Used by the binary and host-free tests.
pub fn parse_launcher_command(args: &[String]) -> Option<LauncherCommand> {
    let mut data_dir = None;
    let mut xml = None;
    let mut install = false;
    let mut delete = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--install-task" => install = true,
            "--delete-task" => delete = true,
            "--data" => {
                i += 1;
                data_dir = Some(PathBuf::from(args.get(i)?));
            }
            "--xml" => {
                i += 1;
                xml = Some(PathBuf::from(args.get(i)?));
            }
            _ => return None,
        }
        i += 1;
    }
    match (install, delete, xml, data_dir) {
        (true, false, Some(xml), None) if !xml.as_os_str().is_empty() => {
            Some(LauncherCommand::InstallTask { xml })
        }
        (false, true, None, None) => Some(LauncherCommand::DeleteTask),
        (false, false, None, Some(data_dir)) if !data_dir.as_os_str().is_empty() => {
            Some(LauncherCommand::Run { data_dir })
        }
        _ => None,
    }
}

/// Render the task description stored in `RegistrationInfo/Description`.
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
    let close = format!("</{tag}>");
    let content_start = if let Some(s) = xml.find(&format!("<{tag}>")) {
        s + tag.len() + 2
    } else {
        let s = xml.find(&format!("<{tag} "))?;
        let rel = xml[s..].find('>')?;
        s + rel + 1
    };
    let rest = xml.get(content_start..)?;
    let end = rest.find(&close)?;
    Some(xml_unescape(rest[..end].trim()))
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn xml_unescape(value: &str) -> String {
    value
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// Exec/Command from `schtasks /Query /XML` (the elevated launcher path).
pub fn extract_tun_task_command_from_xml(xml: &str) -> Option<String> {
    xml_tag_value(xml, "Command")
}

/// Whether an Exec/Command value points at `launcher` (quotes and ASCII case
/// ignored so a Task Scheduler round-trip still matches).
pub fn command_matches_launcher(command: &str, launcher: &Path) -> bool {
    let trimmed = command.trim().trim_matches('"');
    Path::new(trimmed) == launcher || trimmed.eq_ignore_ascii_case(&launcher.display().to_string())
}

/// Task Scheduler 1.2 XML for `ice-box-tun`. `schtasks /Create /XML` is the
/// only supported way to persist [`format_tun_task_pin`] — `/D` is a day of
/// week, not a description. The time trigger is in the past so the task
/// never auto-starts; `AllowStartOnDemand` keeps `schtasks /Run` working.
/// `ExecutionTimeLimit` is unlimited so a long-lived TUN core is not killed
/// at the 72-hour default.
pub fn render_tun_task_xml(launcher: &Path, data_dir: &Path, pin: &str) -> String {
    let command = xml_escape(&launcher.display().to_string());
    let arguments = xml_escape(&format!("--data \"{}\"", data_dir.display()));
    let description = xml_escape(pin);
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>{description}</Description>
    <URI>\{TUN_TASK_NAME}</URI>
  </RegistrationInfo>
  <Triggers>
    <TimeTrigger>
      <StartBoundary>1999-01-01T00:00:00</StartBoundary>
      <Enabled>true</Enabled>
    </TimeTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>HighestAvailable</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>false</StartWhenAvailable>
    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>
    <IdleSettings>
      <StopOnIdleEnd>false</StopOnIdleEnd>
      <RestartOnIdle>false</RestartOnIdle>
    </IdleSettings>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>false</Hidden>
    <RunOnlyIfIdle>false</RunOnlyIfIdle>
    <WakeToRun>false</WakeToRun>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>7</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{command}</Command>
      <Arguments>{arguments}</Arguments>
    </Exec>
  </Actions>
</Task>
"#
    )
}

/// UTF-16 LE with BOM. `schtasks /Create /XML` requires a Unicode file.
pub fn encode_utf16_le_bom(text: &str) -> Vec<u8> {
    let mut bytes = vec![0xFF, 0xFE];
    for unit in text.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes
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
        let attributed =
            format!("<Task><Description xml:space=\"preserve\">{pin}</Description></Task>");
        assert_eq!(
            extract_tun_task_pin_from_xml(&attributed).expect("attributed description"),
            parse_tun_task_pin(&pin).expect("parsed")
        );
    }

    #[test]
    fn render_tun_task_xml_stores_pin_highest_privilege_and_on_demand_action() {
        let pin = format_tun_task_pin(LAUNCHER, CORE);
        let launcher = Path::new(r"C:\Program Files\ice-box\ice-tun-launcher.exe");
        let data = Path::new(r"C:\Users\O'Brien\AppData\Roaming\com.yilong-musk.icebox");
        let xml = render_tun_task_xml(launcher, data, &pin);
        assert!(xml.contains(&format!("<Description>{pin}</Description>")));
        assert!(xml.contains(r"<URI>\ice-box-tun</URI>"));
        assert!(xml.contains("<RunLevel>HighestAvailable</RunLevel>"));
        assert!(xml.contains("<AllowStartOnDemand>true</AllowStartOnDemand>"));
        assert!(xml.contains("<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>"));
        assert!(xml.contains(r"<Command>C:\Program Files\ice-box\ice-tun-launcher.exe</Command>"));
        assert!(xml.contains(
            r#"<Arguments>--data &quot;C:\Users\O&apos;Brien\AppData\Roaming\com.yilong-musk.icebox&quot;</Arguments>"#
        ));
        assert_eq!(
            extract_tun_task_pin_from_xml(&xml).expect("pin from rendered xml"),
            parse_tun_task_pin(&pin).expect("parsed")
        );
        assert!(command_matches_launcher(
            &extract_tun_task_command_from_xml(&xml).expect("command"),
            launcher
        ));
        assert!(!command_matches_launcher(
            r"C:\other\ice-tun-launcher.exe",
            launcher
        ));
    }

    #[test]
    fn render_tun_task_xml_escapes_ampersand_paths() {
        let pin = format_tun_task_pin(LAUNCHER, CORE);
        let xml = render_tun_task_xml(
            Path::new(r"C:\a&b\ice-tun-launcher.exe"),
            Path::new(r"C:\data"),
            &pin,
        );
        assert!(xml.contains(r"<Command>C:\a&amp;b\ice-tun-launcher.exe</Command>"));
        assert!(command_matches_launcher(
            &extract_tun_task_command_from_xml(&xml).expect("command"),
            Path::new(r"C:\a&b\ice-tun-launcher.exe"),
        ));
    }

    #[test]
    fn encode_utf16_le_bom_round_trips_through_schtasks_decoder() {
        let xml = render_tun_task_xml(
            Path::new(r"C:\ice-box\ice-tun-launcher.exe"),
            Path::new(r"C:\data"),
            &format_tun_task_pin(LAUNCHER, CORE),
        );
        let encoded = encode_utf16_le_bom(&xml);
        assert_eq!(&encoded[..2], [0xFF, 0xFE]);
        assert_eq!(decode_schtasks_output(&encoded), xml);
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

    #[test]
    fn parse_launcher_command_accepts_run_install_and_delete() {
        assert_eq!(
            parse_launcher_command(&["--data".into(), r"C:\data".into()]),
            Some(LauncherCommand::Run {
                data_dir: PathBuf::from(r"C:\data"),
            })
        );
        assert_eq!(
            parse_launcher_command(&[
                "--xml".into(),
                r"C:\data\ice-box-tun.xml".into(),
                "--install-task".into(),
            ]),
            Some(LauncherCommand::InstallTask {
                xml: PathBuf::from(r"C:\data\ice-box-tun.xml"),
            })
        );
        assert_eq!(
            parse_launcher_command(&["--delete-task".into()]),
            Some(LauncherCommand::DeleteTask)
        );
        assert!(parse_launcher_command(&["--install-task".into()]).is_none());
        assert!(
            parse_launcher_command(&["--install-task".into(), "--delete-task".into()]).is_none()
        );
        assert!(parse_launcher_command(&[
            "--data".into(),
            r"C:\data".into(),
            "--delete-task".into()
        ])
        .is_none());
        assert!(parse_launcher_command(&["--unknown".into()]).is_none());
    }
}
