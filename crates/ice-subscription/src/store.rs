// SPDX-License-Identifier: GPL-3.0-or-later

//! Disk layout under `subscriptions/` (architecture §6).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::Utc;
use ice_config::{
    write_bytes_atomic, write_json_atomic, AppPaths, ConfigError, NormalizedOutbound,
    NormalizedProfile,
};
use uuid::Uuid;

use crate::error::SubscriptionError;
use crate::{SubscriptionIndex, SubscriptionMeta};

static COMMIT_LOCK: Mutex<()> = Mutex::new(());

pub struct SubscriptionPaths {
    root: PathBuf,
}

impl SubscriptionPaths {
    pub fn from_app(paths: &AppPaths) -> Self {
        Self {
            root: paths.subscriptions_dir(),
        }
    }

    pub fn from_root(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn staging_root(&self) -> PathBuf {
        self.root.join(".staging")
    }

    pub fn staging_dir(&self, id: Uuid) -> PathBuf {
        self.staging_root().join(id.to_string())
    }

    pub fn index(&self) -> PathBuf {
        self.root.join("index.json")
    }

    pub fn sub_dir(&self, id: Uuid) -> PathBuf {
        self.root.join(id.to_string())
    }

    pub fn raw(&self, id: Uuid) -> PathBuf {
        self.sub_dir(id).join("raw")
    }

    pub fn nodes(&self, id: Uuid) -> PathBuf {
        self.sub_dir(id).join("nodes.json")
    }

    pub fn meta(&self, id: Uuid) -> PathBuf {
        self.sub_dir(id).join("meta.json")
    }

    pub fn profile(&self, id: Uuid) -> PathBuf {
        self.sub_dir(id).join("profile.json")
    }
}

pub fn load_index(paths: &SubscriptionPaths) -> Result<SubscriptionIndex, SubscriptionError> {
    let path = paths.index();
    if !path.exists() {
        recover_subscription_dirs(paths);
        return Ok(SubscriptionIndex::default());
    }
    let raw = fs::read_to_string(&path)?;
    let mut index: SubscriptionIndex = serde_json::from_str(&raw)?;
    migrate_index_active(&mut index);
    recover_subscription_dirs(paths);
    Ok(index)
}

/// Ensure at most one `active` subscription (`enabled` deserializes via alias).
fn migrate_index_active(index: &mut SubscriptionIndex) {
    let mut kept = false;
    for meta in &mut index.items {
        if meta.active {
            if kept {
                meta.active = false;
            } else {
                kept = true;
            }
        }
    }
}

fn commit_staged_subscription(
    paths: &SubscriptionPaths,
    id: Uuid,
    raw_body: &str,
    profile: &NormalizedProfile,
    meta: &SubscriptionMeta,
) -> Result<(), SubscriptionError> {
    let _guard = COMMIT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let staging = paths.staging_dir(id);
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    fs::create_dir_all(&staging)?;
    write_bytes_atomic(&staging.join("raw"), raw_body.as_bytes()).map_err(map_cfg)?;
    // `nodes.json` is a legacy duplicate of `profile.nodes`; it is no longer
    // written. `read_profile` still falls back to it for pre-split dirs.
    write_json_atomic(&staging.join("profile.json"), profile).map_err(map_cfg)?;
    write_json_atomic(&staging.join("meta.json"), meta).map_err(map_cfg)?;

    let final_dir = paths.sub_dir(id);
    if final_dir.exists() {
        let ts = Utc::now().format("%Y%m%d%H%M%S%.f");
        let old = old_profile_dir(paths, id, &ts.to_string());
        fs::rename(&final_dir, &old).map_err(SubscriptionError::Io)?;
        if let Err(err) = fs::rename(&staging, &final_dir) {
            let _ = fs::rename(&old, &final_dir);
            return Err(SubscriptionError::Io(err));
        }
        let _ = fs::remove_dir_all(&old);
    } else if let Err(err) = fs::rename(&staging, &final_dir) {
        return Err(SubscriptionError::Io(err));
    }
    sweep_old_profile_dirs_locked(paths, Some(id));
    Ok(())
}

fn old_profile_dir(paths: &SubscriptionPaths, id: Uuid, ts: &str) -> PathBuf {
    paths.root().join(format!("{id}.old-{ts}"))
}

fn parse_old_profile_dir(name: &str) -> Option<Uuid> {
    let (id, rest) = name.split_once(".old-")?;
    if rest.is_empty() {
        return None;
    }
    Uuid::parse_str(id).ok()
}

/// Restore `final` from `*.old-*` leftovers after a crash between renames,
/// and drop leftover `.old-*` dirs when `final` already exists.
pub fn recover_subscription_dirs(paths: &SubscriptionPaths) {
    sweep_old_profile_dirs(paths, None);
}

fn sweep_old_profile_dirs(paths: &SubscriptionPaths, only: Option<Uuid>) {
    let _guard = COMMIT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    sweep_old_profile_dirs_locked(paths, only);
}

fn sweep_old_profile_dirs_locked(paths: &SubscriptionPaths, only: Option<Uuid>) {
    let Ok(entries) = fs::read_dir(paths.root()) else {
        return;
    };
    let mut by_id: HashMap<Uuid, Vec<PathBuf>> = HashMap::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Some(id) = parse_old_profile_dir(name) else {
            continue;
        };
        if only.is_some_and(|want| want != id) {
            continue;
        }
        by_id.entry(id).or_default().push(entry.path());
    }
    for (id, mut olds) in by_id {
        olds.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
        let final_dir = paths.sub_dir(id);
        let mut rest = olds.as_slice();
        if !final_dir.exists() {
            if let Some((newest, tail)) = rest.split_first() {
                match fs::rename(newest, &final_dir) {
                    Ok(()) => {
                        tracing::warn!(
                            id = %id,
                            "restored subscription profile from leftover .old dir"
                        );
                    }
                    Err(err) => {
                        tracing::error!(
                            id = %id,
                            error = %err,
                            "failed to restore subscription profile from leftover .old dir"
                        );
                    }
                }
                rest = tail;
            }
        }
        for old in rest {
            let _ = fs::remove_dir_all(old);
        }
    }
}

