//! `tun-state.json`: the TUN mutation journal (plan §4.4).
//!
//! The journal is a mutation log, not a final-state snapshot. It records
//! ownership (`owner_token`, `owned` flags), the last completed mutation
//! step, and the DNS snapshots needed for compare-before-restore. Every
//! write is atomic (temp file + rename); a crash mid-write leaves the
//! previous journal intact.
//!
//! The journal never enables capture. It only records what ice-box owns so
//! that recovery can remove exactly those resources and no others.

use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::error::{TunError, TunErrorCode};

/// Capture lifecycle states (plan §4.3). `clean` means verified: no owned
/// resource remains. `recovery_required` is fail-closed: ownership or
/// cleanup could not be verified and new TUN activation stays rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JournalState {
    Preparing,
    Applied,
    Restoring,
    Error,
    RecoveryRequired,
    Clean,
}

/// An address record ice-box may own on the adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CidrRecord {
    pub cidr: String,
    /// True when ice-box owns this address and may remove it.
    #[serde(default = "default_owned")]
    pub owned: bool,
}

const fn default_owned() -> bool {
    true
}

/// A route record ice-box may own on the platform route table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteRecord {
    pub destination: String,
    pub gateway: Option<String>,
    #[serde(default)]
    pub metric: u32,
    /// True when ice-box owns this route and may remove it.
    #[serde(default = "default_owned")]
    pub owned: bool,
}

/// Platform DNS snapshot. `before` is what must be restored on cleanup;
/// `after` is what the platform must still look like right before restore
/// (compare-before-restore: an external DNS change is never overwritten).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DnsSnapshot {
    pub platform_snapshot: String,
}

/// Mutation step names shared by the apply driver and the platform backends.
pub mod steps {
    /// Journal written, no OS mutation yet (enable path).
    pub const JOURNAL_PREPARING: &str = "journal_preparing";
    /// Adapter interface created.
    pub const INTERFACE_CREATED: &str = "interface_created";
    /// Adapter addresses assigned.
    pub const ADDRESSES_ASSIGNED: &str = "addresses_assigned";
    /// Owned routes added.
    pub const ROUTES_ADDED: &str = "routes_added";
    /// DNS interception applied (backend that owns DNS).
    pub const DNS_APPLIED: &str = "dns_applied";
    /// Applied capture verified healthy (interface, routes, DNS, control path).
    pub const VERIFY_APPLIED: &str = "verify_applied";
    /// Restore sequence started (disable path).
    pub const RESTORE_STARTED: &str = "restore_started";
    /// Adapter interface removed.
    pub const INTERFACE_REMOVED: &str = "interface_removed";
    /// Owned routes removed.
    pub const ROUTES_REMOVED: &str = "routes_removed";
    /// DNS restored (compare-before-restore).
    pub const DNS_RESTORED: &str = "dns_restored";
    /// Cleanup verified: no owned resource remains.
    pub const VERIFY_CLEAN: &str = "verify_clean";
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TunJournal {
    /// Current lifecycle state.
    pub state: JournalState,
    /// Opaque identifier of the capture transition that wrote this journal.
    pub transition_id: String,
    /// Adapter interface name (e.g. `utun420` on macOS, `wintun` on Windows).
    pub interface_name: Option<String>,
    /// Platform interface identity token (index / GUID), never guessed.
    pub interface_id: Option<String>,
    /// Addresses recorded as owned during the transition.
    #[serde(default)]
    pub addresses: Vec<CidrRecord>,
    /// Routes recorded as owned during the transition.
    #[serde(default)]
    pub routes: Vec<RouteRecord>,
    /// Addresses the transition *required* on the adapter (the config CIDRs).
    /// Health verification compares the live interface against this set, not
    /// just the recorded owned subset, so a missing address family (e.g. a
    /// tun that silently lost IPv6) can never pass because it was never
    /// recorded.
    #[serde(default)]
    pub expected_addresses: Vec<String>,
    /// Route destinations the transition *required* to resolve to the
    /// adapter. Verification requires every one of them to still resolve to
    /// us (full-route lock); a partially missing route set is never accepted.
    #[serde(default)]
    pub expected_routes: Vec<String>,
    /// DNS state before the first OS mutation (restore target).
    pub dns_before: Option<DnsSnapshot>,
    /// DNS state while capture is applied (compare-before-restore).
    pub dns_after: Option<DnsSnapshot>,
    /// `ice-box:<installation-id>` — recovery refuses foreign tokens.
    pub owner_token: String,
    /// The last mutation step known to have completed.
    pub last_completed_step: String,
    /// RFC3339 timestamp of the last journal write.
    pub updated_at: String,
}

impl TunJournal {
    pub fn new(transition_id: String, owner_token: String) -> Self {
        Self {
            state: JournalState::Preparing,
            transition_id,
            interface_name: None,
            interface_id: None,
            addresses: Vec::new(),
            routes: Vec::new(),
            expected_addresses: Vec::new(),
            expected_routes: Vec::new(),
            dns_before: None,
            dns_after: None,
            owner_token,
            last_completed_step: steps::JOURNAL_PREPARING.to_string(),
            updated_at: Utc::now().to_rfc3339(),
        }
    }

    /// Load the journal; a missing file means "no journal".
    pub fn load(path: &Path) -> Result<Option<Self>, TunError> {
        if !path.is_file() {
            return Ok(None);
        }
        let raw = fs::read_to_string(path).map_err(|err| {
            TunError::new(
                TunErrorCode::ApplyFailed,
                format!("read tun journal {}: {err}", path.display()),
            )
        })?;
        let journal: Self = serde_json::from_str(&raw).map_err(|err| {
            TunError::new(
                TunErrorCode::ApplyFailed,
                format!("parse tun journal {}: {err}", path.display()),
            )
        })?;
        Ok(Some(journal))
    }

