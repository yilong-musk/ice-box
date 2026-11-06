// SPDX-License-Identifier: GPL-3.0-or-later

//! Disk-backed subscription manager and the in-memory placeholder.

use std::io::{Read, Seek, SeekFrom, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use ice_config::{HostPlatform, NormalizedProfile};
use uuid::Uuid;

use crate::error::SubscriptionError;
use crate::fetch::{
    DefaultFetcher, DirectFetcher, FetchClient, FetchResponse, HttpFetcher, SubscriptionProxy,
};
use crate::merge::load_active_profile;
use crate::store::{
    apply_error_to_index, apply_success_to_index, commit_subscription_success, load_index,
    mark_refreshed_in_index, mark_subscription_refreshed, remove_subscription, save_index,
    set_active, set_auto_update, write_subscription_error, write_subscription_success,
    SubscriptionPaths,
};
use crate::url::{validate_subscription_url, validate_subscription_url_syntax};
use crate::{
    meta_from_fetched_profile, meta_from_profile, normalize_raw_body, resolve_subscription_name,
    AutoUpdateInterval, SubscriptionFormat, SubscriptionIndex, SubscriptionMeta,
    SubscriptionSource, MAX_BODY_BYTES, MAX_FETCH_CONCURRENCY,
};

/// How usable a sing-box client response is.
enum SingBoxAnswer {
    Usable,
    /// A sing-box config that parses but loses route rules to rule sets
    /// ice-box never downloads.
    Lossy,
    Unusable,
}

/// Whether a failed sing-box request is worth repeating as the generic
/// client. A 4xx can be a panel refusing an unknown client, and a rejected
/// body may be the sing-box config alone. 408 / 429 and 5xx are about the
/// server or the timing, not the client, so a retry would only add load and
/// wait.
fn retry_as_generic(err: &SubscriptionError) -> bool {
    match err {
        SubscriptionError::HttpStatus { status, .. } => {
            (400..500).contains(status) && !matches!(status, 408 | 429)
        }
        SubscriptionError::BodyRejected(_) => true,
        _ => false,
    }
}

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
    /// Client that produced `fetched`.
    pub fetched_as: FetchClient,
    pub(crate) spooled: Option<SpooledBody>,
}

/// All responses in a batch share one anonymous file, removed on final drop.
#[derive(Debug, Clone)]
pub(crate) struct SpooledBody {
    file: Arc<Mutex<std::fs::File>>,
    offset: u64,
    len: u64,
}

impl FetchedUpdate {
    fn spool(
        mut self,
        file: &mut Option<Arc<Mutex<std::fs::File>>>,
    ) -> Result<Self, SubscriptionError> {
        if self.fetched.body.is_empty() {
            return Ok(self);
        }
        if file.is_none() {
            *file = Some(Arc::new(Mutex::new(tempfile::tempfile()?)));
        }
        let file = file.as_ref().expect("spool initialized");
        let mut writer = file.lock().unwrap_or_else(|e| e.into_inner());
        let offset = writer.seek(SeekFrom::End(0))?;
        writer.write_all(self.fetched.body.as_bytes())?;
        self.spooled = Some(SpooledBody {
            file: Arc::clone(file),
            offset,
            len: self.fetched.body.len() as u64,
        });
        // Replacing, rather than clearing, releases the response allocation.
        self.fetched.body = String::new();
        Ok(self)
    }

    fn materialize(mut self) -> Result<Self, SubscriptionError> {
        if let Some(body) = self.spooled.take() {
            let mut reader = body.file.lock().unwrap_or_else(|e| e.into_inner());
            reader.seek(SeekFrom::Start(body.offset))?;
            (&mut *reader)
                .take(body.len)
                .read_to_string(&mut self.fetched.body)?;
            if self.fetched.body.len() as u64 != body.len {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "subscription spool truncated",
                )
                .into());
            }
        }
        Ok(self)
    }
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
    pub source: SubscriptionSource,
    pub fetched: FetchResponse,
    /// Client that produced `fetched`; `None` for a file import.
    pub fetched_as: Option<FetchClient>,
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

impl SubscriptionManager<DefaultFetcher> {
    /// Open a manager whose fetches follow the app's default network path:
    /// through the local mixed inbound when `proxy_url` is `Some`, direct
    /// otherwise.
    pub fn open_with_proxy(
        paths: SubscriptionPaths,
        platform: HostPlatform,
        proxy_url: Option<String>,
    ) -> Result<Self, SubscriptionError> {
        Ok(Self::with_fetcher_on(
            paths,
            DefaultFetcher::new(proxy_url)?,
            platform,
        ))
    }

