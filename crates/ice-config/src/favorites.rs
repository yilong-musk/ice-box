// SPDX-License-Identifier: GPL-3.0-or-later

//! Persisted per-subscription node favorites (`node-favorites.json`).
//!
//! Favorites are local UI preferences, not subscription parsing state. Each
//! subscription owns an ordered tag list, newest favorite first. Stale tags
//! stay on disk when a provider refresh drops them, so the pin returns if the
//! same tag reappears. Reads and writes are serialized by a module-level
//! mutex; the atomic write protects readers from partial files.

use crate::atomic::write_json_atomic;
use crate::{AppError, ErrorCode};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::Mutex;
use uuid::Uuid;

/// Newest favorites kept per subscription. Older tags are dropped from the
/// tail when a new favorite pushes the list past the cap.
pub const MAX_FAVORITES_PER_SUBSCRIPTION: usize = 500;

const FAVORITES_VERSION: u32 = 1;

/// Serializes every read-modify-write, including callers that do not hold the
/// application orchestration lock.
static FAVORITES_LOCK: Mutex<()> = Mutex::new(());

/// Per-subscription favorite tags, most recently favorited first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NodeFavorites {
    subscriptions: BTreeMap<Uuid, Vec<String>>,
}

impl NodeFavorites {
    pub fn contains(&self, subscription: Uuid, tag: &str) -> bool {
        self.favorite_rank(subscription, tag).is_some()
    }

    /// Position in the subscription's list (`0` = newest); `None` when the tag
    /// is not favorited.
    pub fn favorite_rank(&self, subscription: Uuid, tag: &str) -> Option<usize> {
        self.subscriptions
            .get(&subscription)?
            .iter()
            .position(|candidate| candidate == tag)
    }

    /// Ordered tags for one subscription, newest first.
    pub fn tags(&self, subscription: Uuid) -> &[String] {
        self.subscriptions
            .get(&subscription)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
}

/// Load favorites. A missing file is empty; a corrupt wrapper degrades to
/// empty. Invalid subscription keys and malformed entries are skipped without
/// discarding the rest of the file.
pub fn load_node_favorites(path: &Path) -> NodeFavorites {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return NodeFavorites::default();
        }
        Err(err) => {
            tracing::warn!(path = %path.display(), error = %err, "load node favorites");
            return NodeFavorites::default();
        }
    };
    parse_favorites(&raw)
}

fn parse_favorites(raw: &str) -> NodeFavorites {
    let value: Value = match serde_json::from_str(raw) {
        Ok(value) => value,
        Err(err) => {
            tracing::warn!(error = %err, "node favorites: corrupt file treated as empty");
            return NodeFavorites::default();
        }
    };
    let Some(subscriptions) = value.get("subscriptions").and_then(Value::as_object) else {
        tracing::warn!("node favorites: missing subscriptions object treated as empty");
        return NodeFavorites::default();
    };
    let mut favorites = NodeFavorites::default();
    for (key, value) in subscriptions {
        let Ok(subscription) = Uuid::parse_str(key) else {
            tracing::warn!(key, "node favorites: invalid subscription id skipped");
            continue;
        };
        let Some(tags) = value.as_array() else {
            tracing::warn!(key, "node favorites: malformed subscription value skipped");
            continue;
        };
        let mut cleaned: Vec<String> =
            Vec::with_capacity(tags.len().min(MAX_FAVORITES_PER_SUBSCRIPTION));
        for tag in tags {
            let Some(tag) = tag.as_str() else {
                tracing::warn!(key, "node favorites: non-string tag skipped");
                continue;
            };
            if cleaned.iter().any(|kept| kept == tag) {
                continue;
            }
            if cleaned.len() >= MAX_FAVORITES_PER_SUBSCRIPTION {
                break;
            }
            cleaned.push(tag.to_string());
        }
        if !cleaned.is_empty() {
            favorites.subscriptions.insert(subscription, cleaned);
        }
    }
    favorites
}

#[derive(Serialize)]
struct FavoritesFile<'a> {
    version: u32,
    subscriptions: &'a BTreeMap<Uuid, Vec<String>>,
}