/// Index mutation half of [`write_subscription_success`] (no `index.json`
/// write): deactivates other subscriptions when `meta.active`, then upserts
/// `meta`. Callers batching multiple updates must [`save_index`] once after.
pub fn apply_success_to_index(index: &mut SubscriptionIndex, meta: &SubscriptionMeta) {
    if meta.active {
        for item in &mut index.items {
            if item.id != meta.id {
                item.active = false;
            }
        }
    }
    if let Some(slot) = index.items.iter_mut().find(|m| m.id == meta.id) {
        *slot = meta.clone();
    } else {
        if meta.active {
            for item in &mut index.items {
                item.active = false;
            }
        }
        index.items.push(meta.clone());
    }
}

/// Success path: stage raw + profile + meta atomically, then update `index.json`.
pub fn write_subscription_success(
    paths: &SubscriptionPaths,
    meta: &SubscriptionMeta,
    raw_body: &str,
    profile: &NormalizedProfile,
) -> Result<(), SubscriptionError> {
    commit_staged_subscription(paths, meta.id, raw_body, profile, meta)?;
    let mut index = load_index(paths)?;
    apply_success_to_index(&mut index, meta);
    save_index(paths, &index)?;
    Ok(())
}

/// Stage + commit the subscription files without touching `index.json`.
/// Batch callers (e.g. `apply_all`) update the index once afterwards with
/// [`apply_success_to_index`] + a single [`save_index`].
pub fn commit_subscription_success(
    paths: &SubscriptionPaths,
    meta: &SubscriptionMeta,
    raw_body: &str,
    profile: &NormalizedProfile,
) -> Result<(), SubscriptionError> {
    commit_staged_subscription(paths, meta.id, raw_body, profile, meta)
}

pub fn save_index(
    paths: &SubscriptionPaths,
    index: &SubscriptionIndex,
) -> Result<(), SubscriptionError> {
    fs::create_dir_all(paths.root())?;
    write_json_atomic(&paths.index(), index).map_err(map_cfg)?;
    Ok(())
}

fn map_cfg(err: ConfigError) -> SubscriptionError {
    SubscriptionError::Io(std::io::Error::other(err.to_string()))
}

