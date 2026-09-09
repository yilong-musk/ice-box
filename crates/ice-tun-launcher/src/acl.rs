// SPDX-License-Identifier: GPL-3.0-or-later

//! Take ownership, reset DACLs, and copy files into admin-owned trees.
//! Directories are wiped before recreate so a pre-created ProgramData owner
//! cannot keep WRITE_DAC across `std::fs::copy`.

use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Clone, Copy)]
pub enum UsersAccess {
    None,
    Read,
    ReadExecute,
}

pub fn wipe_and_create_dir(path: &Path, users: UsersAccess) -> Result<(), ()> {
    if path.exists() {
        let _ = take_ownership(path);
        let _ = icacls(
            path,
            &[
                "/grant:r".to_string(),
                "*S-1-5-32-544:(OI)(CI)F".to_string(),
                "/T".to_string(),
                "/C".to_string(),
            ],
        );
        std::fs::remove_dir_all(path).map_err(|_| ())?;
    }
    std::fs::create_dir_all(path).map_err(|_| ())?;
    take_ownership(path)?;
    apply_acl(path, true, users)?;
    set_owner_administrators(path, true)
}

pub fn copy_protected_file(src: &Path, dest: &Path, users: UsersAccess) -> Result<(), ()> {
    let _ = std::fs::remove_file(dest);
    std::fs::copy(src, dest).map_err(|_| ())?;
    apply_acl(dest, false, users)?;
    set_owner_administrators(dest, false)
}

pub fn apply_acl(path: &Path, directory: bool, users: UsersAccess) -> Result<(), ()> {
    let inherit = if directory { "(OI)(CI)" } else { "" };
    let mut grants = vec![
        format!("*S-1-5-18:{inherit}F"),
        format!("*S-1-5-32-544:{inherit}F"),
    ];
    match users {
        UsersAccess::None => {}
        UsersAccess::Read => grants.push(format!("*S-1-5-32-545:{inherit}R")),
        UsersAccess::ReadExecute => grants.push(format!("*S-1-5-32-545:{inherit}RX")),
    }
    icacls(path, &["/inheritance:r".to_string()])?;
    let mut extra = vec!["/grant:r".to_string()];
    extra.extend(grants);
    icacls(path, &extra)
}

pub fn take_ownership(path: &Path) -> Result<(), ()> {
    let mut cmd = Command::new("takeown.exe");
    cmd.args(["/F"]).arg(path).args(["/A", "/R", "/D", "Y"]);
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

pub fn set_owner_administrators(path: &Path, recursive: bool) -> Result<(), ()> {
    let mut extra = vec!["/setowner".to_string(), "*S-1-5-32-544".to_string()];
    if recursive {
        extra.push("/T".to_string());
        extra.push("/C".to_string());
    }
    icacls(path, &extra)
}

pub fn remove_protected_tree(path: &Path) {
    if !path.exists() {
        return;
    }
    let _ = take_ownership(path);
    let _ = icacls(
        path,
        &[
            "/grant:r".to_string(),
            "*S-1-5-32-544:(OI)(CI)F".to_string(),
            "/T".to_string(),
            "/C".to_string(),
        ],
    );
    let _ = std::fs::remove_dir_all(path);
}

fn icacls(path: &Path, extra: &[String]) -> Result<(), ()> {
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
