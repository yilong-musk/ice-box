// SPDX-License-Identifier: GPL-3.0-or-later

//! Elevated launcher for the Windows TUN core (plan B: scheduled-task
//! elevation).
//!
//! The app runs unelevated; a scheduled task (created once, elevated) runs
//! this binary with the highest-privilege token. The launcher:
//!
//! 1. spawns the bundled sing-box with the runtime config, redirecting its
//!    output to the protected core log,
//! 2. writes the child pid to the handshake pid file (the app reads it),
//! 3. waits on a named stop event: when signaled (the app requests a
//!    graceful stop), it sends the graceful close first (`taskkill /T`
//!    without `/F`, which sing-box uses to remove its WFP filters and
//!    routes — the strict-route filters must not be stranded, they
//!    black-hole host TCP), then the forced `/F` fallback,
//! 4. removes the pid file and exits when the core is gone or stops itself.
//!
//! The scheduled task may also be ended hard (`schtasks /End`), which kills
//! the launcher tree without cleanup — the coordinator tolerates the stale
//! pid file and resets it on the next start.
//!
//! Usage:
//! - `ice-tun-launcher --data <app-data-dir>` — run the elevated core
//! - `ice-tun-launcher --install --data <app-data-dir> --user-sid <sid>` —
//!   one-time UAC: remove any leftover `ice-box-tun` task, copy binaries to
//!   `%ProgramFiles%\ice-box`, register the task (`ITaskService::RegisterTask`,
//!   with `schtasks /Create /XML` fallback from an admin-owned UTF-16 file)
//! - `ice-tun-launcher --delete-task` — remove the scheduled task and
//!   protected copies
//!
//! Before spawning sing-box the launcher checks the SHA-256 pin stored in
//! the `ice-box-tun` scheduled-task description (written at elevated create
//! time). A replaced `sing-box.exe` is refused.

#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

#[cfg(target_os = "windows")]
mod acl;
#[cfg(target_os = "windows")]
mod stop_event;
#[cfg(target_os = "windows")]
mod task_com;

#[cfg(target_os = "windows")]
use std::path::{Path, PathBuf};
#[cfg(target_os = "windows")]
use std::process::{Command, Stdio};
#[cfg(target_os = "windows")]
use std::time::{Duration, Instant};

#[cfg(target_os = "windows")]
const POLL_INTERVAL: Duration = Duration::from_millis(300);
#[cfg(target_os = "windows")]
const TERM_GRACE: Duration = Duration::from_secs(5);
#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
#[cfg(target_os = "windows")]
const LOG_CAP_EVERY: Duration = Duration::from_secs(60);

#[cfg(target_os = "windows")]
struct Args {
    binary: PathBuf,
    config: PathBuf,
    log: PathBuf,
    pidfile: PathBuf,
}

#[cfg(target_os = "windows")]
fn args_from_data_dir(data_dir: PathBuf) -> Option<Args> {
    if data_dir.as_os_str().is_empty() {
        return None;
    }
    let exe_dir = std::env::current_exe().ok()?.parent().map(PathBuf::from)?;
    let program_data = ice_tun_pin::program_data_dir();
    Some(Args {
        binary: exe_dir.join("sing-box.exe"),
        config: data_dir.join("config.json"),
        log: ice_tun_pin::protected_core_log_path(&program_data),
        pidfile: ice_tun_pin::protected_pidfile_path(&program_data),
    })
}

#[cfg(target_os = "windows")]
fn taskkill(pid: u32, forced: bool) {
    use std::os::windows::process::CommandExt;
    let mut command = Command::new("taskkill");
    command
        .args(["/PID", &pid.to_string(), "/T"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW);
    if forced {
        command.arg("/F");
    }
    let _ = command.status();
}

#[cfg(target_os = "windows")]
fn silent_schtasks(command: &mut Command) -> i32 {
    use std::os::windows::process::CommandExt;
    match command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .status()
    {
        Ok(status) => status.code().unwrap_or(1),
        Err(_) => 1,
    }
}

#[cfg(target_os = "windows")]
fn delete_task() -> i32 {
    let code = silent_schtasks(Command::new("schtasks.exe").args([
        "/Delete",
        "/TN",
        ice_tun_pin::TUN_TASK_NAME,
        "/F",
    ]));
    acl::remove_protected_tree(&ice_tun_pin::protected_bin_dir(
        &ice_tun_pin::program_files_dir(),
    ));
    acl::remove_protected_tree(&ice_tun_pin::protected_data_dir(
        &ice_tun_pin::program_data_dir(),
    ));
    code
}

#[cfg(target_os = "windows")]
fn write_install_error(detail: &str) {
    let path = ice_tun_pin::protected_install_error_path(&ice_tun_pin::program_data_dir());
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, format!("{detail}\n"));
    let _ = acl::apply_acl(&path, false, acl::UsersAccess::Read);
}

