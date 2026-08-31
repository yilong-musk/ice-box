//! Copy platform sing-box into `resources/` for Tauri bundling (architecture §4.3).

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn target_dir_name() -> &'static str {
    match (
        env::var("CARGO_CFG_TARGET_OS").as_deref(),
        env::var("CARGO_CFG_TARGET_ARCH").as_deref(),
    ) {
        (Ok("macos"), Ok("aarch64")) => "darwin-aarch64",
        (Ok("macos"), Ok("x86_64")) => "darwin-x86_64",
        (Ok("windows"), Ok("x86_64")) => "windows-x86_64",
        _ => "unknown-target",
    }
}

fn binary_name() -> &'static str {
    match env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("windows") => "sing-box.exe",
        _ => "sing-box",
    }
}

fn copy_singbox_resource(manifest_dir: &Path) {
    let repo_root = manifest_dir
        .join("../../..")
        .canonicalize()
        .unwrap_or_else(|_| manifest_dir.join("../../.."));
    let src = repo_root
        .join("third_party/sing-box")
        .join(target_dir_name())
        .join(binary_name());
    let dest_dir = manifest_dir.join("resources");
    let dest = dest_dir.join(binary_name());

    if let Err(err) = fs::create_dir_all(&dest_dir) {
        println!("cargo:warning=create resources dir: {err}");
        return;
    }

    if src.is_file() {
        if let Err(err) = fs::copy(&src, &dest) {
            println!(
                "cargo:warning=copy sing-box resource {} → {}: {err}",
                src.display(),
                dest.display()
            );
        } else {
            println!("cargo:rerun-if-changed={}", src.display());
            println!(
                "cargo:warning=bundled sing-box resource → {}",
                dest.display()
            );
        }
    } else {
        println!(
            "cargo:warning=sing-box missing at {}; run scripts/fetch-singbox.sh before release build",
            src.display()
        );
        // Keep a stale resource if present so local rebuilds still package.
        if !dest.is_file() {
            // Touch an empty marker so non-release builds (`cargo check` / test / clippy)
            // work on a fresh checkout; `prepare-singbox-resource.sh` (beforeBuildCommand)
            // enforces a real binary for `tauri build`.
            if let Err(err) = fs::write(&dest, b"") {
                println!("cargo:warning=create sing-box resource marker: {err}");
            }
        }
    }
}

fn copy_geoip_resources(manifest_dir: &Path) {
    let repo_root = manifest_dir
        .join("../../..")
        .canonicalize()
        .unwrap_or_else(|_| manifest_dir.join("../../.."));
    let src = repo_root.join("third_party/sing-geoip/rule-set");
    let dest_dir = manifest_dir.join("resources").join("geoip");

    if !src.is_dir() {
        println!(
            "cargo:warning=geoip rule-sets missing at {}; run scripts/fetch-geoip.sh before release build",
            src.display()
        );
        return;
    }
    if let Err(err) = fs::create_dir_all(&dest_dir) {
        println!("cargo:warning=create geoip resources dir: {err}");
        return;
    }
    let mut copied = 0usize;
    if let Ok(entries) = fs::read_dir(&src) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            if !name.to_string_lossy().ends_with(".srs") {
                continue;
            }
            let dest = dest_dir.join(&name);
            if fs::copy(entry.path(), &dest).is_ok() {
                copied += 1;
            }
        }
    }
    println!(
        "cargo:warning=bundled {copied} geoip rule-sets → {}",
        dest_dir.display()
    );
}

/// Ensure `resources/ice-helper` exists for the Tauri bundle (plan §5 T5).
/// The production path builds the real daemon (`prepare-singbox-resource.sh`
/// does it in beforeBuildCommand); plain `cargo check` / test / clippy on a
/// fresh checkout gets a marker so the workspace gate stays green without a
/// build.
fn copy_helper_resource(manifest_dir: &Path) {
    let dest = manifest_dir.join("resources").join("ice-helper");
    if let Err(err) = fs::create_dir_all(dest.parent().expect("parent")) {
        println!("cargo:warning=create resources dir: {err}");
        return;
    }
    if dest.is_file() {
        return;
    }
    // Touch an empty marker so non-release builds work on a fresh checkout;
    // the real binary is built by scripts/prepare-singbox-resource.sh
    // (beforeBuildCommand) and CI verifies it is embedded.
    if let Err(err) = fs::write(&dest, b"") {
        println!("cargo:warning=create ice-helper resource marker: {err}");
    }
}

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    copy_singbox_resource(&manifest_dir);
    copy_geoip_resources(&manifest_dir);
    copy_helper_resource(&manifest_dir);
    tauri_build::build()
}
