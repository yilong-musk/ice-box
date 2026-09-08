// SPDX-License-Identifier: GPL-3.0-or-later

//! Disk-backed subscription manager and the in-memory placeholder.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use ice_config::{HostPlatform, NormalizedProfile};
use uuid::Uuid;

use crate::error::SubscriptionError;
use crate::fetch::{DirectFetcher, FetchResponse, HttpFetcher};
use crate::merge::load_active_profile;
use crate::store::{
    apply_error_to_index, apply_success_to_index, commit_subscription_success, load_index,
    mark_refreshed_in_index, mark_subscription_refreshed, remove_subscription, save_index,
    set_active, set_auto_update, write_subscription_error, write_subscription_success,
    SubscriptionPaths,
};
use crate::url::validate_subscription_url;
use crate::{
    meta_from_fetched_profile, meta_from_profile, normalize_raw_body, resolve_subscription_name,
    AutoUpdateInterval, SubscriptionFormat, SubscriptionIndex, SubscriptionMeta,
    MAX_FETCH_CONCURRENCY,
};

/// Disk-backed subscription manager.
pub struct SubscriptionManager<F: HttpFetcher = DirectFetcher> {
    paths: SubscriptionPaths,
    fetcher: F,
    platform: HostPlatform,
    /// False when a fetch worker exited because the result channel closed
    /// (SUB-4). The desktop watchdog inspects this and respawns.
    worker_alive: Arc<AtomicBool>,
}

/// Result of the network phase of an update; the disk phase consumes it via
/// `SubscriptionManager::apply_update`.
#[derive(Debug, Clone)]
pub struct FetchedUpdate {
    pub meta: SubscriptionMeta,
    pub fetched: FetchResponse,
}

/// Result of the network phase of an add; the disk phase consumes it via
/// `SubscriptionManager::apply_add`.
#[derive(Debug)]
pub struct FetchedAdd {
    pub id: Uuid,
    pub url: String,
    pub name: Option<String>,
    pub auto_update: bool,
    pub auto_update_interval: Option<AutoUpdateInterval>,
    pub fetched: FetchResponse,
}

impl SubscriptionManager<DirectFetcher> {
    pub fn open(paths: SubscriptionPaths, platform: HostPlatform) -> Self {
        Self {
            paths,
            fetcher: DirectFetcher,
            platform,
            worker_alive: Arc::new(AtomicBool::new(true)),
        }
    }
}

impl<F: HttpFetcher> SubscriptionManager<F> {
    pub fn with_fetcher(paths: SubscriptionPaths, fetcher: F) -> Self {
        Self::with_fetcher_on(paths, fetcher, HostPlatform::MacOs)
    }

    pub fn with_fetcher_on(paths: SubscriptionPaths, fetcher: F, platform: HostPlatform) -> Self {
        Self {
            paths,
            fetcher,
            platform,
            worker_alive: Arc::new(AtomicBool::new(true)),
        }
    }

    /// Whether the last [`Self::fetch_ids`] worker pool is still considered live
    /// (SUB-4). A closed result channel flips this to false.
    pub fn fetch_workers_alive(&self) -> bool {
        self.worker_alive.load(Ordering::SeqCst)
    }

    pub fn paths(&self) -> &SubscriptionPaths {
        &self.paths
    }

    pub fn list(&self) -> Result<Vec<SubscriptionMeta>, SubscriptionError> {
        Ok(load_index(&self.paths)?.items)
    }

    /// Import URL: fetch → parse → write success files. Empty/unknown → no success write.
    pub fn add(
        &self,
        url: &str,
        name: Option<&str>,
        auto_update: bool,
        auto_update_interval: Option<AutoUpdateInterval>,
    ) -> Result<SubscriptionMeta, SubscriptionError> {
        self.apply_add(self.fetch_add(url, name, auto_update, auto_update_interval)?)
    }