#[cfg(target_os = "windows")]
fn clear_install_error() {
    let path = ice_tun_pin::protected_install_error_path(&ice_tun_pin::program_data_dir());
    let _ = std::fs::remove_file(path);
}

/// Stop and delete a leftover `ice-box-tun` *before* protected copies are
/// replaced. Updating a Highest-privilege task whose `Command` exe was
/// already wiped (the ProgramData → Program Files migration) fails closed.
#[cfg(target_os = "windows")]
fn remove_leftover_task() {
    let _ = silent_schtasks(Command::new("schtasks.exe").args([
        "/End",
        "/TN",
        ice_tun_pin::TUN_TASK_NAME,
    ]));
    std::thread::sleep(Duration::from_millis(500));
    let _ = silent_schtasks(Command::new("schtasks.exe").args([
        "/Delete",
        "/TN",
        ice_tun_pin::TUN_TASK_NAME,
        "/F",
    ]));
    std::thread::sleep(Duration::from_millis(300));
}

/// Admin-owned UTF-16 LE + BOM file under the protected run dir, then
/// `schtasks /Create /XML /F`. The file is deleted after import so a
/// user-writable XML is never the registration source.
#[cfg(target_os = "windows")]
fn register_task_schtasks_xml(xml: &str, run_dir: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let xml_path = run_dir.join("ice-box-tun.xml");
    std::fs::write(&xml_path, ice_tun_pin::encode_utf16_le_bom(xml))
        .map_err(|err| format!("write {}: {err}", xml_path.display()))?;
    let _ = acl::apply_acl(&xml_path, false, acl::UsersAccess::None);
    let _ = acl::set_owner_administrators(&xml_path, false);
    let output = Command::new("schtasks.exe")
        .args(["/Create", "/TN", ice_tun_pin::TUN_TASK_NAME, "/XML"])
        .arg(&xml_path)
        .arg("/F")
        .stdin(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|err| format!("spawn schtasks /Create /XML: {err}"))?;
    let _ = std::fs::remove_file(&xml_path);
    if output.status.success() {
        Ok(())
    } else {
        let stdout = ice_tun_pin::decode_schtasks_output(&output.stdout);
        let stderr = ice_tun_pin::decode_schtasks_output(&output.stderr);
        Err(format!(
            "schtasks /Create /XML exited {}: {} {}",
            output.status.code().unwrap_or(-1),
            stdout.trim(),
            stderr.trim()
        ))
    }
}

#[cfg(target_os = "windows")]
fn register_ice_box_tun_task(xml: &str, run_dir: &Path) -> Result<(), String> {
    let com_xml = ice_tun_pin::task_xml_for_com_bstr(xml);
    match task_com::register_task_xml(&com_xml) {
        Ok(()) => Ok(()),
        Err(com_err) => {
            register_task_schtasks_xml(xml, run_dir).map_err(|sch| format!("{com_err}; {sch}"))
        }
    }
}

#[cfg(target_os = "windows")]
fn install_protected(data_dir: &Path, user_sid: &str) -> i32 {
    match install_protected_inner(data_dir, user_sid) {
        Ok(()) => {
            clear_install_error();
            0
        }
        Err(err) => {
            write_install_error(&err);
            eprintln!("{err}");
            2
        }
    }
}

