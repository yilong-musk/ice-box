//! Disk layout under `subscriptions/` (architecture §6).

use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use ice_config::{
    write_bytes_atomic, write_json_atomic, AppPaths, ConfigError, NormalizedOutbound,
    NormalizedProfile,
};
use uuid::Uuid;

use crate::error::SubscriptionError;
use crate::{SubscriptionIndex, SubscriptionMeta};

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
        return Ok(SubscriptionIndex::default());
    }
    let raw = fs::read_to_string(&path)?;
    let mut index: SubscriptionIndex = serde_json::from_str(&raw)?;
    migrate_index_active(&mut index);
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
        fs::remove_dir_all(&final_dir)?;
    }
    fs::rename(&staging, &final_dir).map_err(SubscriptionError::Io)?;
    Ok(())
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
}