/// Error half of [`write_subscription_error`] (no `index.json` write): records
/// `last_error` on the in-memory index entry and refreshes the on-disk
/// `meta.json` best-effort. Returns whether the id existed in the index.
pub fn apply_error_to_index(
    paths: &SubscriptionPaths,
    index: &mut SubscriptionIndex,
    id: Uuid,
    last_error: String,
) -> bool {
    let Some(meta) = index.items.iter_mut().find(|m| m.id == id) else {
        return false;
    };
    meta.last_error = Some(last_error);
    let updated = meta.clone();
    if (paths.meta(id).exists() || paths.sub_dir(id).exists())
        && fs::create_dir_all(paths.sub_dir(id)).is_ok()
    {
        let _ = write_json_atomic(&paths.meta(id), &updated);
    }
    true
}

/// Clear a recorded error in an in-memory index + on-disk `meta.json`
/// (no `index.json` write). Returns the updated meta when an error was
/// actually cleared; `None` when there was nothing to clear or the id is
/// missing from the index.
pub fn clear_error_in_index(
    paths: &SubscriptionPaths,
    index: &mut SubscriptionIndex,
    id: Uuid,
) -> Option<SubscriptionMeta> {
    let meta = index.items.iter().find(|m| m.id == id)?.clone();
    meta.last_error.as_ref()?;
    let mut updated = meta;
    updated.last_error = None;
    if let Some(slot) = index.items.iter_mut().find(|m| m.id == id) {
        *slot = updated.clone();
    }
    if paths.meta(id).exists() {
        let _ = write_json_atomic(&paths.meta(id), &updated);
    }
    Some(updated)
}

/// Update failure: keep raw/nodes, only refresh `last_error` in meta + index.
pub fn write_subscription_error(
    paths: &SubscriptionPaths,
    id: Uuid,
    last_error: String,
) -> Result<(), SubscriptionError> {
    let mut index = load_index(paths)?;
    if !apply_error_to_index(paths, &mut index, id, last_error) {
        return Ok(());
    }
    save_index(paths, &index)?;
    Ok(())
}

/// Backward-compatible alias for recording a successful conditional refresh.
pub fn clear_subscription_error(
    paths: &SubscriptionPaths,
    id: Uuid,
) -> Result<SubscriptionMeta, SubscriptionError> {
    mark_subscription_refreshed(paths, id)
}

/// Record a successful conditional refresh in an in-memory index and on-disk meta.
/// The cached subscription content is unchanged, but the refresh timestamp and error state
/// reflect the successful HTTP response.
pub fn mark_refreshed_in_index(
    paths: &SubscriptionPaths,
    index: &mut SubscriptionIndex,
    id: Uuid,
) -> Option<SubscriptionMeta> {
    let meta = index.items.iter().find(|m| m.id == id)?.clone();
    let mut updated = meta;
    updated.last_error = None;
    updated.last_updated = Some(Utc::now());
    if let Some(slot) = index.items.iter_mut().find(|m| m.id == id) {
        *slot = updated.clone();
    }
    if paths.meta(id).exists() {
        let _ = write_json_atomic(&paths.meta(id), &updated);
    }
    Some(updated)
}

/// Record a successful conditional refresh, updating its timestamp and clearing any error.
pub fn mark_subscription_refreshed(
    paths: &SubscriptionPaths,
    id: Uuid,
) -> Result<SubscriptionMeta, SubscriptionError> {
    let mut index = load_index(paths)?;
    let updated = mark_refreshed_in_index(paths, &mut index, id)
        .ok_or_else(|| SubscriptionError::ParseFailed(format!("subscription {id} not found")))?;
    save_index(paths, &index)?;
    Ok(updated)
}

pub fn read_profile(
    paths: &SubscriptionPaths,
    id: Uuid,
) -> Result<NormalizedProfile, SubscriptionError> {
    let profile_path = paths.profile(id);
    if profile_path.exists() {
        let raw = fs::read_to_string(&profile_path)?;
        return Ok(serde_json::from_str(&raw)?);
    }
    let nodes_path = paths.nodes(id);
    if !nodes_path.exists() {
        if paths.meta(id).exists() {
            return Err(SubscriptionError::ParseFailed(format!(
                "subscription {id} is missing profile.json"
            )));
        }
        return Ok(NormalizedProfile::from_nodes_only(vec![]));
    }
    let raw = fs::read_to_string(&nodes_path)?;
    let nodes: Vec<NormalizedOutbound> = serde_json::from_str(&raw)?;
    Ok(NormalizedProfile::from_nodes_only(nodes))
}