#[cfg(target_os = "windows")]
fn install_protected_inner(data_dir: &Path, user_sid: &str) -> Result<(), String> {
    if data_dir.as_os_str().is_empty() || !ice_tun_pin::is_windows_sid(user_sid) {
        return Err("install requires --data <dir> and --user-sid <SID>".into());
    }
    let exe = std::env::current_exe().map_err(|err| format!("resolve own executable: {err}"))?;
    let src_dir = exe
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| format!("helper path {} has no parent", exe.display()))?;
    let core_src = src_dir.join("sing-box.exe");
    if !core_src.is_file() {
        return Err(format!("sing-box.exe not found next to {}", exe.display()));
    }

    remove_leftover_task();
    acl::remove_protected_tree(&ice_tun_pin::legacy_protected_bin_dir(
        &ice_tun_pin::program_data_dir(),
    ));

    let program_files = ice_tun_pin::program_files_dir();
    let program_data = ice_tun_pin::program_data_dir();
    let bin_dir = ice_tun_pin::protected_bin_dir(&program_files);
    let data_root = ice_tun_pin::protected_data_dir(&program_data);
    let run_dir = ice_tun_pin::protected_run_dir(&program_data);
    acl::wipe_and_create_dir(&bin_dir, acl::UsersAccess::ReadExecute)
        .map_err(|()| format!("create protected bin dir {}", bin_dir.display()))?;
    acl::wipe_and_create_dir(&data_root, acl::UsersAccess::Read)
        .map_err(|()| format!("create protected data dir {}", data_root.display()))?;
    acl::wipe_and_create_dir(&run_dir, acl::UsersAccess::Read)
        .map_err(|()| format!("create protected run dir {}", run_dir.display()))?;

    let dest_launcher = ice_tun_pin::protected_launcher_path(&program_files);
    let dest_core = bin_dir.join("sing-box.exe");
    acl::copy_protected_file(&exe, &dest_launcher, acl::UsersAccess::ReadExecute)
        .map_err(|()| format!("copy launcher to {}", dest_launcher.display()))?;
    acl::copy_protected_file(&core_src, &dest_core, acl::UsersAccess::ReadExecute)
        .map_err(|()| format!("copy core to {}", dest_core.display()))?;
    for name in ["libcronet.dll", "wintun.dll"] {
        let src = src_dir.join(name);
        if src.is_file() {
            let dest = bin_dir.join(name);
            acl::copy_protected_file(&src, &dest, acl::UsersAccess::ReadExecute)
                .map_err(|()| format!("copy {name} to {}", dest.display()))?;
        }
    }
    write_resources_pointer(&bin_dir, &src_dir)
        .map_err(|()| "write resources-dir.txt".to_string())?;
    let launcher_sha = ice_tun_pin::sha256_of_file(&dest_launcher)?;
    let core_sha = ice_tun_pin::sha256_of_file(&dest_core)?;
    let pin = ice_tun_pin::format_tun_task_pin(&launcher_sha, &core_sha);
    let xml = ice_tun_pin::render_tun_task_xml(&dest_launcher, data_dir, &pin, user_sid);
    register_ice_box_tun_task(&xml, &run_dir)
}

#[cfg(target_os = "windows")]
fn write_resources_pointer(bin_dir: &Path, resources: &Path) -> Result<(), ()> {
    let path = bin_dir.join("resources-dir.txt");
    std::fs::write(&path, resources.to_string_lossy().as_bytes()).map_err(|_| ())?;
    acl::apply_acl(&path, false, acl::UsersAccess::None)?;
    acl::set_owner_administrators(&path, false)
}

#[cfg(target_os = "windows")]
fn read_resources_pointer(program_files: &Path) -> Option<PathBuf> {
    let path = ice_tun_pin::protected_bin_dir(program_files).join("resources-dir.txt");
    let raw = std::fs::read_to_string(path).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(PathBuf::from(trimmed))
    }
}

#[cfg(target_os = "windows")]
fn run() -> i32 {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    match ice_tun_pin::parse_launcher_command(&argv) {
        Some(ice_tun_pin::LauncherCommand::Install { data_dir, user_sid }) => {
            install_protected(&data_dir, &user_sid)
        }
        Some(ice_tun_pin::LauncherCommand::DeleteTask) => delete_task(),
        Some(ice_tun_pin::LauncherCommand::Run { data_dir }) => {
            let Some(args) = args_from_data_dir(data_dir) else {
                return 2;
            };
            run_core(args)
        }
        None => {
            eprintln!(
                "usage: ice-tun-launcher --data <app-data-dir>\n       ice-tun-launcher --install --data <app-data-dir> --user-sid <sid>\n       ice-tun-launcher --delete-task"
            );
            2
        }
    }
}

#[cfg(target_os = "windows")]
fn cap_core_log(path: &std::path::Path) {
    const MAX_BYTES: u64 = 20 * 1024 * 1024;
    const KEEP: u32 = 3;
    let oversized = std::fs::metadata(path)
        .map(|m| m.len() > MAX_BYTES)
        .unwrap_or(false);
    if oversized {
        let _ = retain_log_tail(path, MAX_BYTES);
    }
    let rotated = |n: u32| {
        let mut name = path.file_name().unwrap_or_default().to_os_string();
        name.push(format!(".{n}"));
        match path.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => parent.join(name),
            _ => std::path::PathBuf::from(name),
        }
    };
    for i in 1..=KEEP {
        let _ = std::fs::remove_file(rotated(i));
    }
}