    /// Open a manager with per-scheme proxy endpoints.
    pub fn open_with_proxy_config(
        paths: SubscriptionPaths,
        platform: HostPlatform,
        proxy: SubscriptionProxy,
    ) -> Result<Self, SubscriptionError> {
        Ok(Self::with_fetcher_on(
            paths,
            DefaultFetcher::with_proxy(proxy)?,
            platform,
        ))
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

    /// Validate a fetch target before calling the fetcher.
    ///
    /// Direct fetchers resolve the host themselves and need the full SSRF
    /// check. A proxied fetcher resolves through the proxy, so resolving here
    /// would fail whenever the local resolver is the broken hop.
    fn validate_fetch_url(&self, url: &str) -> Result<(), SubscriptionError> {
        if self.fetcher.bypasses_system_proxy() {
            validate_subscription_url(url)
        } else {
            validate_subscription_url_syntax(url)
        }
    }

    /// GET a subscription as the sing-box client first, which makes provider
    /// panels send a full sing-box config and their traffic counters, and
    /// return the response with the client it came from.
    ///
    /// Falls back to the generic client when the sing-box request is refused
    /// (a 4xx status other than 408 / 429), its body is rejected (too large,
    /// not UTF-8) or unusable, or its config loses route rules to rule sets
    /// ice-box cannot load. In the last case the generic answer is kept only
    /// when it parses; otherwise the sing-box config is still better than
    /// failing. Transport failures and 5xx are returned as they are: another
    /// `User-Agent` cannot fix them, and a retry would double the wait.
    ///
    /// Validators from `cached` go only to the client that issued them, so a
    /// validator for one client's body can never turn into a 304 for the
    /// other and pin the old body.
    fn fetch_preferring_singbox(
        &self,
        url: &str,
        cached: Option<&SubscriptionMeta>,
    ) -> Result<(FetchResponse, FetchClient), SubscriptionError> {
        // A cached sing-box config kept only because the generic client
        // failed must be judged again, so a 304 cannot pin it.
        let validators = |client: FetchClient| match cached {
            Some(meta)
                if meta.fetched_as.unwrap_or(FetchClient::Generic) == client
                    && !(client == FetchClient::SingBox
                        && crate::lost_rule_set_rules(&meta.parse_warnings)) =>
            {
                (meta.etag.as_deref(), meta.last_modified.as_deref())
            }
            _ => (None, None),
        };
        let redacted = crate::redact_subscription_url_for_log(url);

        let (etag, last_modified) = validators(FetchClient::SingBox);
        let (singbox_userinfo, lossy) = match self.fetcher.get_as(
            url,
            etag,
            last_modified,
            FetchClient::SingBox,
        ) {
            Ok(resp) => match self.judge_singbox_answer(&resp) {
                SingBoxAnswer::Usable => return Ok((resp, FetchClient::SingBox)),
                SingBoxAnswer::Lossy => {
                    tracing::info!(
                        url = %redacted,
                        "subscription fetch: sing-box config depends on rule sets ice-box cannot load; trying the generic client"
                    );
                    (resp.userinfo, Some(resp))
                }
                SingBoxAnswer::Unusable => {
                    tracing::info!(
                        url = %redacted,
                        "subscription fetch: sing-box response unusable; retrying as generic client"
                    );
                    (resp.userinfo, None)
                }
            },
            Err(err) if retry_as_generic(&err) => {
                tracing::info!(
                    url = %redacted,
                    error = %err.redacted_display(),
                    "subscription fetch: sing-box request refused; retrying as generic client"
                );
                (None, None)
            }
            Err(err) => return Err(err),
        };

        let (etag, last_modified) = validators(FetchClient::Generic);
        let generic = self
            .fetcher
            .get_as(url, etag, last_modified, FetchClient::Generic);
        if let Some(lossy) = lossy {
            // Parsed in full: a generic body that fails in the apply phase
            // must not replace a sing-box config that works.
            let generic_usable = matches!(
                &generic,
                Ok(resp) if resp.not_modified
                    || normalize_raw_body(&resp.body, self.platform).is_ok()
            );
            if !generic_usable {
                tracing::info!(
                    url = %redacted,
                    "subscription fetch: generic response unusable; keeping the sing-box config"
                );
                return Ok((lossy, FetchClient::SingBox));
            }
        }
        let mut resp = generic?;
        // Some panels send traffic counters only to clients they recognize.
        if resp.userinfo.is_none() {
            resp.userinfo = singbox_userinfo;
        }
        Ok((resp, FetchClient::Generic))
    }

    /// Classify a sing-box client response. Only a body that is a sing-box
    /// config is parsed here: a provider that ignores the `User-Agent` and
    /// sends share links or Clash would send the generic client the same body,
    /// so that body is used as it is and parsed once, in the apply phase.
    /// The parse is transient (each fetch worker drops it before spooling),
    /// so it costs CPU but does not keep a profile per batch entry in memory.
    fn judge_singbox_answer(&self, resp: &FetchResponse) -> SingBoxAnswer {
        if resp.not_modified {
            return SingBoxAnswer::Usable;
        }
        match crate::sniff_format(&resp.body) {
            SubscriptionFormat::Unknown => SingBoxAnswer::Unusable,
            SubscriptionFormat::SingBox => match normalize_raw_body(&resp.body, self.platform) {
                Ok((_, profile)) if crate::lost_rule_set_rules(&profile.parse_stats.warnings) => {
                    SingBoxAnswer::Lossy
                }
                Ok(_) => SingBoxAnswer::Usable,
                Err(_) => SingBoxAnswer::Unusable,
            },
            _ => SingBoxAnswer::Usable,
        }
    }

    /// Network fetch phase of an add: validate URL, GET (no disk writes).
    pub fn fetch_add(
        &self,
        url: &str,
        name: Option<&str>,
        auto_update: bool,
        auto_update_interval: Option<AutoUpdateInterval>,
    ) -> Result<FetchedAdd, SubscriptionError> {
        self.validate_fetch_url(url)?;

        let id = Uuid::new_v4();
        let (fetched, fetched_as) = self.fetch_preferring_singbox(url, None)?;
        Ok(FetchedAdd {
            id,
            url: url.to_string(),
            name: name.map(str::to_string),
            auto_update,
            auto_update_interval,
            source: SubscriptionSource::Remote,
            fetched,
            fetched_as: Some(fetched_as),
        })
    }

    /// Parse a local sing-box document. No network and no disk writes.
    ///
    /// The body is not logged. Callers drop it after this returns; the copy
    /// that must be persisted lives on [`FetchedAdd`].
    pub fn prepare_file_import(
        &self,
        body: &str,
        name: Option<&str>,
    ) -> Result<FetchedAdd, SubscriptionError> {
        let body = body.strip_prefix('\u{feff}').unwrap_or(body);
        if body.len() > MAX_BODY_BYTES {
            return Err(SubscriptionError::ParseFailed(format!(
                "body exceeds {MAX_BODY_BYTES} bytes"
            )));
        }
        if body.trim().is_empty() {
            return Err(SubscriptionError::UnknownFormat);
        }
        let (format, _) = normalize_raw_body(body, self.platform)?;
        if format != SubscriptionFormat::SingBox {
            return Err(SubscriptionError::InvalidSingBox(
                "not a sing-box configuration",
            ));
        }
        Ok(FetchedAdd {
            id: Uuid::new_v4(),
            url: String::new(),
            name: bounded_import_name(name),
            auto_update: false,
            auto_update_interval: None,
            source: SubscriptionSource::File,
            fetched_as: None,
            fetched: FetchResponse {
                body: body.to_string(),
                not_modified: false,
                etag: None,
                last_modified: None,
                userinfo: None,
                content_disposition: None,
            },
        })
    }

    /// Import a local sing-box document: parse → write success files.
    pub fn import_file(
        &self,
        body: &str,
        name: Option<&str>,
    ) -> Result<SubscriptionMeta, SubscriptionError> {
        self.apply_add(self.prepare_file_import(body, name)?)
    }

    /// Disk phase of an add: normalize + persist (or leave no success artifacts on failure).
    pub fn apply_add(&self, add: FetchedAdd) -> Result<SubscriptionMeta, SubscriptionError> {
        let FetchedAdd {
            id,
            url,
            name,
            auto_update,
            auto_update_interval,
            source,
            fetched,
            fetched_as,
        } = add;
        match normalize_raw_body(&fetched.body, self.platform) {
            Ok((format, mut profile)) => {
                if source == SubscriptionSource::File && format != SubscriptionFormat::SingBox {
                    return Err(SubscriptionError::InvalidSingBox(
                        "not a sing-box configuration",
                    ));
                }
                let index = load_index(&self.paths)?;
                let make_active = index.items.iter().all(|m| !m.active);
                let mut meta = meta_from_profile(
                    id,
                    resolve_subscription_name(
                        name.as_deref(),
                        fetched.content_disposition.as_deref(),
                        &url,
                        id,
                    ),
                    url,
                    format,
                    &mut profile,
                    make_active,
                    fetched.etag,
                    fetched.last_modified,
                    auto_update,
                    auto_update_interval,
                );
                meta.source = source;
                meta.fetched_as = fetched_as;
                if source == SubscriptionSource::File {
                    meta.auto_update = false;
                    meta.auto_update_interval = None;
                }
                meta.userinfo = fetched.userinfo;
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
        let index = load_index(&self.paths)?;
        let meta = index
            .items
            .iter()
            .find(|m| m.id == id)
            .cloned()
            .ok_or_else(|| {
                SubscriptionError::ParseFailed(format!("subscription {id} not found"))
            })?;

        if meta.source == SubscriptionSource::File {
            return Err(SubscriptionError::FetchFailed(
                "file subscription has no remote URL".into(),
            ));
        }

        self.validate_fetch_url(&meta.url)?;

        let (fetched, fetched_as) = self.fetch_preferring_singbox(&meta.url, Some(&meta))?;
        Ok(FetchedUpdate {
            meta,
            fetched,
            fetched_as,
            spooled: None,
        })
    }

    /// Disk phase of an update: normalize + persist, or record `last_error`.
    pub fn apply_update(&self, upd: FetchedUpdate) -> Result<SubscriptionMeta, SubscriptionError> {
        let upd = upd.materialize()?;
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
            return mark_subscription_refreshed(&self.paths, upd.meta.id, upd.fetched.userinfo);
        }
        match normalize_raw_body(&upd.fetched.body, self.platform) {
            Ok((format, mut profile)) => {
                let updated = meta_from_fetched_profile(
                    &current,
                    &upd.fetched,
                    upd.fetched_as,
                    format,
                    &mut profile,
                );
                write_subscription_success(&self.paths, &updated, &upd.fetched.body, &profile)?;
                Ok(updated)
            }
            Err(err) => {
                write_subscription_error(&self.paths, upd.meta.id, err.ui_message())?;
                Err(err)
            }
        }
    }

    /// Network phase of updating the subscriptions with `auto_update` enabled. Same parallel
    /// fetch as [`SubscriptionManager::fetch_ids`], but only touches the flagged entries so a
    /// background refresh never re-fetches subscriptions the user did not opt into.
    pub fn fetch_auto(&self) -> Vec<(Uuid, Result<FetchedUpdate, SubscriptionError>)>
    where
        F: Sync,
    {
        let ids: Vec<Uuid> = load_index(&self.paths)
            .map(|i| {
                i.items
                    .into_iter()
                    .filter(|m| m.auto_update && m.source != SubscriptionSource::File)
                    .map(|m| m.id)
                    .collect()
            })
            .unwrap_or_default();
        self.fetch_ids(ids)
    }

    /// Parallel fetch with bounded pending responses and disk-backed bodies.
    /// Only temporary storage is written, so the orchestrate lock is unnecessary.
    /// [`SubscriptionManager::apply_all`] materializes and persists one body at a time.
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
        let (sender, receiver) = std::sync::mpsc::sync_channel(worker_count);

        let mut completed = std::thread::scope(|scope| {
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
            // Drain while workers are running: joining before receiving would
            // deadlock a bounded channel. Never retain a whole batch in memory.
            let mut file = None;
            receiver
                .into_iter()
                .map(|(index, id, result)| {
                    (index, id, result.and_then(|update| update.spool(&mut file)))
                })
                .collect::<Vec<_>>()
        });

        completed.sort_unstable_by_key(|(index, _, _)| *index);
        completed
            .into_iter()
            .map(|(_, id, result)| (id, result))
            .collect()
    }

    /// Disk phase of a batch fetch ([`SubscriptionManager::fetch_ids`] or
    /// [`SubscriptionManager::fetch_auto`]): persists each fetched update
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
            match result.and_then(FetchedUpdate::materialize) {
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
                        let updated = mark_refreshed_in_index(
                            &self.paths,
                            &mut index,
                            id,
                            upd.fetched.userinfo,
                        )
                        .unwrap_or(current);
                        out.push((id, Ok(updated)));
                        continue;
                    }
                    match normalize_raw_body(&upd.fetched.body, self.platform) {
                        Ok((format, mut profile)) => {
                            let updated = meta_from_fetched_profile(
                                &current,
                                &upd.fetched,
                                upd.fetched_as,
                                format,
                                &mut profile,
                            );
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

    /// Download remote rule-sets with this manager's fetcher (same proxy
    /// and SSRF rules as subscription fetches). See
    /// [`crate::refresh_rule_sets`].
    pub fn refresh_rule_sets(
        &self,
        cache_dir: &std::path::Path,
        urls: &[String],
        max_age: std::time::Duration,
        force: bool,
    ) -> Vec<crate::RuleSetRefreshReport> {
        crate::refresh_rule_sets(&self.fetcher, cache_dir, urls, max_age, force)
    }
}

/// Display names are stored in the index. Cap them so a file name cannot bloat it.
fn bounded_import_name(name: Option<&str>) -> Option<String> {
    let trimmed = name?.trim();
    if trimmed.is_empty() {
        return None;
    }
    let cleaned: String = trimmed
        .chars()
        .filter(|ch| !ch.is_control())
        .take(200)
        .collect();
    let cleaned = cleaned.trim().to_string();
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
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
            fetched_as: None,
            userinfo: None,
            provider_info: vec![],
            auto_update: false,
            auto_update_interval: None,
            source: crate::SubscriptionSource::Remote,
        };
        self.index.items.push(meta.clone());
        meta
    }
}
