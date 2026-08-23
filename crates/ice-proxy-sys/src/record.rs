//! Helpers that apply/restore and update `proxy-backup.json` atomically.

use std::path::Path;

use chrono::Utc;

use crate::{ProxyBackupFile, ProxyEndpoints, ProxySysError, SystemProxy};

/// Persist backup → mark pending → apply → write `applied: true`.
/// On apply failure, restores the in-memory backup and clears pending.
pub fn apply_and_record(
    backup_path: &Path,
    proxy: &dyn SystemProxy,
    endpoints: &ProxyEndpoints,
) -> Result<(), ProxySysError> {
    // Refuse to snapshot the OS proxy state while the on-disk backup still reports an
    // outstanding apply (e.g. a previous restore failed). The current OS state may be
    // ice-box's own settings; snapshotting it as the "user backup" would permanently
    // lose the user's original settings on the next restore.
    if let Ok(record) = ProxyBackupFile::load(backup_path) {
        if record.applied || record.pending_apply {
            return Err(ProxySysError::RestoreFailed(
                "system proxy is still marked applied on disk; restore must succeed before re-applying"
                    .into(),
            ));
        }
    }

    let backup = proxy.backup()?;
    let pending = ProxyBackupFile {
        applied: false,
        pending_apply: true,
        applied_at: None,
        endpoints: endpoints.clone(),
        backup: backup.clone(),
    };
    pending.save(backup_path)?;

    match proxy.apply(endpoints) {
        Ok(()) => {
            let record = ProxyBackupFile {
                applied: true,
                pending_apply: false,
                applied_at: Some(Utc::now()),
                endpoints: endpoints.clone(),
                backup,
            };
            record.save(backup_path)?;
            Ok(())
        }
        Err(err) => match proxy.restore(&backup) {
            Ok(()) => {
                let cleared = ProxyBackupFile {
                    applied: false,
                    pending_apply: false,
                    applied_at: None,
                    endpoints: endpoints.clone(),
                    backup,
                };
                cleared.save(backup_path)?;
                Err(err)
            }
            Err(restore_err) => {
                tracing::error!(
                    apply_error = %err,
                    restore_error = %restore_err,
                    "proxy apply failed and rollback failed"
                );
                let stuck = ProxyBackupFile {
                    applied: false,
                    pending_apply: true,
                    applied_at: None,
                    endpoints: endpoints.clone(),
                    backup,
                };
                stuck.save(backup_path)?;
                Err(ProxySysError::RestoreFailed(format!(
                    "apply failed ({err}); rollback also failed: {restore_err}"
                )))
            }
        },
    }
}