/// Keep in sync with `ice_core::trim_log_file`.
#[cfg(target_os = "windows")]
fn retain_log_tail(path: &std::path::Path, max_bytes: u64) -> std::io::Result<()> {
    use std::io::{Read, Seek, SeekFrom, Write};

    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)?;
    let len = file.metadata()?.len();
    let start = if len <= 1 {
        0
    } else {
        let drop = (max_bytes / 4).max(1);
        let over = len.saturating_sub(max_bytes);
        drop.max(over).min(len - 1)
    };
    let kept = if start == 0 || len <= 1 {
        file.seek(SeekFrom::Start(0))?;
        let mut all = Vec::new();
        file.read_to_end(&mut all)?;
        all
    } else {
        let at_line_start = {
            file.seek(SeekFrom::Start(start - 1))?;
            let mut prev = [0u8; 1];
            file.read_exact(&mut prev)?;
            prev[0] == b'\n'
        };
        file.seek(SeekFrom::Start(start))?;
        let mut tail = Vec::new();
        file.read_to_end(&mut tail)?;
        if !at_line_start {
            if let Some(i) = tail.iter().position(|&b| b == b'\n') {
                if i + 1 < tail.len() {
                    tail.drain(..=i);
                }
            }
        }
        tail
    };
    file.set_len(0)?;
    file.seek(SeekFrom::Start(0))?;
    file.write_all(&kept)?;
    file.flush()?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn run_core(args: Args) -> i32 {
    let program_files = ice_tun_pin::program_files_dir();
    let program_data = ice_tun_pin::program_data_dir();
    let Ok(exe) = std::env::current_exe() else {
        return 2;
    };
    if !ice_tun_pin::path_is_protected_launcher(&exe, &program_files) {
        eprintln!(
            "refusing to run {} (not the protected Program Files launcher)",
            exe.display()
        );
        return 2;
    }
    if !args.binary.is_file() {
        eprintln!("sing-box binary not found at {}", args.binary.display());
        return 2;
    }
    if let Err(err) = verify_pinned_core(&args.binary) {
        eprintln!("{err}");
        return 2;
    }
    let protected_config =
        match sanitize_user_config(&args.config, &args.log, &program_data, &program_files) {
            Ok(path) => path,
            Err(err) => {
                eprintln!("{err}");
                return 2;
            }
        };
    if let Some(parent) = args.log.parent() {
        if !parent.exists() {
            eprintln!("protected log dir missing {}", parent.display());
            return 2;
        }
    }
    cap_core_log(&args.log);

    let stop = match stop_event::StopEvent::create() {
        Ok(stop) => stop,
        Err(_) => {
            eprintln!("cannot create the TUN stop event");
            return 2;
        }
    };

    use std::os::windows::process::CommandExt;

    let log = match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&args.log)
    {
        Ok(log) => log,
        Err(err) => {
            eprintln!("open core log {}: {err}", args.log.display());
            return 2;
        }
    };
    let _ = acl::apply_acl(&args.log, false, acl::UsersAccess::Read);
    let log_err = match log.try_clone() {
        Ok(clone) => clone,
        Err(err) => {
            eprintln!("clone core log handle: {err}");
            return 2;
        }
    };
    let mut child = match Command::new(&args.binary)
        .arg("run")
        .arg("-c")
        .arg(&protected_config)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err))
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
    {
        Ok(child) => child,
        Err(err) => {
            eprintln!(
                "spawn {} run -c {}: {err}",
                args.binary.display(),
                protected_config.display()
            );
            return 2;
        }
    };
    let pid = child.id();
    if let Err(err) = std::fs::write(&args.pidfile, pid.to_string()) {
        eprintln!("write pid file {}: {err}", args.pidfile.display());
        let _ = child.kill();
        return 2;
    }
    let _ = acl::apply_acl(&args.pidfile, false, acl::UsersAccess::Read);

    let mut last_cap = Instant::now();
    loop {
        if stop.wait_signaled(POLL_INTERVAL.as_millis() as u32) {
            graceful_stop(pid);
            let deadline = Instant::now() + TERM_GRACE;
            while Instant::now() < deadline {
                if child.try_wait().ok().flatten().is_some() {
                    break;
                }
                std::thread::sleep(POLL_INTERVAL);
            }
            if child.try_wait().ok().flatten().is_none() {
                taskkill(pid, true);
            }
            break;
        }
        if child.try_wait().ok().flatten().is_some() {
            break;
        }
        if last_cap.elapsed() >= LOG_CAP_EVERY {
            cap_core_log(&args.log);
            last_cap = Instant::now();
        }
    }
    let _ = child.wait();
    let _ = std::fs::remove_file(&args.pidfile);
    0
}

