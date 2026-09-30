// SPDX-License-Identifier: GPL-3.0-or-later

//! Application data directory layout.
//!
//! Path joins only, plus `ensure_dirs` (`std::fs::create_dir_all`). Settings
//! load/save stays in `ice-config`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Host-private files, plus the shared root the tunnel process reads.
///
/// [`AppPaths::new`] sets both roots to the same directory. That is the
/// desktop layout: every path stays where it is today. Mobile calls
/// [`AppPaths::with_shared`] so the tunnel can read config, geoip, and the
/// core log even when the platform maps the two roots to one directory.
#[derive(Debug, Clone)]
pub struct AppPaths {
    root: PathBuf,
    shared: PathBuf,
}

impl AppPaths {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self {
            shared: root.clone(),
            root,
        }
    }

    /// `root` is host-private (settings, subscriptions, the app log).
    /// `shared` is what the tunnel process reads (config, geoip, core log).
    pub fn with_shared(root: impl Into<PathBuf>, shared: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            shared: shared.into(),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn shared_root(&self) -> &Path {
        &self.shared
    }

    pub fn settings(&self) -> PathBuf {
        self.root.join("settings.json")
    }

    /// App-update check throttle / skip state.
    pub fn update_check(&self) -> PathBuf {
        self.root.join("update-check.json")
    }

    pub fn config(&self) -> PathBuf {
        self.shared.join("config.json")
    }

    /// Per-install Clash API Bearer token (0600 on Unix). Not part of
    /// `settings.json` so the webview never sees it.
    pub fn clash_api_secret(&self) -> PathBuf {
        self.root.join("clash-api.secret")
    }

    pub fn config_bak(&self) -> PathBuf {
        self.shared.join("config.json.bak")
    }

    pub fn proxy_backup(&self) -> PathBuf {
        self.root.join("proxy-backup.json")
    }

    /// TUN mutation journal + ownership records (`docs/tun.md`, mutation journal).
    pub fn tun_state(&self) -> PathBuf {
        self.root.join("tun-state.json")
    }

    /// Settings transaction pending record (`docs/tun.md`): written before a live
    /// capture-backend transition, committed only after health checks pass,
    /// cleared after commit; startup treats a leftover as an interrupted
    /// transition and restores the committed settings.
    pub fn pending_settings(&self) -> PathBuf {
        self.root.join("settings-pending.json")
    }

    /// Persisted per-group member selections (survive restarts / config regeneration).
    pub fn group_selections(&self) -> PathBuf {
        self.root.join("group-selections.json")
    }

    /// Persisted rule overrides: disabled subscription rules + user custom rules.
    pub fn rule_overrides(&self) -> PathBuf {
        self.root.join("rules.json")
    }

    pub fn pid(&self) -> PathBuf {
        self.root.join("sing-box.pid")
    }

    pub fn subscriptions_dir(&self) -> PathBuf {
        self.root.join("subscriptions")
    }

    /// Bundled `geoip-{code}.srs` rule-sets copied next to the app data (used by route rules).
    pub fn geoip_dir(&self) -> PathBuf {
        self.shared.join("geoip")
    }

    pub fn subscriptions_index(&self) -> PathBuf {
        self.subscriptions_dir().join("index.json")
    }

    pub fn subscription_dir(&self, id: &str) -> PathBuf {
        self.subscriptions_dir().join(id)
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }

    pub fn app_log(&self) -> PathBuf {
        self.logs_dir().join("ice-box.log")
    }

    /// Desktop keeps `logs/sing-box.log`. A distinct shared root matches the
    /// mobile config generator, which writes `log.output` to `sing-box.log`
    /// directly under that root.
    pub fn core_log(&self) -> PathBuf {
        if self.shared == self.root {
            self.logs_dir().join("sing-box.log")
        } else {
            self.shared.join("sing-box.log")
        }
    }

    /// Create the private root, the shared root, `subscriptions/`, and `logs/`.
    pub fn ensure_dirs(&self) -> io::Result<()> {
        fs::create_dir_all(self.root())?;
        fs::create_dir_all(self.shared_root())?;
        fs::create_dir_all(self.subscriptions_dir())?;
        fs::create_dir_all(self.logs_dir())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_join_expected_names() {
        let p = AppPaths::new("/tmp/ice-box-data");
        assert_eq!(
            p.settings(),
            PathBuf::from("/tmp/ice-box-data/settings.json")
        );
        assert_eq!(
            p.update_check(),
            PathBuf::from("/tmp/ice-box-data/update-check.json")
        );
        assert_eq!(p.config(), PathBuf::from("/tmp/ice-box-data/config.json"));
        assert_eq!(
            p.clash_api_secret(),
            PathBuf::from("/tmp/ice-box-data/clash-api.secret")
        );
        assert_eq!(
            p.config_bak(),
            PathBuf::from("/tmp/ice-box-data/config.json.bak")
        );
        assert_eq!(
            p.proxy_backup(),
            PathBuf::from("/tmp/ice-box-data/proxy-backup.json")
        );
        assert_eq!(
            p.tun_state(),
            PathBuf::from("/tmp/ice-box-data/tun-state.json")
        );
        assert_eq!(
            p.pending_settings(),
            PathBuf::from("/tmp/ice-box-data/settings-pending.json")
        );
        assert_eq!(
            p.rule_overrides(),
            PathBuf::from("/tmp/ice-box-data/rules.json")
        );
        assert_eq!(p.pid(), PathBuf::from("/tmp/ice-box-data/sing-box.pid"));
        assert_eq!(
            p.subscriptions_index(),
            PathBuf::from("/tmp/ice-box-data/subscriptions/index.json")
        );
        assert_eq!(
            p.app_log(),
            PathBuf::from("/tmp/ice-box-data/logs/ice-box.log")
        );
        assert_eq!(
            p.core_log(),
            PathBuf::from("/tmp/ice-box-data/logs/sing-box.log")
        );
        assert_eq!(p.geoip_dir(), PathBuf::from("/tmp/ice-box-data/geoip"));
    }

    #[test]
    fn shared_root_keeps_host_files_private_and_core_log_beside_config() {
        let private = PathBuf::from("/tmp/ice-box-private");
        let shared = PathBuf::from("/tmp/ice-box-shared");
        let paths = AppPaths::with_shared(&private, &shared);
        assert_eq!(paths.settings(), private.join("settings.json"));
        assert_eq!(
            paths.subscriptions_index(),
            private.join("subscriptions/index.json")
        );
        assert_eq!(paths.app_log(), private.join("logs/ice-box.log"));
        assert_eq!(paths.clash_api_secret(), private.join("clash-api.secret"));
        assert_eq!(paths.config(), shared.join("config.json"));
        assert_eq!(paths.config_bak(), shared.join("config.json.bak"));
        assert_eq!(paths.geoip_dir(), shared.join("geoip"));
        assert_eq!(paths.core_log(), shared.join("sing-box.log"));
        assert_eq!(paths.shared_root(), shared.as_path());
    }

    #[test]
    fn same_shared_root_keeps_the_desktop_core_log() {
        let paths = AppPaths::with_shared("/tmp/ice-box-data", "/tmp/ice-box-data");
        assert_eq!(
            paths.core_log(),
            PathBuf::from("/tmp/ice-box-data/logs/sing-box.log")
        );
    }
}