fn save_node_favorites(path: &Path, favorites: &NodeFavorites) -> Result<(), AppError> {
    let file = FavoritesFile {
        version: FAVORITES_VERSION,
        subscriptions: &favorites.subscriptions,
    };
    write_json_atomic(path, &file).map_err(|err| {
        AppError::new(
            ErrorCode::ConfigInvalid,
            format!("save node favorites: {err}"),
        )
    })
}

/// Favorite or unfavorite `tag` for one subscription.
///
/// Favoriting moves the tag to the front (most recent) and applies the
/// per-subscription cap. Unfavoriting an absent tag is a no-op.
pub fn set_node_favorite(
    path: &Path,
    subscription: Uuid,
    tag: &str,
    favorite: bool,
) -> Result<(), AppError> {
    let _guard = FAVORITES_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut favorites = load_node_favorites(path);
    let previous = favorites.subscriptions.get(&subscription).cloned();
    if favorite {
        let mut tags = favorites
            .subscriptions
            .remove(&subscription)
            .unwrap_or_default();
        tags.retain(|candidate| candidate != tag);
        tags.insert(0, tag.to_string());
        tags.truncate(MAX_FAVORITES_PER_SUBSCRIPTION);
        favorites.subscriptions.insert(subscription, tags);
    } else {
        let Some(tags) = favorites.subscriptions.get_mut(&subscription) else {
            return Ok(());
        };
        tags.retain(|candidate| candidate != tag);
        if tags.is_empty() {
            favorites.subscriptions.remove(&subscription);
        }
    }
    if favorites.subscriptions.get(&subscription) == previous.as_ref() {
        return Ok(());
    }
    save_node_favorites(path, &favorites)
}

