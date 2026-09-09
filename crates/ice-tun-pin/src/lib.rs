// SPDX-License-Identifier: GPL-3.0-or-later

//! Host-free helpers for the Windows TUN scheduled-task binary pin.
//!
//! Shared by `ice-tun-sys` and the `ice-tun-launcher` binary so the library
//! crate does not depend on a binary crate.
//!
//! The per-user install directory is writable, so `ice-tun-launcher.exe` and
//! `sing-box.exe` can be replaced by the same account. Protected copies live
//! under `%ProgramFiles%\ice-box` (standard users cannot pre-create that
//! tree). The scheduled task is created elevated from an in-memory XML
//! string (`ITaskService::RegisterTask`) so the SHA-256 pin lives in
//! `RegistrationInfo/Description` (`schtasks /D` is a day-of-week flag and
//! cannot store a description). `schtasks /Run` and this launcher refuse to
//! start when the on-disk files do not match.
//!
//! Some Windows 11 builds reject an unsigned `ice-tun-launcher.exe` as the
//! task `Exec/Command` (`0x80004005`). The installer then registers a
//! Microsoft-signed GUI host (`wscript.exe`) that waits on an admin-owned
//! `.vbs` next to the launcher; PowerShell and `cmd.exe` are last-resort
//! hosts. App-side verify accepts those wrappers only when Arguments still
//! pin the protected launcher (or its sibling run script) and `--data`.

use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// Fixed name of the scheduled task that runs this launcher elevated.
pub const TUN_TASK_NAME: &str = "ice-box-tun";

/// Prefix of the scheduled-task description that carries the binary pin.
pub const TUN_TASK_PIN_PREFIX: &str = "ice-box-pin:";

/// Admin-owned VBScript that `wscript.exe` runs when Task Scheduler refuses
/// an unsigned `ice-tun-launcher.exe` as `Exec/Command`.
pub const TUN_RUN_SCRIPT_NAME: &str = "ice-tun-run.vbs";

/// Named event the unelevated app signals to request a graceful TUN stop.
/// Created by the elevated launcher in the Global namespace with a DACL that
/// grants the interactive user `EVENT_MODIFY_STATE` and a Medium integrity
/// label so a medium-IL client can set it.
pub const TUN_STOP_EVENT_NAME: &str = r"Global\ice-box-tun-stop";

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
    /// Copy binaries to `%ProgramFiles%\ice-box`, render the task XML in
    /// memory, and register it (`--install --data <dir> --user-sid <sid>`).
    Install { data_dir: PathBuf, user_sid: String },
    /// Delete the `ice-box-tun` scheduled task (`--delete-task`).
    DeleteTask,
}

/// Parse launcher argv (without argv0). Used by the binary and host-free tests.
pub fn parse_launcher_command(args: &[String]) -> Option<LauncherCommand> {
    let mut data_dir = None;
    let mut user_sid = None;
    let mut install = false;
    let mut delete = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--install" => install = true,
            "--delete-task" => delete = true,
            "--data" | "--data-dir" => {
                i += 1;
                data_dir = Some(PathBuf::from(args.get(i)?));
            }
            "--user-sid" => {
                i += 1;
                user_sid = Some(args.get(i)?.clone());
            }
            _ => return None,
        }
        i += 1;
    }
    match (install, delete, user_sid, data_dir) {
        (true, false, Some(user_sid), Some(data_dir))
            if !data_dir.as_os_str().is_empty() && is_windows_sid(&user_sid) =>
        {
            Some(LauncherCommand::Install { data_dir, user_sid })
        }
        (false, true, None, None) => Some(LauncherCommand::DeleteTask),
        (false, false, None, Some(data_dir)) if !data_dir.as_os_str().is_empty() => {
            Some(LauncherCommand::Run { data_dir })
        }
        _ => None,
    }
}