    /// Network fetch phase of an add: validate URL, GET (no disk writes).
    pub fn fetch_add(
        &self,
        url: &str,
        name: Option<&str>,
        auto_update: bool,
        auto_update_interval: Option<AutoUpdateInterval>,
    ) -> Result<FetchedAdd, SubscriptionError> {
        assert!(
            self.fetcher.bypasses_system_proxy(),
            "subscription fetch must bypass system proxy"
        );

        validate_subscription_url(url)?;

        let id = Uuid::new_v4();
        let fetched = self.fetcher.get(url, None, None)?;
        Ok(FetchedAdd {
            id,
            url: url.to_string(),
            name: name.map(str::to_string),
            auto_update,
            auto_update_interval,
            fetched,
        })
    }

    /// Disk phase of an add: normalize + persist (or leave no success artifacts on failure).
    pub fn apply_add(&self, add: FetchedAdd) -> Result<SubscriptionMeta, SubscriptionError> {
        let FetchedAdd {
            id,
            url,
            name,
            auto_update,
            auto_update_interval,
            fetched,
        } = add;
        match normalize_raw_body(&fetched.body, self.platform) {
            Ok((format, profile)) => {
                let index = load_index(&self.paths)?;
                let make_active = index.items.iter().all(|m| !m.active);
                let meta = meta_from_profile(
                    id,
                    resolve_subscription_name(
                        name.as_deref(),
                        fetched.content_disposition.as_deref(),
                        &url,
                        id,
                    ),
                    url,
                    format,
                    &profile,
                    make_active,
                    fetched.etag,
                    fetched.last_modified,
                    auto_update,
                    auto_update_interval,
                );
                write_subscription_success(&self.paths, &meta, &fetched.body, &profile)?;
                Ok(meta)
            }
            Err(err) => {
                // Do not create success artifacts; leave no index entry.
                Err(err)
            }
        }
    }

    pub fn update(&self, id: Uuid) -> Result<SubscriptionMeta, SubscriptionError> {
        match self.fetch_update(id) {
            Ok(upd) => self.apply_update(upd),
            Err(err) => {
                write_subscription_error(&self.paths, id, err.ui_message())?;
                Err(err)
            }
        }
    }

    /// Network fetch phase of an update: load meta, validate URL, GET (no disk writes).
    pub fn fetch_update(&self, id: Uuid) -> Result<FetchedUpdate, SubscriptionError> {
        assert!(self.fetcher.bypasses_system_proxy());

        let index = load_index(&self.paths)?;
        let meta = index
            .items
            .iter()
            .find(|m| m.id == id)
            .cloned()
            .ok_or_else(|| {
                SubscriptionError::ParseFailed(format!("subscription {id} not found"))
            })?;

        validate_subscription_url(&meta.url)?;

        let fetched = self.fetcher.get(
            &meta.url,
            meta.etag.as_deref(),
            meta.last_modified.as_deref(),
        )?;
        Ok(FetchedUpdate { meta, fetched })
    }

    /// Disk phase of an update: normalize + persist, or record `last_error`.
    pub fn apply_update(&self, upd: FetchedUpdate) -> Result<SubscriptionMeta, SubscriptionError> {
        // A subscription removed while its fetch was in flight must not be resurrected:
        // the index no longer contains the id, so stop before writing any files.
        let current = load_index(&self.paths)?
            .items
            .iter()
            .find(|m| m.id == upd.meta.id)
            .cloned()
            .ok_or_else(|| {
                SubscriptionError::ParseFailed(format!("subscription {} not found", upd.meta.id))
            })?;
        if upd.fetched.not_modified {
            return mark_subscription_refreshed(&self.paths, upd.meta.id);
        }
        match normalize_raw_body(&upd.fetched.body, self.platform) {
            Ok((format, profile)) => {
                let updated = meta_from_fetched_profile(&current, &upd.fetched, format, &profile);
                write_subscription_success(&self.paths, &updated, &upd.fetched.body, &profile)?;
                Ok(updated)
            }
            Err(err) => {
                write_subscription_error(&self.paths, upd.meta.id, err.ui_message())?;
                Err(err)
            }
        }
    }

