// SPDX-License-Identifier: GPL-3.0-or-later

//! Take ownership, reset DACLs, and copy files into admin-owned trees.
//! Directories are wiped before recreate so a pre-created ProgramData owner
//! cannot keep WRITE_DAC across `std::fs::copy`.

use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub use ice_tun_pin::ProtectedUsersAccess as UsersAccess;

pub fn wipe_and_create_dir(path: &Path, users: UsersAccess) -> Result<(), ()> {
    if path.exists() {
        let _ = take_ownership(path);
        // Recursive grant is only to make a pre-existing tree deletable.
        // The tree that sing-box later reads is locked by [`apply_acl`].
        let _ = icacls(
            path,
            &[
                "/grant:r".to_string(),
                format!("{}:(OI)(CI)F", ice_tun_pin::SID_ADMINISTRATORS),
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

/// Lock `path` to SYSTEM + Administrators (and optional Users).
///
/// Directory grants are inheritable `(OI)(CI)` so *new* children pick them up.
/// Existing files are then locked with a **file** DACL (`SYSTEM:F` /
/// `Administrators:F`, no inherit flags). Do not run `icacls /T` with
/// `(OI)(CI)`: that combination strips file DACLs and leaves staged
/// `.srs` copies unreadable by the elevated core on the next TUN start.
pub fn apply_acl(path: &Path, directory: bool, users: UsersAccess) -> Result<(), ()> {
    apply_acl_object(path, directory, users)?;
    if directory {
        apply_acl_children(path, users)?;
    }
    Ok(())
}

fn apply_acl_object(path: &Path, directory: bool, users: UsersAccess) -> Result<(), ()> {
    icacls(path, &ice_tun_pin::icacls_reset_inheritance_args())?;
    icacls(
        path,
        &ice_tun_pin::icacls_grant_protected_args(directory, users),
    )
}

fn apply_acl_children(dir: &Path, users: UsersAccess) -> Result<(), ()> {
    let entries = std::fs::read_dir(dir).map_err(|_| ())?;
    for entry in entries {
        let entry = entry.map_err(|_| ())?;
        let child = entry.path();
        let file_type = entry.file_type().map_err(|_| ())?;
        if file_type.is_symlink() {
            return Err(());
        }
        if file_type.is_dir() {
            apply_acl(&child, true, users)?;
        } else {
            apply_acl_object(&child, false, users)?;
        }
    }
    Ok(())
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
    let mut extra = vec![
        "/setowner".to_string(),
        ice_tun_pin::SID_ADMINISTRATORS.to_string(),
    ];
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
            format!("{}:(OI)(CI)F", ice_tun_pin::SID_ADMINISTRATORS),
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