/// `S-1-5-…` security identifier (revision, authority, at least one sub-authority).
pub fn is_windows_sid(value: &str) -> bool {
    let rest = match value.strip_prefix("S-") {
        Some(rest) => rest,
        None => return false,
    };
    let mut n = 0usize;
    for part in rest.split('-') {
        if part.is_empty() || !part.chars().all(|c| c.is_ascii_digit()) {
            return false;
        }
        n += 1;
    }
    n >= 3
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

/// Exec/Arguments from `schtasks /Query /XML`.
pub fn extract_tun_task_args_from_xml(xml: &str) -> Option<String> {
    xml_tag_value(xml, "Arguments")
}

/// Principal/UserId from `schtasks /Query /XML`.
pub fn extract_tun_task_user_id_from_xml(xml: &str) -> Option<String> {
    xml_tag_value(xml, "UserId")
}

/// Parse `--data` / `--data-dir` from a scheduled-task Arguments string.
pub fn parse_data_dir_from_task_args(args: &str) -> Option<PathBuf> {
    let parts = split_windows_cmd_args(args);
    let mut i = 0;
    while i < parts.len() {
        if parts[i] == "--data" || parts[i] == "--data-dir" {
            let value = parts.get(i + 1)?.as_str();
            if value.is_empty() {
                return None;
            }
            return Some(PathBuf::from(value));
        }
        i += 1;
    }
    None
}

fn split_windows_cmd_args(args: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quote = false;
    for ch in args.chars() {
        match ch {
            '"' => in_quote = !in_quote,
            c if c.is_whitespace() && !in_quote => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Whether an Exec/Command value points at `launcher` (quotes and ASCII case
/// ignored so a Task Scheduler round-trip still matches).
pub fn command_matches_launcher(command: &str, launcher: &Path) -> bool {
    let trimmed = command.trim().trim_matches('"');
    Path::new(trimmed) == launcher || trimmed.eq_ignore_ascii_case(&launcher.display().to_string())
}

/// `%ProgramFiles%`, or `C:\Program Files` when the env var is unset.
pub fn program_files_dir() -> PathBuf {
    std::env::var_os("ProgramFiles")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Program Files"))
}

/// `%ProgramData%`, or `C:\ProgramData` when the env var is unset.
pub fn program_data_dir() -> PathBuf {
    std::env::var_os("ProgramData")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"))
}

/// Protected binaries (`ice-tun-launcher.exe`, `sing-box.exe`, DLLs).
pub fn protected_bin_dir(program_files: &Path) -> PathBuf {
    program_files.join("ice-box")
}

/// Admin-owned runtime tree (`run\config.json`, logs, pid).
pub fn protected_data_dir(program_data: &Path) -> PathBuf {
    program_data.join("ice-box")
}

pub fn protected_run_dir(program_data: &Path) -> PathBuf {
    protected_data_dir(program_data).join("run")
}

pub fn protected_launcher_path(program_files: &Path) -> PathBuf {
    protected_bin_dir(program_files).join("ice-tun-launcher.exe")
}

/// Sibling of the protected launcher; executed by [`wscript_exe`].
pub fn protected_run_script_path(program_files: &Path) -> PathBuf {
    protected_bin_dir(program_files).join(TUN_RUN_SCRIPT_NAME)
}

/// `%SystemRoot%\System32`, or `C:\Windows\System32` when the env var is unset.
pub fn windows_system32_dir() -> PathBuf {
    std::env::var_os("SystemRoot")
        .or_else(|| std::env::var_os("WINDIR"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
        .join("System32")
}

/// GUI-subsystem Microsoft host; no console flash.
pub fn wscript_exe() -> PathBuf {
    windows_system32_dir().join("wscript.exe")
}

pub fn cmd_exe() -> PathBuf {
    windows_system32_dir().join("cmd.exe")
}

pub fn powershell_exe() -> PathBuf {
    windows_system32_dir()
        .join("WindowsPowerShell")
        .join("v1.0")
        .join("powershell.exe")
}

/// VBScript that starts the protected launcher and waits (window style 0).
pub fn render_tun_run_vbs(launcher: &Path) -> String {
    let launcher_lit = vbs_string_literal(&launcher.display().to_string());
    format!(
        r#"Option Explicit
Dim launcher, dataDir, i, sh, cmd
launcher = {launcher_lit}
dataDir = ""
For i = 0 To WScript.Arguments.Count - 1
  If StrComp(WScript.Arguments(i), "--data", 1) = 0 Or StrComp(WScript.Arguments(i), "--data-dir", 1) = 0 Then
    If i + 1 <= WScript.Arguments.Count - 1 Then
      dataDir = WScript.Arguments(i + 1)
    End If
  End If
Next
If Len(dataDir) = 0 Then
  WScript.Quit 2
End If
dataDir = Replace(dataDir, Chr(34), Chr(34) & Chr(34))
Set sh = CreateObject("WScript.Shell")
cmd = Chr(34) & launcher & Chr(34) & " --data " & Chr(34) & dataDir & Chr(34)
WScript.Quit sh.Run(cmd, 0, True)
"#
    )
}

fn vbs_string_literal(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

/// `wscript.exe //B "<script>" --data "<dir>"`.
pub fn wscript_task_arguments(script: &Path, data_dir: &Path) -> String {
    format!(
        "//B \"{}\" --data \"{}\"",
        script.display(),
        data_dir.display()
    )
}

/// Hidden PowerShell that waits on the protected launcher. Trailing `--data`
/// is for [`parse_data_dir_from_task_args`]; `Start-Process` also gets it.
pub fn powershell_task_arguments(launcher: &Path, data_dir: &Path) -> String {
    let launch = launcher.display().to_string().replace('\'', "''");
    let data = data_dir.display().to_string().replace('\'', "''");
    format!(
        "-NoProfile -NonInteractive -WindowStyle Hidden -ExecutionPolicy Bypass -Command \"Start-Process -FilePath '{launch}' -ArgumentList '--data','{data}' -Wait -WindowStyle Hidden\" --data \"{}\"",
        data_dir.display()
    )
}

/// Last-resort console host (may flash). `start /b /wait` keeps the task
/// process alive for the TUN core lifetime.
pub fn cmd_task_arguments(launcher: &Path, data_dir: &Path) -> String {
    format!(
        "/c start /b /wait \"\" \"{}\" --data \"{}\"",
        launcher.display(),
        data_dir.display()
    )
}

fn task_args_contain_path(args: &str, path: &Path) -> bool {
    let needle = path.display().to_string().replace('/', "\\");
    if needle.is_empty() {
        return false;
    }
    let haystack = args.replace('/', "\\");
    haystack
        .to_ascii_lowercase()
        .contains(&needle.to_ascii_lowercase())
}

fn require_task_data_dir(args: &str) -> Result<(), String> {
    if parse_data_dir_from_task_args(args).is_none() {
        Err("scheduled-task Arguments are missing --data; re-run elevation setup".into())
    } else {
        Ok(())
    }
}

pub fn protected_core_log_path(program_data: &Path) -> PathBuf {
    protected_run_dir(program_data).join("sing-box.log")
}

pub fn protected_pidfile_path(program_data: &Path) -> PathBuf {
    protected_run_dir(program_data).join("tun-task.pid")
}

/// Pre-ProgramFiles copies (`%ProgramData%\ice-box\bin`). Left behind across
/// the path migration; the elevated installer deletes this tree after the
/// leftover `ice-box-tun` task has been removed.
pub fn legacy_protected_bin_dir(program_data: &Path) -> PathBuf {
    protected_data_dir(program_data).join("bin")
}

/// Written by the elevated installer on failure so the unelevated app can
/// surface a COM / `schtasks` detail (the GUI-subsystem launcher has no
/// console). Users-read; overwritten on the next install attempt.
pub fn protected_install_error_path(program_data: &Path) -> PathBuf {
    protected_run_dir(program_data).join("last-install-error.txt")
}

/// Read [`protected_install_error_path`]. `None` when missing or empty.
pub fn read_last_tun_install_error(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// `ITaskService::RegisterTask` takes a BSTR already in UTF-16. Declaring
/// `encoding="UTF-16"` in the prologue (correct for a BOM file handed to
/// `schtasks /Create /XML`) has been observed to fail XML validation on the
/// COM path. Strip the encoding so the parser treats the BSTR as native
/// Unicode.
pub fn task_xml_for_com_bstr(xml: &str) -> String {
    xml.replacen(" encoding=\"UTF-16\"", "", 1)
}

pub fn path_is_protected_launcher(exe: &Path, program_files: &Path) -> bool {
    command_matches_launcher(
        &exe.display().to_string(),
        &protected_launcher_path(program_files),
    )
}

/// `Command` must be the protected launcher, or a Microsoft-signed host
/// whose Arguments still pin that launcher (Windows 11 Task Scheduler
/// rejects some unsigned Exec images with `0x80004005`).
pub fn verify_task_command(xml: &str, expected_launcher: &Path) -> Result<(), String> {
    let command = extract_tun_task_command_from_xml(xml)
        .ok_or_else(|| "scheduled-task XML is missing Exec/Command".to_string())?;
    if command_matches_launcher(&command, expected_launcher) {
        return Ok(());
    }
    let args = extract_tun_task_args_from_xml(xml).unwrap_or_default();
    if command_matches_launcher(&command, &wscript_exe()) {
        let script = expected_launcher
            .parent()
            .ok_or_else(|| "protected launcher path has no parent".to_string())?
            .join(TUN_RUN_SCRIPT_NAME);
        if !task_args_contain_path(&args, &script) {
            return Err(
                "scheduled-task wscript Arguments do not reference the protected run script; re-run elevation setup"
                    .into(),
            );
        }
        return require_task_data_dir(&args);
    }
    if command_matches_launcher(&command, &powershell_exe())
        || command_matches_launcher(&command, &cmd_exe())
    {
        if !task_args_contain_path(&args, expected_launcher) {
            return Err(
                "scheduled-task Arguments do not reference the protected launcher; re-run elevation setup"
                    .into(),
            );
        }
        return require_task_data_dir(&args);
    }
    Err(
        "scheduled-task Command does not match the protected launcher; re-run elevation setup"
            .into(),
    )
}

/// `UserId` must be the interactive user's SID (the unelevated app, not an
/// over-the-shoulder administrator typed at UAC).
pub fn verify_task_user_id(xml: &str, expected_sid: &str) -> Result<(), String> {
    let user_id = extract_tun_task_user_id_from_xml(xml)
        .ok_or_else(|| "scheduled-task XML is missing Principal/UserId".to_string())?;
    if !user_id.eq_ignore_ascii_case(expected_sid) {
        return Err(
            "scheduled-task UserId does not match the interactive user; re-run elevation setup"
                .into(),
        );
    }
    Ok(())
}

/// Compare the task's `--data` directory with `config_path`'s parent.
/// Mismatch means the task would start a different config than the app wrote.
pub fn task_config_path_matches(xml: &str, config_path: &Path) -> Result<(), String> {
    let args = extract_tun_task_args_from_xml(xml)
        .ok_or_else(|| "scheduled-task XML is missing Exec/Arguments".to_string())?;
    let data_dir = parse_data_dir_from_task_args(&args)
        .ok_or_else(|| "scheduled-task Arguments are missing --data".to_string())?;
    let pinned = data_dir.join("config.json");
    if !paths_refer_to_same_file(&pinned, config_path) {
        return Err("task pinned to a different config path; re-run ensure_tun_elevation".into());
    }
    Ok(())
}

fn paths_refer_to_same_file(a: &Path, b: &Path) -> bool {
    if let (Ok(left), Ok(right)) = (a.canonicalize(), b.canonicalize()) {
        return left == right;
    }
    let left = a.to_string_lossy().replace('/', "\\");
    let right = b.to_string_lossy().replace('/', "\\");
    left.eq_ignore_ascii_case(&right)
}

/// Task Scheduler 1.2 XML for `ice-box-tun` with `Command` = the protected
/// launcher. See [`render_tun_task_xml_exec`] for signed-host wrappers.
pub fn render_tun_task_xml(launcher: &Path, data_dir: &Path, pin: &str, user_sid: &str) -> String {
    render_tun_task_xml_exec(
        launcher,
        &format!("--data \"{}\"", data_dir.display()),
        pin,
        user_sid,
    )
}

/// Task Scheduler 1.2 XML for `ice-box-tun`. `ITaskService::RegisterTask`
/// takes this string in memory — no on-disk XML. Privilege, the on-demand
/// action, and the SHA-256 pin live here. `schtasks /D` is a day of week,
/// not a description. The time trigger is in the past so the task never
/// auto-starts; `AllowStartOnDemand` keeps `schtasks /Run` working.
/// `ExecutionTimeLimit` is unlimited so a long-lived TUN core is not killed
/// at the 72-hour default. `UserId` is the interactive user's SID so an
/// over-the-shoulder UAC admin cannot silently own the task.
pub fn render_tun_task_xml_exec(
    command: &Path,
    arguments: &str,
    pin: &str,
    user_sid: &str,
) -> String {
    let command = xml_escape(&command.display().to_string());
    let arguments = xml_escape(arguments);
    let description = xml_escape(pin);
    let user_id = xml_escape(user_sid);
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
      <UserId>{user_id}</UserId>
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
/// Hashed in a streaming loop so a tens-of-MB `sing-box.exe` is never fully
/// buffered.
pub fn sha256_of_file(path: &Path) -> Result<String, String> {
    let mut file =
        std::fs::File::open(path).map_err(|err| format!("read {}: {err}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|err| format!("read {}: {err}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
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
    const USER_SID: &str = "S-1-5-21-1-2-3-1001";

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
        let xml = render_tun_task_xml(launcher, data, &pin, USER_SID);
        assert!(xml.contains(&format!("<Description>{pin}</Description>")));
        assert!(xml.contains(r"<URI>\ice-box-tun</URI>"));
        assert!(xml.contains(&format!("<UserId>{USER_SID}</UserId>")));
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
            USER_SID,
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
            USER_SID,
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
            parse_launcher_command(&["--delete-task".into()]),
            Some(LauncherCommand::DeleteTask)
        );
        assert_eq!(
            parse_launcher_command(&[
                "--install".into(),
                "--data".into(),
                r"C:\data".into(),
                "--user-sid".into(),
                USER_SID.into(),
            ]),
            Some(LauncherCommand::Install {
                data_dir: PathBuf::from(r"C:\data"),
                user_sid: USER_SID.to_string(),
            })
        );
        assert_eq!(
            parse_launcher_command(&[
                "--install".into(),
                "--data-dir".into(),
                r"C:\data".into(),
                "--user-sid".into(),
                USER_SID.into(),
            ]),
            Some(LauncherCommand::Install {
                data_dir: PathBuf::from(r"C:\data"),
                user_sid: USER_SID.to_string(),
            })
        );
        assert!(
            parse_launcher_command(&["--install".into(), "--data".into(), r"C:\data".into()])
                .is_none()
        );
        assert!(parse_launcher_command(&[
            "--install".into(),
            "--data".into(),
            r"C:\data".into(),
            "--user-sid".into(),
            "not-a-sid".into(),
        ])
        .is_none());
        assert!(parse_launcher_command(&["--install-task".into()]).is_none());
        assert!(parse_launcher_command(&[
            "--data".into(),
            r"C:\data".into(),
            "--delete-task".into()
        ])
        .is_none());
        assert!(parse_launcher_command(&["--unknown".into()]).is_none());
    }

    #[test]
    fn is_windows_sid_accepts_user_and_well_known() {
        assert!(is_windows_sid(USER_SID));
        assert!(is_windows_sid("S-1-5-18"));
        assert!(is_windows_sid("S-1-5-32-544"));
        assert!(!is_windows_sid(""));
        assert!(!is_windows_sid("S-1-5"));
        assert!(!is_windows_sid("S-1-5-21-abc"));
        assert!(!is_windows_sid("not-a-sid"));
    }

    #[test]
    fn extract_args_and_verify_command_from_rendered_xml() {
        let launcher = Path::new(r"C:\Program Files\ice-box\ice-tun-launcher.exe");
        let data_dir = Path::new(r"C:\Users\admin\AppData\Roaming\com.yilong-musk.icebox");
        let xml = render_tun_task_xml(launcher, data_dir, "ice-box-pin:aa:bb", USER_SID);
        assert!(verify_task_command(&xml, launcher).is_ok());
        verify_task_user_id(&xml, USER_SID).expect("user id");
        let other = Path::new(r"C:\Users\admin\ice-tun-launcher.exe");
        let err = verify_task_command(&xml, other).expect_err("command mismatch");
        assert!(err.contains("Command"), "{err}");
        let sid_err = verify_task_user_id(&xml, "S-1-5-21-9-9-9-9").expect_err("sid mismatch");
        assert!(sid_err.contains("UserId"), "{sid_err}");
        assert_eq!(
            parse_data_dir_from_task_args(&extract_tun_task_args_from_xml(&xml).unwrap())
                .as_deref(),
            Some(data_dir)
        );
        task_config_path_matches(&xml, &data_dir.join("config.json")).expect("match");
        let mismatch = task_config_path_matches(&xml, Path::new(r"D:\other\config.json"))
            .expect_err("mismatch");
        assert!(mismatch.contains("different config path"), "{mismatch}");
    }

    #[test]
    fn protected_launcher_path_joins_programfiles() {
        let pf = Path::new("/programfiles");
        assert_eq!(
            protected_launcher_path(pf),
            PathBuf::from("/programfiles/ice-box/ice-tun-launcher.exe")
        );
        assert!(path_is_protected_launcher(&protected_launcher_path(pf), pf));
        let pd = Path::new("/programdata");
        assert_eq!(
            protected_core_log_path(pd),
            PathBuf::from("/programdata/ice-box/run/sing-box.log")
        );
        assert_eq!(
            protected_pidfile_path(pd),
            PathBuf::from("/programdata/ice-box/run/tun-task.pid")
        );
        assert_eq!(
            legacy_protected_bin_dir(pd),
            PathBuf::from("/programdata/ice-box/bin")
        );
        assert_eq!(
            protected_install_error_path(pd),
            PathBuf::from("/programdata/ice-box/run/last-install-error.txt")
        );
        assert_eq!(
            protected_run_script_path(pf),
            PathBuf::from("/programfiles/ice-box/ice-tun-run.vbs")
        );
    }

    #[test]
    fn verify_task_command_accepts_signed_host_wrappers() {
        let launcher = Path::new(r"C:\Program Files\ice-box\ice-tun-launcher.exe");
        let data_dir = Path::new(r"C:\Users\admin\AppData\Roaming\com.yilong-musk.icebox");
        let pin = "ice-box-pin:aa:bb";
        let script = Path::new(r"C:\Program Files\ice-box\ice-tun-run.vbs");

        let wscript_xml = render_tun_task_xml_exec(
            &wscript_exe(),
            &wscript_task_arguments(script, data_dir),
            pin,
            USER_SID,
        );
        verify_task_command(&wscript_xml, launcher).expect("wscript wrapper");
        task_config_path_matches(&wscript_xml, &data_dir.join("config.json"))
            .expect("wscript data");

        let wrong_script = render_tun_task_xml_exec(
            &wscript_exe(),
            &wscript_task_arguments(Path::new(r"C:\Temp\evil.vbs"), data_dir),
            pin,
            USER_SID,
        );
        let err = verify_task_command(&wrong_script, launcher).expect_err("wrong vbs");
        assert!(err.contains("run script"), "{err}");

        let ps_xml = render_tun_task_xml_exec(
            &powershell_exe(),
            &powershell_task_arguments(launcher, data_dir),
            pin,
            USER_SID,
        );
        verify_task_command(&ps_xml, launcher).expect("powershell wrapper");
        task_config_path_matches(&ps_xml, &data_dir.join("config.json")).expect("powershell data");

        let cmd_xml = render_tun_task_xml_exec(
            &cmd_exe(),
            &cmd_task_arguments(launcher, data_dir),
            pin,
            USER_SID,
        );
        verify_task_command(&cmd_xml, launcher).expect("cmd wrapper");

        let notepad = render_tun_task_xml_exec(
            Path::new(r"C:\Windows\System32\notepad.exe"),
            &format!("--data \"{}\"", data_dir.display()),
            pin,
            USER_SID,
        );
        let err = verify_task_command(&notepad, launcher).expect_err("unsigned host rejected");
        assert!(err.contains("Command"), "{err}");

        let cmd_without_launcher = render_tun_task_xml_exec(
            &cmd_exe(),
            &format!("/c echo hi --data \"{}\"", data_dir.display()),
            pin,
            USER_SID,
        );
        let err =
            verify_task_command(&cmd_without_launcher, launcher).expect_err("cmd missing launcher");
        assert!(err.contains("protected launcher"), "{err}");
    }

    #[test]
    fn render_tun_run_vbs_embeds_launcher_and_escapes_quotes() {
        let vbs = render_tun_run_vbs(Path::new(r"C:\Program Files\ice-box\ice-tun-launcher.exe"));
        assert!(vbs.contains(r#"launcher = "C:\Program Files\ice-box\ice-tun-launcher.exe""#));
        assert!(vbs.contains("WScript.Shell"));
        assert!(vbs.contains("sh.Run(cmd, 0, True)"));
        let quoted = render_tun_run_vbs(Path::new(r#"C:\ice"box\ice-tun-launcher.exe"#));
        assert!(quoted.contains("ice\"\"box"));
    }

    #[test]
    fn task_xml_for_com_bstr_strips_utf16_encoding_declaration() {
        let pin = format_tun_task_pin(LAUNCHER, CORE);
        let xml = render_tun_task_xml(
            Path::new(r"C:\Program Files\ice-box\ice-tun-launcher.exe"),
            Path::new(r"C:\data"),
            &pin,
            USER_SID,
        );
        assert!(xml.contains("encoding=\"UTF-16\""));
        let com = task_xml_for_com_bstr(&xml);
        assert!(
            !com.contains("encoding="),
            "COM BSTR must not declare UTF-16: {com}"
        );
        assert!(com.contains("<?xml version=\"1.0\"?>"));
        assert_eq!(com, task_xml_for_com_bstr(&com), "idempotent");
    }

    #[test]
    fn read_last_tun_install_error_skips_missing_and_blank() {
        let dir = std::env::temp_dir().join(format!(
            "ice-tun-pin-install-err-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let path = dir.join("last-install-error.txt");
        assert!(read_last_tun_install_error(&path).is_none());
        std::fs::write(&path, "   \n").expect("blank");
        assert!(read_last_tun_install_error(&path).is_none());
        std::fs::write(&path, "ITaskService::RegisterTask failed: 0x80041318\n").expect("write");
        assert_eq!(
            read_last_tun_install_error(&path).as_deref(),
            Some("ITaskService::RegisterTask failed: 0x80041318")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