pub fn read_nodes(
    paths: &SubscriptionPaths,
    id: Uuid,
) -> Result<Vec<NormalizedOutbound>, SubscriptionError> {
    Ok(read_profile(paths, id)?.nodes)
}

pub fn remove_subscription(paths: &SubscriptionPaths, id: Uuid) -> Result<(), SubscriptionError> {
    let dir = paths.sub_dir(id);
    if dir.exists() {
        fs::remove_dir_all(&dir)?;
    }
    let staging = paths.staging_dir(id);
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    let mut index = load_index(paths)?;
    index.items.retain(|m| m.id != id);
    save_index(paths, &index)?;
    Ok(())
}

pub fn set_active(
    paths: &SubscriptionPaths,
    id: Uuid,
    active: bool,
) -> Result<SubscriptionMeta, SubscriptionError> {
    let mut index = load_index(paths)?;
    let meta =
        index.items.iter_mut().find(|m| m.id == id).ok_or_else(|| {
            SubscriptionError::ParseFailed(format!("subscription {id} not found"))
        })?;
    if active {
        for item in &mut index.items {
            item.active = item.id == id;
        }
    } else {
        meta.active = false;
    }
    let updated = index.items.iter().find(|m| m.id == id).unwrap().clone();
    write_json_atomic(&paths.meta(id), &updated).map_err(map_cfg)?;
    save_index(paths, &index)?;
    Ok(updated)
}

/// Deprecated alias for `set_active`.
pub fn set_enabled(
    paths: &SubscriptionPaths,
    id: Uuid,
    enabled: bool,
) -> Result<SubscriptionMeta, SubscriptionError> {
    set_active(paths, id, enabled)
}

