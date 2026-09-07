// SPDX-License-Identifier: GPL-3.0-or-later

//! Elevated launcher for the Windows TUN core (plan B: scheduled-task
//! elevation).
//!
//! The app runs unelevated; a scheduled task (created once, elevated) runs
//! this binary with the highest-privilege token. The launcher:
//!
//! 1. spawns the bundled sing-box with the runtime config, redirecting its
//!    output to the core log,
//! 2. writes the child pid to the handshake pid file (the app reads it),
//! 3. polls the stop file: when it appears (the app requests a graceful
//!    stop), it sends the graceful close first (`taskkill /T` without `/F`,
//!    which sing-box uses to remove its WFP filters and routes — the
//!    strict-route filters must not be stranded, they black-hole host TCP),
//!    then the forced `/F` fallback,
//! 4. removes the pid file and exits when the core is gone or stops itself.
//!
//! The scheduled task may also be ended hard (`schtasks /End`), which kills
//! the launcher tree without cleanup — the coordinator tolerates the stale
//! pid file and resets it on the next start.
//!
//! Usage:
//! - `ice-tun-launcher --data <app-data-dir>` — run the elevated core
//! - `ice-tun-launcher --install --data <app-data-dir>` — one-time UAC:
//!   copy binaries to `%ProgramData%\ice-box\bin`, render and import the task
//! - `ice-tun-launcher --install-task --xml <task.xml>` — legacy XML import
//! - `ice-tun-launcher --delete-task` — remove the scheduled task and
//!   protected copies
//!
//! Before spawning sing-box the launcher checks the SHA-256 pin stored in
//! the `ice-box-tun` scheduled-task description (written via XML import at
//! elevated create time). A replaced `sing-box.exe` is refused.

// A GUI-subsystem binary: the scheduled task starts it elevated, and a
// console subsystem would flash a black window on every `schtasks /Run`
// and on the one-time UAC install (which used to wrap `cmd.exe`).
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

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
struct Args {
    binary: PathBuf,
    config: PathBuf,
    log: PathBuf,
    pidfile: PathBuf,
    stopfile: PathBuf,
}