    /// Network phase of updating every subscription. Fetches run in parallel (up to one
    /// `FETCH_TIMEOUT` of wall time instead of N×) and write nothing to disk, so the caller
    /// can run them without holding the orchestrate lock.
    pub fn fetch_all(&self) -> Vec<(Uuid, Result<FetchedUpdate, SubscriptionError>)>
    where
        F: Sync,
    {
        let ids: Vec<Uuid> = load_index(&self.paths)
            .map(|i| i.items.into_iter().map(|m| m.id).collect())
            .unwrap_or_default();
        self.fetch_ids(ids)
    }

    /// Network phase of updating the subscriptions with `auto_update` enabled. Same parallel
    /// fetch as [`SubscriptionManager::fetch_all`], but only touches the flagged entries so a
    /// background refresh never re-fetches subscriptions the user did not opt into.
    pub fn fetch_auto(&self) -> Vec<(Uuid, Result<FetchedUpdate, SubscriptionError>)>
    where
        F: Sync,
    {
        let ids: Vec<Uuid> = load_index(&self.paths)
            .map(|i| {
                i.items
                    .into_iter()
                    .filter(|m| m.auto_update)
                    .map(|m| m.id)
                    .collect()
            })
            .unwrap_or_default();
        self.fetch_ids(ids)
    }