/// Flip the background auto-update flag and its refresh cadence for a
/// subscription, persisting both to `index.json` and the on-disk `meta.json`.
pub fn set_auto_update(
    paths: &SubscriptionPaths,
    id: Uuid,
    auto_update: bool,
    auto_update_interval: Option<crate::AutoUpdateInterval>,
) -> Result<SubscriptionMeta, SubscriptionError> {
    let mut index = load_index(paths)?;
    let meta =
        index.items.iter_mut().find(|m| m.id == id).ok_or_else(|| {
            SubscriptionError::ParseFailed(format!("subscription {id} not found"))
        })?;
    meta.auto_update = auto_update;
    if auto_update_interval.is_some() {
        meta.auto_update_interval = auto_update_interval;
    }
    let updated = index.items.iter().find(|m| m.id == id).unwrap().clone();
    write_json_atomic(&paths.meta(id), &updated).map_err(map_cfg)?;
    save_index(paths, &index)?;
    Ok(updated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn read_nodes_errors_when_meta_exists_without_nodes_file() {
        let dir = std::env::temp_dir().join(format!(
            "ice-box-store-missing-nodes-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let paths = SubscriptionPaths::from_root(&dir);
        let id = Uuid::new_v4();
        std::fs::create_dir_all(paths.sub_dir(id)).unwrap();
        std::fs::write(paths.meta(id), b"{}").unwrap();

        let err = read_profile(&paths, id).expect_err("missing profile");
        assert!(err.to_string().contains("missing profile.json"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_nodes_returns_empty_when_subscription_dir_missing() {
        let dir = std::env::temp_dir().join(format!(
            "ice-box-store-no-dir-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let paths = SubscriptionPaths::from_root(&dir);
        let nodes = read_nodes(&paths, Uuid::new_v4()).unwrap();
        assert!(nodes.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_subscription_success_is_atomic_on_disk() {
        let dir = std::env::temp_dir().join(format!(
            "ice-box-store-atomic-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let paths = SubscriptionPaths::from_root(&dir);
        let id = Uuid::new_v4();
        let meta = SubscriptionMeta {
            id,
            name: "t".into(),
            url: "https://example.com/s".into(),
            active: true,
            format: crate::SubscriptionFormat::SingBox,
            node_count: 1,
            group_count: 0,
            rule_count: 0,
            has_dns: false,
            parse_warnings: vec![],
            last_updated: None,
            last_error: None,
            etag: None,
            last_modified: None,
            auto_update: false,
            auto_update_interval: None,
        };
        let profile = NormalizedProfile::from_nodes_only(vec![NormalizedOutbound {
            tag: "n1".into(),
            outbound: serde_json::json!({"type":"direct","tag":"n1"}),
        }]);
        write_subscription_success(&paths, &meta, "{}", &profile).unwrap();
        assert!(paths.raw(id).is_file());
        assert!(
            !paths.nodes(id).exists(),
            "nodes.json is a legacy duplicate and is no longer written"
        );
        assert!(paths.profile(id).is_file());
        assert!(paths.meta(id).is_file());
        assert!(!paths.staging_dir(id).exists());
        let index = load_index(&paths).unwrap();
        assert_eq!(index.items.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn recover_restores_profile_when_crash_leaves_only_old_dir() {
        let dir = std::env::temp_dir().join(format!(
            "ice-box-store-old-recover-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let paths = SubscriptionPaths::from_root(&dir);
        let id = Uuid::new_v4();
        let meta = SubscriptionMeta {
            id,
            name: "t".into(),
            url: "https://example.com/s".into(),
            active: true,
            format: crate::SubscriptionFormat::SingBox,
            node_count: 1,
            group_count: 0,
            rule_count: 0,
            has_dns: false,
            parse_warnings: vec![],
            last_updated: None,
            last_error: None,
            etag: None,
            last_modified: None,
            auto_update: false,
            auto_update_interval: None,
        };
        let profile = NormalizedProfile::from_nodes_only(vec![NormalizedOutbound {
            tag: "kept".into(),
            outbound: serde_json::json!({"type":"direct","tag":"kept"}),
        }]);
        write_subscription_success(&paths, &meta, "{}", &profile).unwrap();

        let final_dir = paths.sub_dir(id);
        let old = paths.root().join(format!("{id}.old-crash"));
        std::fs::rename(&final_dir, &old).unwrap();
        assert!(!final_dir.exists(), "crash window: final is gone");
        assert!(old.exists());

        recover_subscription_dirs(&paths);
        let loaded = read_profile(&paths, id).expect("profile restored from .old");
        assert_eq!(loaded.nodes[0].tag, "kept");
        assert!(!old.exists(), "restored .old dir is consumed");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn recover_uses_old_dir_when_crash_aborts_between_renames() {
        let dir = std::env::temp_dir().join(format!(
            "ice-box-store-mid-rename-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let paths = SubscriptionPaths::from_root(&dir);
        let id = Uuid::new_v4();
        let meta = SubscriptionMeta {
            id,
            name: "t".into(),
            url: "https://example.com/s".into(),
            active: true,
            format: crate::SubscriptionFormat::SingBox,
            node_count: 1,
            group_count: 0,
            rule_count: 0,
            has_dns: false,
            parse_warnings: vec![],
            last_updated: None,
            last_error: None,
            etag: None,
            last_modified: None,
            auto_update: false,
            auto_update_interval: None,
        };
        let kept = NormalizedProfile::from_nodes_only(vec![NormalizedOutbound {
            tag: "kept".into(),
            outbound: serde_json::json!({"type":"direct","tag":"kept"}),
        }]);
        write_subscription_success(&paths, &meta, "{}", &kept).unwrap();

        let final_dir = paths.sub_dir(id);
        let old = paths.root().join(format!("{id}.old-mid-rename"));
        std::fs::rename(&final_dir, &old).unwrap();

        // Second rename (staging → final) never happened: leftover staging
        // must not be preferred over the `.old` snapshot.
        let staging = paths.staging_dir(id);
        std::fs::create_dir_all(&staging).unwrap();
        let staging_profile = NormalizedProfile::from_nodes_only(vec![NormalizedOutbound {
            tag: "from-staging".into(),
            outbound: serde_json::json!({"type":"direct","tag":"from-staging"}),
        }]);
        std::fs::write(
            staging.join("profile.json"),
            serde_json::to_vec(&staging_profile).unwrap(),
        )
        .unwrap();

        recover_subscription_dirs(&paths);
        let loaded = read_profile(&paths, id).expect("profile restored from .old");
        assert_eq!(loaded.nodes[0].tag, "kept");
        assert!(!old.exists(), "restored .old dir is consumed");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