#[cfg(target_os = "windows")]
fn args_from_data_dir(data_dir: PathBuf) -> Option<Args> {
    if data_dir.as_os_str().is_empty() {
        return None;
    }
    // Derive every other path from the app data dir (fixed layout) so the
    // scheduled task's `/TR` action stays far below schtasks's 261-char
    // limit: only the data dir is baked into the task.
    let exe_dir = std::env::current_exe().ok()?.parent().map(PathBuf::from)?;
    Some(Args {
        binary: exe_dir.join("sing-box.exe"),
        config: data_dir.join("config.json"),
        log: data_dir.join("logs").join("sing-box.log"),
        pidfile: data_dir.join("tun-task.pid"),
        stopfile: data_dir.join("tun-task.stop"),
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
        // CREATE_NO_WINDOW: the launcher is a GUI-subsystem process; a
        // console child would flash a black window on every stop.
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
fn install_task(xml: &Path) -> i32 {
    if !xml.is_file() {
        return 2;
    }
    silent_schtasks(
        Command::new("schtasks.exe")
            .args(["/Create", "/TN", ice_tun_pin::TUN_TASK_NAME, "/XML"])
            .arg(xml)
            .arg("/F"),
    )
}

#[cfg(target_os = "windows")]
fn delete_task() -> i32 {
    let code = silent_schtasks(Command::new("schtasks.exe").args([
        "/Delete",
        "/TN",
        ice_tun_pin::TUN_TASK_NAME,
        "/F",
    ]));
    let program_data = ice_tun_pin::program_data_dir();
    let install_dir = ice_tun_pin::protected_install_dir(&program_data);
    let _ = std::fs::remove_dir_all(&install_dir);
    code
}

#[cfg(target_os = "windows")]
fn install_protected(data_dir: &Path) -> i32 {
    if data_dir.as_os_str().is_empty() {
        return 2;
    }
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(_) => return 2,
    };
    let src_dir = match exe.parent() {
        Some(dir) => dir.to_path_buf(),
        None => return 2,
    };
    let core_src = src_dir.join("sing-box.exe");
    if !core_src.is_file() {
        eprintln!("sing-box.exe not found next to {}", exe.display());
        return 2;
    }
    let program_data = ice_tun_pin::program_data_dir();
    let bin_dir = ice_tun_pin::protected_bin_dir(&program_data);
    let run_dir = ice_tun_pin::protected_run_dir(&program_data);
    if std::fs::create_dir_all(&bin_dir).is_err() || std::fs::create_dir_all(&run_dir).is_err() {
        return 2;
    }
    let dest_launcher = ice_tun_pin::protected_launcher_path(&program_data);
    let dest_core = bin_dir.join("sing-box.exe");
    if copy_protected_file(&exe, &dest_launcher).is_err() {
        return 2;
    }
    if copy_protected_file(&core_src, &dest_core).is_err() {
        return 2;
    }
    for name in ["libcronet.dll", "wintun.dll"] {
        let src = src_dir.join(name);
        if src.is_file() {
            let dest = bin_dir.join(name);
            if copy_protected_file(&src, &dest).is_err() {
                return 2;
            }
        }
    }
    if apply_acl(&bin_dir, true, true).is_err() || apply_acl(&run_dir, true, false).is_err() {
        return 2;
    }
    if write_resources_pointer(&program_data, &src_dir).is_err() {
        return 2;
    }
    let Ok(launcher_sha) = ice_tun_pin::sha256_of_file(&dest_launcher) else {
        return 2;
    };
    let Ok(core_sha) = ice_tun_pin::sha256_of_file(&dest_core) else {
        return 2;
    };
    let pin = ice_tun_pin::format_tun_task_pin(&launcher_sha, &core_sha);
    let xml = ice_tun_pin::render_tun_task_xml(&dest_launcher, data_dir, &pin);
    let xml_path = ice_tun_pin::protected_install_dir(&program_data).join("ice-box-tun.xml");
    let bytes = ice_tun_pin::encode_utf16_le_bom(&xml);
    if std::fs::write(&xml_path, bytes).is_err() {
        return 2;
    }
    let _ = apply_acl(&xml_path, false, false);
    let code = install_task(&xml_path);
    let _ = std::fs::remove_file(&xml_path);
    code
}

#[cfg(target_os = "windows")]
fn copy_protected_file(src: &Path, dest: &Path) -> Result<(), ()> {
    std::fs::copy(src, dest).map(|_| ()).map_err(|_| ())
}

/// SYSTEM + Administrators full; optionally Users read/execute.
#[cfg(target_os = "windows")]
fn apply_acl(path: &Path, directory: bool, users_rx: bool) -> Result<(), ()> {
    let inherit = if directory { "(OI)(CI)" } else { "" };
    let mut grants = vec![
        format!("*S-1-5-18:{inherit}F"),
        format!("*S-1-5-32-544:{inherit}F"),
    ];
    if users_rx {
        grants.push(format!("*S-1-5-32-545:{inherit}RX"));
    }
    icacls(path, &["/inheritance:r".to_string()])?;
    let mut extra = vec!["/grant:r".to_string()];
    extra.extend(grants);
    icacls(path, &extra)
}

#[cfg(target_os = "windows")]
fn icacls(path: &Path, extra: &[String]) -> Result<(), ()> {
    use std::os::windows::process::CommandExt;
    let mut cmd = Command::new("icacls.exe");
    cmd.arg(path).args(extra);
    match cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .status()
    {
        Ok(status) if status.success() => Ok(()),
        _ => Err(()),
    }
}

#[cfg(target_os = "windows")]
fn write_resources_pointer(program_data: &Path, resources: &Path) -> Result<(), ()> {
    let path = ice_tun_pin::protected_install_dir(program_data).join("resources-dir.txt");
    std::fs::write(&path, resources.to_string_lossy().as_bytes()).map_err(|_| ())?;
    apply_acl(&path, false, false)
}

#[cfg(target_os = "windows")]
fn read_resources_pointer(program_data: &Path) -> Option<PathBuf> {
    let path = ice_tun_pin::protected_install_dir(program_data).join("resources-dir.txt");
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
        Some(ice_tun_pin::LauncherCommand::Install { data_dir }) => install_protected(&data_dir),
        Some(ice_tun_pin::LauncherCommand::InstallTask { xml }) => install_task(&xml),
        Some(ice_tun_pin::LauncherCommand::DeleteTask) => delete_task(),
        Some(ice_tun_pin::LauncherCommand::Run { data_dir }) => {
            let Some(args) = args_from_data_dir(data_dir) else {
                return 2;
            };
            run_core(args)
        }
        None => {
            eprintln!(
                "usage: ice-tun-launcher --data <app-data-dir>\n       ice-tun-launcher --install --data <app-data-dir>\n       ice-tun-launcher --delete-task"
            );
            2
        }
    }
}

#[cfg(target_os = "windows")]
fn rotate_core_log(path: &std::path::Path) {
    const MAX_BYTES: u64 = 20 * 1024 * 1024;
    const KEEP: u32 = 3;
    let Ok(meta) = std::fs::metadata(path) else {
        return;
    };
    if meta.len() <= MAX_BYTES {
        return;
    }
    let rotated = |n: u32| {
        let mut name = path.file_name().unwrap_or_default().to_os_string();
        name.push(format!(".{n}"));
        match path.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => parent.join(name),
            _ => std::path::PathBuf::from(name),
        }
    };
    let _ = std::fs::remove_file(rotated(KEEP));
    for i in (1..KEEP).rev() {
        let from = rotated(i);
        if from.exists() {
            let _ = std::fs::rename(&from, rotated(i + 1));
        }
    }
    if path.exists() {
        let _ = std::fs::rename(path, rotated(1));
    }
}

#[cfg(target_os = "windows")]
fn run_core(args: Args) -> i32 {
    let program_data = ice_tun_pin::program_data_dir();
    let Ok(exe) = std::env::current_exe() else {
        return 2;
    };
    if !ice_tun_pin::path_is_protected_launcher(&exe, &program_data) {
        eprintln!(
            "refusing to run {} (not the protected ProgramData launcher)",
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
    let protected_config = match sanitize_user_config(&args.config, &args.log, &program_data) {
        Ok(path) => path,
        Err(err) => {
            eprintln!("{err}");
            return 2;
        }
    };
    if let Some(parent) = args.log.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            eprintln!("cannot create log dir {}", parent.display());
            return 2;
        }
    }
    rotate_core_log(&args.log);

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

    loop {
        // Graceful stop requested by the app (stop file present).
        if args.stopfile.exists() {
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
            let _ = std::fs::remove_file(&args.stopfile);
            break;
        }
        // The core exited on its own.
        if child.try_wait().ok().flatten().is_some() {
            break;
        }
        std::thread::sleep(POLL_INTERVAL);
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
    let resources_dir = read_resources_pointer(program_data).unwrap_or_else(|| {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| data_dir.clone())
    });
    let run_dir = ice_tun_pin::protected_run_dir(program_data);
    std::fs::create_dir_all(&run_dir)
        .map_err(|err| format!("create {}: {err}", run_dir.display()))?;
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
    let _ = apply_acl(&dest, false, false);
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
