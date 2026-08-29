//! Application data directory layout (architecture §6).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Resolved paths under Tauri `app_data_dir` (or a test temp root).
#[derive(Debug, Clone)]
pub struct AppPaths {
    root: PathBuf,
}

impl AppPaths {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn settings(&self) -> PathBuf {
        self.root.join("settings.json")
    }

    pub fn config(&self) -> PathBuf {
        self.root.join("config.json")
    }

    pub fn config_bak(&self) -> PathBuf {
        self.root.join("config.json.bak")
    }

    pub fn proxy_backup(&self) -> PathBuf {
        self.root.join("proxy-backup.json")
    }

    /// TUN mutation journal + ownership records (plan §4.4 / architecture §24.4).
    pub fn tun_state(&self) -> PathBuf {
        self.root.join("tun-state.json")
    }

    /// Settings transaction pending record (plan §4.3): written before a live
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
        self.root.join("geoip")
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

    pub fn core_log(&self) -> PathBuf {
        self.logs_dir().join("sing-box.log")
    }

    /// Create `subscriptions/` and `logs/` (and the root itself).
    pub fn ensure_dirs(&self) -> io::Result<()> {
        fs::create_dir_all(self.root())?;
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
        assert_eq!(p.config(), PathBuf::from("/tmp/ice-box-data/config.json"));
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
    }
}