    /// Parallel network phase for an explicit id list. Writes nothing to disk, so callers
    /// can run it without holding the orchestrate lock; [`SubscriptionManager::apply_all`]
    /// persists the results serially.
    pub fn fetch_ids(&self, ids: Vec<Uuid>) -> Vec<(Uuid, Result<FetchedUpdate, SubscriptionError>)>
    where
        F: Sync,
    {
        if ids.is_empty() {
            return Vec::new();
        }

        self.worker_alive.store(true, Ordering::SeqCst);
        let worker_alive = Arc::clone(&self.worker_alive);
        let worker_count = ids.len().min(MAX_FETCH_CONCURRENCY);
        let queue = std::sync::Arc::new(std::sync::Mutex::new(
            ids.into_iter()
                .enumerate()
                .collect::<std::collections::VecDeque<_>>(),
        ));
        let (sender, receiver) = std::sync::mpsc::channel();

        std::thread::scope(|scope| {
            for _ in 0..worker_count {
                let queue = std::sync::Arc::clone(&queue);
                let sender = sender.clone();
                let worker_alive = Arc::clone(&worker_alive);
                scope.spawn(move || loop {
                    let job = queue.lock().unwrap_or_else(|e| e.into_inner()).pop_front();
                    let Some((index, id)) = job else {
                        break;
                    };
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        self.fetch_update(id)
                    }))
                    .unwrap_or_else(|_| {
                        Err(SubscriptionError::FetchFailed(
                            "fetch worker panicked".into(),
                        ))
                    });
                    if sender.send((index, id, result)).is_err() {
                        worker_alive.store(false, Ordering::SeqCst);
                        tracing::error!("subscription fetch receiver dropped; worker exiting");
                        break;
                    }
                });
            }
            drop(sender);
        });

        let mut completed: Vec<_> = receiver.into_iter().collect();
        completed.sort_unstable_by_key(|(index, _, _)| *index);
        completed
            .into_iter()
            .map(|(_, id, result)| (id, result))
            .collect()
    }

    /// Disk phase of [`SubscriptionManager::fetch_all`]: persists each fetched update
    /// serially so `index.json` writes never interleave. Under the orchestrate lock this
    /// cannot race with add/remove/set_active. The index is loaded once and written once
    /// (per-item updates used to re-read and fsync `index.json` twice per subscription).
    pub fn apply_all(
        &self,
        fetched: Vec<(Uuid, Result<FetchedUpdate, SubscriptionError>)>,
    ) -> Vec<(Uuid, Result<SubscriptionMeta, SubscriptionError>)> {
        let mut index = match load_index(&self.paths) {
            Ok(index) => index,
            Err(err) => {
                let message = err.to_string();
                return fetched
                    .into_iter()
                    .map(|(id, _)| (id, Err(SubscriptionError::ParseFailed(message.clone()))))
                    .collect();
            }
        };
        let mut out = Vec::with_capacity(fetched.len());
        for (id, result) in fetched {
            match result {
                Err(err) => {
                    apply_error_to_index(&self.paths, &mut index, id, err.ui_message());
                    out.push((id, Err(err)));
                }
                Ok(upd) => {
                    // A subscription removed while its fetch was in flight must not be
                    // resurrected (same guard as apply_update).
                    let Some(current) = index.items.iter().find(|m| m.id == id).cloned() else {
                        out.push((
                            id,
                            Err(SubscriptionError::ParseFailed(format!(
                                "subscription {id} not found"
                            ))),
                        ));
                        continue;
                    };
                    if upd.fetched.not_modified {
                        let updated =
                            mark_refreshed_in_index(&self.paths, &mut index, id).unwrap_or(current);
                        out.push((id, Ok(updated)));
                        continue;
                    }
                    match normalize_raw_body(&upd.fetched.body, self.platform) {
                        Ok((format, profile)) => {
                            let updated =
                                meta_from_fetched_profile(&current, &upd.fetched, format, &profile);
                            if let Err(err) = commit_subscription_success(
                                &self.paths,
                                &updated,
                                &upd.fetched.body,
                                &profile,
                            ) {
                                out.push((id, Err(err)));
                                continue;
                            }
                            apply_success_to_index(&mut index, &updated);
                            out.push((id, Ok(updated)));
                        }
                        Err(err) => {
                            apply_error_to_index(&self.paths, &mut index, id, err.ui_message());
                            out.push((id, Err(err)));
                        }
                    }
                }
            }
        }
        if let Err(err) = save_index(&self.paths, &index) {
            // The index write is the single commit point of the batch; surface
            // the failure on every item so the caller cannot treat the batch
            // as fully persisted.
            let message = err.to_string();
            for (_, item) in out.iter_mut() {
                *item = Err(SubscriptionError::ParseFailed(message.clone()));
            }
        }
        out
    }

    /// Update every subscription. Network fetches run in parallel, disk writes stay
    /// serialized so `index.json` updates never interleave.
    pub fn update_all(&self) -> Vec<(Uuid, Result<SubscriptionMeta, SubscriptionError>)>
    where
        F: Sync,
    {
        self.apply_all(self.fetch_all())
    }

    pub fn remove(&self, id: Uuid) -> Result<(), SubscriptionError> {
        remove_subscription(&self.paths, id)
    }

    pub fn set_active(
        &self,
        id: Uuid,
        active: bool,
    ) -> Result<SubscriptionMeta, SubscriptionError> {
        set_active(&self.paths, id, active)
    }

    pub fn set_auto_update(
        &self,
        id: Uuid,
        auto_update: bool,
        auto_update_interval: Option<AutoUpdateInterval>,
    ) -> Result<SubscriptionMeta, SubscriptionError> {
        set_auto_update(&self.paths, id, auto_update, auto_update_interval)
    }

    pub fn active_profile(&self) -> Result<Arc<NormalizedProfile>, SubscriptionError> {
        let index = load_index(&self.paths)?;
        load_active_profile(&self.paths, &index, self.platform)
    }
}

/// In-memory placeholder kept for early shell wiring (prefer `SubscriptionManager::open`).
#[derive(Debug, Default)]
pub struct MemorySubscriptionManager {
    index: SubscriptionIndex,
}

impl MemorySubscriptionManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn list(&self) -> &[SubscriptionMeta] {
        &self.index.items
    }

    pub fn add_placeholder(&mut self, name: String, url: String) -> SubscriptionMeta {
        let meta = SubscriptionMeta {
            id: Uuid::new_v4(),
            name,
            url,
            active: false,
            format: SubscriptionFormat::Unknown,
            node_count: 0,
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
        self.index.items.push(meta.clone());
        meta
    }
}
