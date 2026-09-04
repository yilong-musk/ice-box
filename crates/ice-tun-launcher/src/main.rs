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
//! Usage: `ice-tun-launcher --binary <path> --config <path> --log <path>
//! --pidfile <path> --stopfile <path>`

#[cfg(target_os = "windows")]
use std::path::PathBuf;
#[cfg(target_os = "windows")]
use std::process::{Command, Stdio};
#[cfg(target_os = "windows")]
use std::time::{Duration, Instant};

#[cfg(target_os = "windows")]
const POLL_INTERVAL: Duration = Duration::from_millis(300);
#[cfg(target_os = "windows")]
const TERM_GRACE: Duration = Duration::from_secs(5);

#[cfg(target_os = "windows")]
struct Args {
    binary: PathBuf,
    config: PathBuf,
    log: PathBuf,
    pidfile: PathBuf,
    stopfile: PathBuf,
}

#[cfg(target_os = "windows")]
fn parse_args() -> Option<Args> {
    let mut args = Args {
        binary: PathBuf::new(),
        config: PathBuf::new(),
        log: PathBuf::new(),
        pidfile: PathBuf::new(),
        stopfile: PathBuf::new(),
    };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let value = it.next()?;
        match flag.as_str() {
            "--binary" => args.binary = PathBuf::from(value),
            "--config" => args.config = PathBuf::from(value),
            "--log" => args.log = PathBuf::from(value),
            "--pidfile" => args.pidfile = PathBuf::from(value),
            "--stopfile" => args.stopfile = PathBuf::from(value),
            _ => return None,
        }
    }
    if args.binary.as_os_str().is_empty()
        || args.config.as_os_str().is_empty()
        || args.log.as_os_str().is_empty()
        || args.pidfile.as_os_str().is_empty()
        || args.stopfile.as_os_str().is_empty()
    {
        return None;
    }
    Some(args)
}

#[cfg(target_os = "windows")]
fn run() -> i32 {
    let Some(args) = parse_args() else {
        eprintln!(
            "usage: ice-tun-launcher --binary <path> --config <path> --log <path> --pidfile <path> --stopfile <path>"
        );
        return 2;
    };
    if !args.binary.is_file() {
        eprintln!("sing-box binary not found at {}", args.binary.display());
        return 2;
    }
    if !args.config.is_file() {
        eprintln!("config not found at {}", args.config.display());
        return 2;
    }
    if let Some(parent) = args.log.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            eprintln!("cannot create log dir {}", parent.display());
            return 2;
        }
    }

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
        .arg(&args.config)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err))
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .spawn()
    {
        Ok(child) => child,
        Err(err) => {
            eprintln!(
                "spawn {} run -c {}: {err}",
                args.binary.display(),
                args.config.display()
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
                let _ = Command::new("taskkill")
                    .args(["/PID", &pid.to_string(), "/T", "/F"])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
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

#[cfg(target_os = "windows")]
fn graceful_stop(pid: u32) {
    // taskkill without `/F` delivers WM_CLOSE, which the console sing-box
    // treats as a shutdown signal and uses to remove its WFP filters and
    // routes (design note tun-windows-t0 §4). A failure here is benign: the
    // forced `/F` fallback decides.
    let _ = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(not(target_os = "windows"))]
fn run() -> i32 {
    eprintln!("ice-tun-launcher is Windows-only");
    2
}

fn main() {
    std::process::exit(run());
}