/// Restore from file when `applied` or `pending_apply`, then clear both flags.
/// Returns `true` when a restore was performed.
pub fn restore_and_clear_flag(
    backup_path: &Path,
    proxy: &dyn SystemProxy,
) -> Result<bool, ProxySysError> {
    if !backup_path.exists() {
        return Ok(false);
    }
    let mut record = ProxyBackupFile::load(backup_path)?;
    if !record.applied && !record.pending_apply {
        return Ok(false);
    }
    proxy.restore(&record.backup)?;
    record.applied = false;
    record.pending_apply = false;
    record.save(backup_path)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ProxyBackup, ProxySysError, SystemProxy};
    use std::cell::Cell;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct FailApplyProxy;

    impl SystemProxy for FailApplyProxy {
        fn backup(&self) -> Result<ProxyBackup, ProxySysError> {
            Ok(ProxyBackup::default())
        }

        fn apply(&self, _endpoints: &ProxyEndpoints) -> Result<(), ProxySysError> {
            Err(ProxySysError::ApplyFailed("mock apply fail".into()))
        }

        fn restore(&self, _backup: &ProxyBackup) -> Result<(), ProxySysError> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct OkProxy {
        apply_calls: Cell<usize>,
    }

    struct FailRestoreProxy;

    impl SystemProxy for FailRestoreProxy {
        fn backup(&self) -> Result<ProxyBackup, ProxySysError> {
            Ok(ProxyBackup::default())
        }

        fn apply(&self, _endpoints: &ProxyEndpoints) -> Result<(), ProxySysError> {
            Err(ProxySysError::ApplyFailed("mock apply fail".into()))
        }

        fn restore(&self, _backup: &ProxyBackup) -> Result<(), ProxySysError> {
            Err(ProxySysError::RestoreFailed("mock restore fail".into()))
        }
    }

    impl SystemProxy for OkProxy {
        fn backup(&self) -> Result<ProxyBackup, ProxySysError> {
            Ok(ProxyBackup {
                enabled: false,
                http: None,
                https: None,
                socks: None,
                extra: serde_json::json!({ "services": [] }),
            })
        }

        fn apply(&self, _endpoints: &ProxyEndpoints) -> Result<(), ProxySysError> {
            self.apply_calls.set(self.apply_calls.get() + 1);
            Ok(())
        }

        fn restore(&self, _backup: &ProxyBackup) -> Result<(), ProxySysError> {
            Ok(())
        }
    }

    fn temp_path(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ice-box-apply-record-{label}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir.join("proxy-backup.json")
    }

    #[test]
    fn g4_5_apply_failure_does_not_write_applied_true() {
        let path = temp_path("fail");
        let seed = ProxyBackupFile {
            applied: false,
            pending_apply: false,
            applied_at: None,
            endpoints: ProxyEndpoints {
                http_host: "127.0.0.1".into(),
                http_port: 1,
                socks_host: None,
                socks_port: None,
            },
            backup: ProxyBackup::default(),
        };
        seed.save(&path).unwrap();

        let err = apply_and_record(
            &path,
            &FailApplyProxy,
            &ProxyEndpoints {
                http_host: "127.0.0.1".into(),
                http_port: 17890,
                socks_host: None,
                socks_port: None,
            },
        )
        .expect_err("apply");
        assert!(matches!(err, ProxySysError::ApplyFailed(_)));

        let after = ProxyBackupFile::load(&path).unwrap();
        assert!(!after.applied);
        assert!(!after.pending_apply);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn apply_failure_with_restore_failure_keeps_pending_apply() {
        let path = temp_path("restore-fail");
        let err = apply_and_record(
            &path,
            &FailRestoreProxy,
            &ProxyEndpoints {
                http_host: "127.0.0.1".into(),
                http_port: 17890,
                socks_host: None,
                socks_port: None,
            },
        )
        .expect_err("apply+restore fail");
        assert!(matches!(err, ProxySysError::RestoreFailed(_)));

        let after = ProxyBackupFile::load(&path).unwrap();
        assert!(!after.applied);
        assert!(after.pending_apply);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn apply_refuses_while_backup_still_marks_applied() {
        let path = temp_path("already-applied");
        let seed = ProxyBackupFile {
            applied: true,
            pending_apply: false,
            applied_at: Some(Utc::now()),
            endpoints: ProxyEndpoints {
                http_host: "127.0.0.1".into(),
                http_port: 17890,
                socks_host: None,
                socks_port: None,
            },
            backup: ProxyBackup::default(),
        };
        seed.save(&path).unwrap();

        let proxy = OkProxy::default();
        let err = apply_and_record(
            &path,
            &proxy,
            &ProxyEndpoints {
                http_host: "127.0.0.1".into(),
                http_port: 17890,
                socks_host: None,
                socks_port: None,
            },
        )
        .expect_err("still applied");
        assert!(matches!(err, ProxySysError::RestoreFailed(_)));
        assert_eq!(
            proxy.apply_calls.get(),
            0,
            "must not snapshot OS state while backup still marks applied"
        );

        let after = ProxyBackupFile::load(&path).unwrap();
        assert!(after.applied, "original backup must be preserved");
        assert_eq!(after.backup, seed.backup);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn apply_success_sets_applied_true() {
        let path = temp_path("ok");
        apply_and_record(
            &path,
            &OkProxy::default(),
            &ProxyEndpoints {
                http_host: "127.0.0.1".into(),
                http_port: 17890,
                socks_host: Some("127.0.0.1".into()),
                socks_port: Some(17890),
            },
        )
        .unwrap();
        let after = ProxyBackupFile::load(&path).unwrap();
        assert!(after.applied);
        assert!(!after.pending_apply);
        assert_eq!(after.endpoints.http_port, 17890);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn pending_apply_file_is_recoverable() {
        let path = temp_path("pending");
        let record = ProxyBackupFile {
            applied: false,
            pending_apply: true,
            applied_at: None,
            endpoints: ProxyEndpoints {
                http_host: "127.0.0.1".into(),
                http_port: 17890,
                socks_host: None,
                socks_port: None,
            },
            backup: ProxyBackup {
                enabled: true,
                http: None,
                https: None,
                socks: None,
                extra: serde_json::json!({}),
            },
        };
        record.save(&path).unwrap();

        let proxy = OkProxy::default();
        let did = restore_and_clear_flag(&path, &proxy).unwrap();
        assert!(did);
        let after = ProxyBackupFile::load(&path).unwrap();
        assert!(!after.applied);
        assert!(!after.pending_apply);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn apply_persists_pending_before_apply_completes() {
        let path = temp_path("pending-order");
        struct TrackApply {
            path: std::path::PathBuf,
        }
        impl SystemProxy for TrackApply {
            fn backup(&self) -> Result<ProxyBackup, ProxySysError> {
                Ok(ProxyBackup::default())
            }
            fn apply(&self, _endpoints: &ProxyEndpoints) -> Result<(), ProxySysError> {
                let mid = ProxyBackupFile::load(&self.path).unwrap();
                assert!(mid.pending_apply);
                assert!(!mid.applied);
                Ok(())
            }
            fn restore(&self, _backup: &ProxyBackup) -> Result<(), ProxySysError> {
                Ok(())
            }
        }
        apply_and_record(
            &path,
            &TrackApply { path: path.clone() },
            &ProxyEndpoints {
                http_host: "127.0.0.1".into(),
                http_port: 17890,
                socks_host: None,
                socks_port: None,
            },
        )
        .unwrap();
        let after = ProxyBackupFile::load(&path).unwrap();
        assert!(after.applied);
        assert!(!after.pending_apply);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
}