/// Drop a removed subscription's favorites. Called while the caller still
/// holds its lifecycle lock, before the later apply step.
pub fn remove_subscription_favorites(path: &Path, subscription: Uuid) -> Result<(), AppError> {
    let _guard = FAVORITES_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut favorites = load_node_favorites(path);
    if favorites.subscriptions.remove(&subscription).is_none() {
        return Ok(());
    }
    save_node_favorites(path, &favorites)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_path(label: &str) -> std::path::PathBuf {
        std::env::temp_dir()
            .join(format!(
                "ice-box-favorites-{label}-{}",
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ))
            .join("node-favorites.json")
    }

    #[test]
    fn missing_and_corrupt_files_load_empty() {
        let path = temp_path("missing");
        assert_eq!(load_node_favorites(&path), NodeFavorites::default());

        let path = temp_path("corrupt");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "{not json").unwrap();
        assert_eq!(load_node_favorites(&path), NodeFavorites::default());
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn versioned_round_trip_writes_the_wrapper_shape() {
        let path = temp_path("roundtrip");
        let id = Uuid::new_v4();
        set_node_favorite(&path, id, "Tokyo", true).unwrap();
        set_node_favorite(&path, id, "Auto", true).unwrap();

        let raw: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(raw["version"], json!(1));
        assert_eq!(
            raw["subscriptions"][id.to_string()],
            json!(["Auto", "Tokyo"])
        );

        let loaded = load_node_favorites(&path);
        assert!(loaded.contains(id, "Auto"));
        assert_eq!(loaded.favorite_rank(id, "Auto"), Some(0));
        assert_eq!(loaded.favorite_rank(id, "Tokyo"), Some(1));
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn invalid_entries_are_skipped_individually() {
        let path = temp_path("invalid");
        let id = Uuid::new_v4();
        let other = Uuid::new_v4();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            serde_json::to_string(&json!({
                "version": 1,
                "unknown": "ignored",
                "subscriptions": {
                    "not-a-uuid": ["skipped"],
                    id.to_string(): ["A", 42, "B", "A"],
                    other.to_string(): "not a list",
                }
            }))
            .unwrap(),
        )
        .unwrap();

        let loaded = load_node_favorites(&path);
        assert_eq!(loaded.tags(id), ["A", "B"]);
        assert!(loaded.tags(other).is_empty());
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn loading_deduplicates_and_caps_the_oldest_tags() {
        let path = temp_path("cap");
        let id = Uuid::new_v4();
        let tags: Vec<String> = (0..MAX_FAVORITES_PER_SUBSCRIPTION + 5)
            .map(|index| format!("node-{index}"))
            .collect();
        let mut with_duplicate = tags.clone();
        with_duplicate.insert(1, "node-0".into());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            serde_json::to_string(&json!({
                "version": 1,
                "subscriptions": { id.to_string(): with_duplicate }
            }))
            .unwrap(),
        )
        .unwrap();

        let loaded = load_node_favorites(&path);
        let loaded_tags = loaded.tags(id);
        assert_eq!(loaded_tags.len(), MAX_FAVORITES_PER_SUBSCRIPTION);
        assert_eq!(loaded_tags.first().map(String::as_str), Some("node-0"));
        assert_eq!(loaded_tags.last().map(String::as_str), Some("node-499"));
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn toggling_moves_to_front_and_clears_empty_subscriptions() {
        let path = temp_path("toggle");
        let id = Uuid::new_v4();
        set_node_favorite(&path, id, "A", true).unwrap();
        set_node_favorite(&path, id, "B", true).unwrap();
        assert_eq!(load_node_favorites(&path).tags(id), ["B", "A"]);

        set_node_favorite(&path, id, "A", true).unwrap();
        assert_eq!(load_node_favorites(&path).tags(id), ["A", "B"]);

        set_node_favorite(&path, id, "A", false).unwrap();
        assert_eq!(load_node_favorites(&path).tags(id), ["B"]);

        set_node_favorite(&path, id, "B", false).unwrap();
        let loaded = load_node_favorites(&path);
        assert!(loaded.tags(id).is_empty());
        let raw: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert!(raw["subscriptions"].as_object().unwrap().is_empty());
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn removing_an_absent_favorite_is_a_no_op() {
        let path = temp_path("absent");
        let id = Uuid::new_v4();
        set_node_favorite(&path, id, "A", true).unwrap();
        let before = fs::read_to_string(&path).unwrap();
        set_node_favorite(&path, id, "missing", false).unwrap();
        set_node_favorite(&path, Uuid::new_v4(), "missing", false).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), before);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn subscriptions_stay_isolated() {
        let path = temp_path("isolated");
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        set_node_favorite(&path, first, "A", true).unwrap();
        set_node_favorite(&path, second, "B", true).unwrap();
        let loaded = load_node_favorites(&path);
        assert_eq!(loaded.tags(first), ["A"]);
        assert_eq!(loaded.tags(second), ["B"]);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn ranks_are_newest_first_and_stable_across_reload() {
        let path = temp_path("ranks");
        let id = Uuid::new_v4();
        for tag in ["A", "B", "C"] {
            set_node_favorite(&path, id, tag, true).unwrap();
        }
        let loaded = load_node_favorites(&path);
        assert_eq!(loaded.favorite_rank(id, "C"), Some(0));
        assert_eq!(loaded.favorite_rank(id, "B"), Some(1));
        assert_eq!(loaded.favorite_rank(id, "A"), Some(2));
        assert_eq!(loaded.favorite_rank(id, "missing"), None);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn remove_subscription_purges_only_that_key() {
        let path = temp_path("purge");
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        set_node_favorite(&path, first, "A", true).unwrap();
        set_node_favorite(&path, second, "B", true).unwrap();

        remove_subscription_favorites(&path, first).unwrap();
        let loaded = load_node_favorites(&path);
        assert!(loaded.tags(first).is_empty());
        assert_eq!(loaded.tags(second), ["B"]);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn concurrent_writes_do_not_lose_updates() {
        let path = temp_path("concurrent");
        let id = Uuid::new_v4();
        let path = std::sync::Arc::new(path);
        let handles: Vec<_> = (0..8)
            .map(|index| {
                let path = std::sync::Arc::clone(&path);
                std::thread::spawn(move || {
                    set_node_favorite(&path, id, &format!("node-{index}"), true).unwrap();
                })
            })
            .collect();
        for handle in handles {
            handle.join().unwrap();
        }
        assert_eq!(
            load_node_favorites(&path).tags(id).len(),
            8,
            "every write must survive"
        );
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
}