    /// Atomically persist the journal (temp sibling + rename).
    pub fn save(&self, path: &Path) -> Result<(), TunError> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let tmp = path.with_file_name(format!(
            ".{}.{nanos}.tmp",
            path.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("tun-state")
        ));
        let result = (|| -> std::io::Result<()> {
            let text = serde_json::to_string_pretty(self)?;
            fs::write(&tmp, text)?;
            Ok(())
        })();
        if let Err(err) = result {
            let _ = fs::remove_file(&tmp);
            return Err(TunError::from(err));
        }
        fs::rename(&tmp, path).map_err(|err| {
            let _ = fs::remove_file(&tmp);
            TunError::from(err)
        })?;
        Ok(())
    }

    /// Record a completed mutation boundary: update state + step + timestamp
    /// and persist atomically. `mutate` lets callers update ownership fields
    /// before the write (the write order is: mutate in memory → save).
    pub fn record(
        &mut self,
        path: &Path,
        state: JournalState,
        step: &str,
        mutate: impl FnOnce(&mut Self),
    ) -> Result<(), TunError> {
        self.state = state;
        self.last_completed_step = step.to_string();
        self.updated_at = Utc::now().to_rfc3339();
        mutate(self);
        self.save(path)
    }

    /// Whether the journal still represents a live or in-flight capture
    /// (anything but clean / missing). Used by the controller to reject new
    /// transitions while recovery is pending.
    pub fn is_capture_outstanding(&self) -> bool {
        !matches!(self.state, JournalState::Clean)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_journal_path(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ice-tun-journal-{label}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir.join("tun-state.json")
    }

    #[test]
    fn missing_journal_loads_as_none() {
        let path = temp_journal_path("missing");
        assert_eq!(TunJournal::load(&path).unwrap(), None);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn new_journal_is_preparing_with_step_and_token() {
        let journal = TunJournal::new("t-1".into(), "ice-box:inst-1".into());
        assert_eq!(journal.state, JournalState::Preparing);
        assert_eq!(journal.last_completed_step, steps::JOURNAL_PREPARING);
        assert_eq!(journal.owner_token, "ice-box:inst-1");
        assert!(journal.is_capture_outstanding());
    }

    #[test]
    fn save_load_round_trip_preserves_ownership_fields() {
        let path = temp_journal_path("roundtrip");
        let mut journal = TunJournal::new("t-2".into(), "ice-box:inst-2".into());
        journal
            .record(&path, JournalState::Applied, steps::VERIFY_APPLIED, |j| {
                j.interface_name = Some("utun420".into());
                j.interface_id = Some("idx-7".into());
                j.addresses.push(CidrRecord {
                    cidr: "10.0.0.1/30".into(),
                    owned: true,
                });
                j.expected_addresses = vec!["10.0.0.1/30".into(), "fdfe:dcba:9876::1/126".into()];
                j.routes.push(RouteRecord {
                    destination: "128.0.0.0/1".into(),
                    gateway: Some("10.0.0.2".into()),
                    metric: 0,
                    owned: true,
                });
                j.expected_routes = vec!["128.0.0.0/1".into()];
                j.dns_before = Some(DnsSnapshot {
                    platform_snapshot: "before".into(),
                });
                j.dns_after = Some(DnsSnapshot {
                    platform_snapshot: "after".into(),
                });
            })
            .expect("record applied");

        let mut loaded = TunJournal::load(&path).unwrap().expect("journal exists");
        assert_eq!(loaded.state, JournalState::Applied);
        assert_eq!(loaded.last_completed_step, steps::VERIFY_APPLIED);
        assert_eq!(loaded.interface_name.as_deref(), Some("utun420"));
        assert_eq!(loaded.addresses.len(), 1);
        assert!(loaded.addresses[0].owned);
        assert!(loaded.routes[0].owned);
        assert_eq!(
            loaded.expected_addresses,
            vec![
                "10.0.0.1/30".to_string(),
                "fdfe:dcba:9876::1/126".to_string()
            ]
        );
        assert_eq!(loaded.expected_routes, vec!["128.0.0.0/1".to_string()]);
        assert!(loaded.is_capture_outstanding());

        loaded
            .record(&path, JournalState::Clean, steps::VERIFY_CLEAN, |j| {
                // The driver clears ownership records when persisting
                // `clean`; a journal that keeps them would keep claiming
                // ownership after verification.
                j.interface_name = None;
                j.interface_id = None;
                j.addresses.clear();
                j.routes.clear();
                j.expected_addresses.clear();
                j.expected_routes.clear();
                j.dns_before = None;
                j.dns_after = None;
            })
            .expect("record clean");
        let clean = TunJournal::load(&path).unwrap().expect("journal exists");
        assert_eq!(clean.state, JournalState::Clean);
        assert!(!clean.is_capture_outstanding());
        assert_eq!(
            clean.addresses.len(),
            0,
            "clean journal keeps no owned resources"
        );
        assert!(clean.expected_addresses.is_empty());
        assert!(clean.expected_routes.is_empty());

        let leftovers: Vec<_> = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "no tmp leftovers");
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn unparseable_journal_is_an_error_not_a_wipe() {
        let path = temp_journal_path("corrupt");
        fs::write(&path, b"{not json").unwrap();
        let err = TunJournal::load(&path).unwrap_err();
        assert_eq!(err.code, TunErrorCode::ApplyFailed);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
}