/// Read the user-writable config, sanitise it, write to ProgramData.
#[cfg(target_os = "windows")]
fn sanitize_user_config(
    user_config: &Path,
    core_log: &Path,
    program_data: &Path,
    program_files: &Path,
) -> Result<PathBuf, String> {
    if !user_config.is_file() {
        return Err(format!("config not found at {}", user_config.display()));
    }
    let raw = std::fs::read(user_config)
        .map_err(|err| format!("read {}: {err}", user_config.display()))?;
    if raw.len() > ice_config_guard::MAX_CONFIG_BYTES {
        return Err(format!(
            "config exceeds {} bytes",
            ice_config_guard::MAX_CONFIG_BYTES
        ));
    }
    let mut cfg: serde_json::Value =
        serde_json::from_slice(&raw).map_err(|err| format!("config is not JSON: {err}"))?;
    let data_dir = user_config
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let resources_dir = read_resources_pointer(program_files).unwrap_or_else(|| {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| data_dir.clone())
    });
    let run_dir = ice_tun_pin::protected_run_dir(program_data);
    if !run_dir.is_dir() {
        return Err(format!("protected run dir missing {}", run_dir.display()));
    }
    let dest = run_dir.join("config.json");
    let ctx = ice_config_guard::GuardContext {
        data_dir,
        resources_dir,
        log_output: Some(core_log.to_path_buf()),
        cache_file_path: Some(run_dir.join("cache.db")),
    };
    ice_config_guard::sanitize_for_elevated_core(&mut cfg, &ctx).map_err(|err| err.to_string())?;
    let bytes = serde_json::to_vec(&cfg).map_err(|err| format!("encode config: {err}"))?;
    std::fs::write(&dest, bytes).map_err(|err| format!("write {}: {err}", dest.display()))?;
    let _ = acl::apply_acl(&dest, false, acl::UsersAccess::None);
    Ok(dest)
}

/// Refuse to spawn a replaced `sing-box.exe`: the expected hash lives in the
/// elevated scheduled task, which an unelevated process cannot rewrite.
#[cfg(target_os = "windows")]
fn verify_pinned_core(core: &std::path::Path) -> Result<(), String> {
    let xml = query_task_xml()?;
    let pin = ice_tun_pin::extract_tun_task_pin_from_xml(&xml).ok_or_else(|| {
        "TUN scheduled task is missing the binary pin; refusing to start".to_string()
    })?;
    let exe = std::env::current_exe().map_err(|err| format!("current exe: {err}"))?;
    if !ice_tun_pin::pin_matches_files(&pin, &exe, core)? {
        return Err(format!(
            "launcher or {} does not match the scheduled-task sha256 pin; refusing to start",
            core.display()
        ));
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn query_task_xml() -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    let output = Command::new("schtasks")
        .args(["/Query", "/TN", ice_tun_pin::TUN_TASK_NAME, "/XML"])
        .stdin(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|err| format!("query TUN scheduled task: {err}"))?;
    if !output.status.success() {
        return Err(format!(
            "query TUN scheduled task failed (exit {})",
            output.status.code().unwrap_or(-1)
        ));
    }
    Ok(ice_tun_pin::decode_schtasks_output(&output.stdout))
}

#[cfg(target_os = "windows")]
fn graceful_stop(pid: u32) {
    // taskkill without `/F` delivers WM_CLOSE, which the console sing-box
    // treats as a shutdown signal and uses to remove its WFP filters and
    // routes (`docs/tun.md`). A failure here is benign: the
    // forced `/F` fallback decides.
    taskkill(pid, false);
}

#[cfg(not(target_os = "windows"))]
fn run() -> i32 {
    eprintln!("ice-tun-launcher is Windows-only");
    2
}

fn main() {
    std::process::exit(run());
}
