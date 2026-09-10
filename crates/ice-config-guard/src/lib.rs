// SPDX-License-Identifier: GPL-3.0-or-later

//! Deny-by-default sanitizer for sing-box JSON that will be loaded by an
//! elevated core (macOS helper as root, Windows scheduled-task launcher as
//! Administrator).
//!
//! The unprivileged app (and a remote subscription) can write `config.json`.
//! This crate is the privileged-side content filter: unknown inbound keys,
//! disallowed outbound / DNS server types, remote `route.rule_set` URLs, and
//! filesystem-referencing keys are rejected with a JSON pointer in the error.
//! Callers write the sanitised object to a root/admin-owned path and start
//! sing-box from that copy.

#[cfg(not(unix))]
use std::fs;
use std::path::{Path, PathBuf};

use ice_types::{is_loopback_host, is_restricted_fetch_host};
use serde_json::{json, Map, Value};

/// Upper bound on a sanitised config after parse (8 MiB).
pub const MAX_CONFIG_BYTES: usize = 8 * 1024 * 1024;
/// Upper bound on `outbounds` array length.
pub const MAX_OUTBOUNDS: usize = 640;
/// Upper bound on `route.rules` array length.
pub const MAX_ROUTE_RULES: usize = 10_000;

/// Paths and policy the privileged caller already owns.
#[derive(Debug, Clone)]
pub struct GuardContext {
    /// App data dir. `route.rule_set[].path` may canonicalise here (GeoIP
    /// copies live under `data_dir/geoip`) or under [`Self::resources_dir`].
    pub data_dir: PathBuf,
    /// Bundled resources directory.
    pub resources_dir: PathBuf,
    /// When set, `log` is replaced so `output` is this helper-owned file.
    pub log_output: Option<PathBuf>,
    /// When set, any `experimental.cache_file.path` is overwritten to this
    /// helper-owned file. The object is not created if it was absent (the
    /// generator keeps cache_file off so a cached Clash mode cannot override
    /// `default_mode`).
    pub cache_file_path: Option<PathBuf>,
}

/// Rejection with a JSON pointer locating the first offending field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardError {
    pub pointer: String,
    pub message: String,
}

impl GuardError {
    pub fn new(pointer: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            pointer: pointer.into(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for GuardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.pointer.is_empty() {
            f.write_str(&self.message)
        } else {
            write!(f, "{}: {}", self.pointer, self.message)
        }
    }
}

impl std::error::Error for GuardError {}

const ALLOWED_OUTBOUND_TYPES: &[&str] = &[
    "direct",
    "block",
    "dns",
    "selector",
    "urltest",
    "fallback",
    "loadbalance",
    "shadowsocks",
    "vmess",
    "vless",
    "trojan",
    "hysteria",
    "hysteria2",
    "tuic",
    "http",
    "socks",
    "anytls",
    "wireguard",
    "shadowtls",
];

/// DNS servers the generator and Clash parser actually emit. `hosts` is
/// excluded: its `path` is a filesystem hosts file, not a DoH URL path.
const ALLOWED_DNS_SERVER_TYPES: &[&str] = &[
    "local", "tls", "https", "h3", "tcp", "udp", "quic", "fakeip", "rcode",
];

const MIXED_INBOUND_KEYS: &[&str] = &["type", "tag", "listen", "listen_port"];
const TUN_INBOUND_KEYS: &[&str] = &[
    "type",
    "tag",
    "interface_name",
    "address",
    "mtu",
    "auto_route",
    "strict_route",
    "stack",
    "route_exclude_address",
    "loopback_address",
];

/// Whether `ty` is allowed as an elevated (or subscription) outbound type.
pub fn outbound_type_is_allowed(ty: &str) -> bool {
    ALLOWED_OUTBOUND_TYPES.contains(&ty)
}

/// Whether `ty` is allowed as a DNS server in generated / elevated configs.
pub fn dns_server_type_is_allowed(ty: &str) -> bool {
    ALLOWED_DNS_SERVER_TYPES.contains(&ty)
}

/// Keep only `type: local` rule-sets that have a path and no remote URL.
/// Used by the config builder so the user-mode core never fetches `remote`
/// rule-sets; the elevated sanitizer still fail-closes if any slip through.
pub fn retain_local_rule_sets(sets: &mut Vec<Value>) {
    sets.retain(rule_set_is_local_only);
}

/// Drop DNS servers whose type is not on the elevated allowlist (notably
/// `hosts`, whose `path` is a filesystem file).
pub fn retain_allowed_dns_servers(dns: &mut Value) {
    let Some(servers) = dns.get_mut("servers").and_then(|v| v.as_array_mut()) else {
        return;
    };
    servers.retain(|server| {
        server
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(dns_server_type_is_allowed)
    });
}

fn rule_set_is_local_only(set: &Value) -> bool {
    let Some(obj) = set.as_object() else {
        return false;
    };
    if obj.get("type").and_then(Value::as_str) != Some("local") {
        return false;
    }
    if obj.contains_key("url") || obj.contains_key("download_url") {
        return false;
    }
    obj.get("path")
        .and_then(Value::as_str)
        .is_some_and(|path| !path.is_empty())
}

/// Read a user-supplied config without following a final-component symlink
/// or buffering more than [`MAX_CONFIG_BYTES`]. FIFOs are opened non-blocking
/// so a planted pipe cannot stall a privileged reader.
pub fn read_config_file(path: &Path) -> Result<Vec<u8>, GuardError> {
    #[cfg(unix)]
    {
        use std::fs::OpenOptions;
        use std::io::Read;
        use std::os::unix::fs::{FileTypeExt, OpenOptionsExt};

        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)
            .map_err(|err| GuardError::new("", format!("open config {}: {err}", path.display())))?;
        let meta = file
            .metadata()
            .map_err(|err| GuardError::new("", format!("stat config {}: {err}", path.display())))?;
        if !meta.file_type().is_file() || meta.file_type().is_fifo() {
            return Err(GuardError::new(
                "",
                format!("config path is not a regular file: {}", path.display()),
            ));
        }
        if meta.len() > MAX_CONFIG_BYTES as u64 {
            return Err(GuardError::new(
                "",
                format!("config exceeds {MAX_CONFIG_BYTES} bytes"),
            ));
        }
        let mut raw = Vec::new();
        file.take(MAX_CONFIG_BYTES as u64 + 1)
            .read_to_end(&mut raw)
            .map_err(|err| GuardError::new("", format!("read config {}: {err}", path.display())))?;
        if raw.len() > MAX_CONFIG_BYTES {
            return Err(GuardError::new(
                "",
                format!("config exceeds {MAX_CONFIG_BYTES} bytes"),
            ));
        }
        Ok(raw)
    }
    #[cfg(not(unix))]
    {
        let meta = fs::symlink_metadata(path)
            .map_err(|err| GuardError::new("", format!("stat config {}: {err}", path.display())))?;
        if meta.file_type().is_symlink() || !meta.is_file() {
            return Err(GuardError::new(
                "",
                format!("config path is not a regular file: {}", path.display()),
            ));
        }
        if meta.len() > MAX_CONFIG_BYTES as u64 {
            return Err(GuardError::new(
                "",
                format!("config exceeds {MAX_CONFIG_BYTES} bytes"),
            ));
        }
        let raw = fs::read(path)
            .map_err(|err| GuardError::new("", format!("read config {}: {err}", path.display())))?;
        if raw.len() > MAX_CONFIG_BYTES {
            return Err(GuardError::new(
                "",
                format!("config exceeds {MAX_CONFIG_BYTES} bytes"),
            ));
        }
        Ok(raw)
    }
}

/// Probe URL used by `urltest` (and Clash `url-test` / `fallback`) groups.
/// HTTP(S) to a non-restricted host only — no loopback, RFC1918, or
/// link-local metadata endpoints.
pub fn health_check_url_is_allowed(raw: &str) -> bool {
    parse_http_url_host(raw).is_some_and(|host| !is_restricted_fetch_host(&host))
}

/// Check a single outbound object (subscription normalisation). Does not
/// require a [`GuardContext`].
pub fn check_outbound(value: &Value) -> Result<(), GuardError> {
    let obj = value
        .as_object()
        .ok_or_else(|| GuardError::new("", "outbound must be a JSON object"))?;
    let ty = obj
        .get("type")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GuardError::new("/type", "outbound is missing type"))?;
    if !outbound_type_is_allowed(ty) {
        return Err(GuardError::new(
            "/type",
            format!("outbound type {ty:?} is not allowed"),
        ));
    }
    reject_wireguard_system(obj, "")?;
    if ty == "urltest" || ty == "fallback" {
        validate_health_check_url(obj, "")?;
    }
    walk_forbidden(value, "", None)?;
    Ok(())
}

/// Validate and, when the context supplies helper-owned paths, overwrite
/// `log` / `cache_file.path`. On success `cfg` is safe to write to a
/// privileged path and pass to sing-box.
pub fn sanitize_for_elevated_core(cfg: &mut Value, ctx: &GuardContext) -> Result<(), GuardError> {
    let encoded_len = serde_json::to_vec(cfg)
        .map_err(|err| GuardError::new("", format!("serialize config: {err}")))?
        .len();
    if encoded_len > MAX_CONFIG_BYTES {
        return Err(GuardError::new(
            "",
            format!("config exceeds {MAX_CONFIG_BYTES} bytes"),
        ));
    }
    if !cfg.is_object() {
        return Err(GuardError::new("", "config root must be a JSON object"));
    }

    let outbound_len = cfg
        .get("outbounds")
        .and_then(|v| v.as_array())
        .ok_or_else(|| GuardError::new("/outbounds", "outbounds must be an array"))?
        .len();
    if outbound_len > MAX_OUTBOUNDS {
        return Err(GuardError::new(
            "/outbounds",
            format!("outbound count {outbound_len} exceeds {MAX_OUTBOUNDS}"),
        ));
    }
    if outbound_len == 0 {
        return Err(GuardError::new("/outbounds", "outbounds must not be empty"));
    }

    reject_ungated_top_level(cfg)?;
    validate_inbounds(cfg)?;

    if let Some(rule_len) = cfg
        .get("route")
        .and_then(|r| r.get("rules"))
        .and_then(|v| v.as_array())
        .map(|a| a.len())
    {
        if rule_len > MAX_ROUTE_RULES {
            return Err(GuardError::new(
                "/route/rules",
                format!("rule count {rule_len} exceeds {MAX_ROUTE_RULES}"),
            ));
        }
    }

    validate_outbounds(cfg)?;
    validate_dns_servers(cfg)?;
    validate_experimental(cfg)?;
    validate_clash_controller(cfg)?;
    walk_forbidden(cfg, "", Some(ctx))?;
    validate_rule_set_paths(cfg, ctx)?;
    apply_helper_overrides(cfg, ctx);
    Ok(())
}

/// A mixed-inbound config the sanitizer accepts. Used by helper tests.
pub fn minimal_allowed_config() -> Value {
    json!({
        "log": { "level": "info", "timestamp": true },
        "inbounds": [{
            "type": "mixed",
            "tag": "mixed-in",
            "listen": "127.0.0.1",
            "listen_port": 17890
        }],
        "outbounds": [{ "type": "direct", "tag": "direct" }],
        "route": { "final": "direct" },
        "experimental": {
            "clash_api": { "external_controller": "127.0.0.1:19090" }
        }
    })
}

fn reject_ungated_top_level(cfg: &Value) -> Result<(), GuardError> {
    for key in ["endpoints", "services"] {
        if cfg.get(key).is_some() {
            return Err(GuardError::new(
                format!("/{key}"),
                format!("{key} is not allowed in an elevated config"),
            ));
        }
    }
    Ok(())
}

fn validate_inbounds(cfg: &Value) -> Result<(), GuardError> {
    let inbounds = cfg
        .get("inbounds")
        .and_then(|v| v.as_array())
        .ok_or_else(|| GuardError::new("/inbounds", "inbounds must be an array"))?;
    if inbounds.is_empty() {
        return Err(GuardError::new("/inbounds", "inbounds must not be empty"));
    }
    for (idx, inbound) in inbounds.iter().enumerate() {
        let pointer = format!("/inbounds/{idx}");
        let obj = inbound
            .as_object()
            .ok_or_else(|| GuardError::new(&pointer, "inbound must be a JSON object"))?;
        let ty = obj
            .get("type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| GuardError::new(format!("{pointer}/type"), "inbound is missing type"))?;
        let allowed = match ty {
            "mixed" => MIXED_INBOUND_KEYS,
            "tun" => TUN_INBOUND_KEYS,
            other => {
                return Err(GuardError::new(
                    format!("{pointer}/type"),
                    format!("inbound type {other:?} is not allowed"),
                ));
            }
        };
        for key in obj.keys() {
            if !allowed.contains(&key.as_str()) {
                return Err(GuardError::new(
                    format!("{pointer}/{}", json_pointer_escape(key)),
                    format!("inbound key {key:?} is not allowed on type {ty}"),
                ));
            }
        }
        if ty == "mixed" {
            validate_mixed_listen(obj, &pointer)?;
        }
    }
    Ok(())
}

/// Mixed inbound may bind loopback (default) or unspecified (`0.0.0.0` /
/// `::`) when the user enables LAN. Privileged ports and arbitrary unicast
/// addresses are refused so a swapped config cannot steal 22/443 or an
/// unexpected interface as root/Administrator.
fn validate_mixed_listen(obj: &Map<String, Value>, pointer: &str) -> Result<(), GuardError> {
    let listen = obj.get("listen").and_then(Value::as_str).ok_or_else(|| {
        GuardError::new(
            format!("{pointer}/listen"),
            "mixed inbound is missing listen",
        )
    })?;
    if !mixed_listen_is_allowed(listen) {
        return Err(GuardError::new(
            format!("{pointer}/listen"),
            format!("mixed inbound listen {listen:?} must be loopback or unspecified"),
        ));
    }
    let port = obj
        .get("listen_port")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            GuardError::new(
                format!("{pointer}/listen_port"),
                "mixed inbound is missing listen_port",
            )
        })?;
    if !(1024..=65535).contains(&port) {
        return Err(GuardError::new(
            format!("{pointer}/listen_port"),
            format!("mixed inbound listen_port {port} must be in 1024..=65535"),
        ));
    }
    Ok(())
}

fn mixed_listen_is_allowed(host: &str) -> bool {
    let host = host.trim();
    is_loopback_host(host) || matches!(host, "0.0.0.0" | "::" | "[::]")
}

fn validate_outbounds(cfg: &Value) -> Result<(), GuardError> {
    let outbounds = cfg
        .get("outbounds")
        .and_then(|v| v.as_array())
        .expect("checked");
    for (idx, outbound) in outbounds.iter().enumerate() {
        let pointer = format!("/outbounds/{idx}");
        let obj = outbound
            .as_object()
            .ok_or_else(|| GuardError::new(&pointer, "outbound must be a JSON object"))?;
        let ty = obj.get("type").and_then(|v| v.as_str()).ok_or_else(|| {
            GuardError::new(format!("{pointer}/type"), "outbound is missing type")
        })?;
        if !outbound_type_is_allowed(ty) {
            return Err(GuardError::new(
                format!("{pointer}/type"),
                format!("outbound type {ty:?} is not allowed"),
            ));
        }
        reject_wireguard_system(obj, &pointer)?;
        if ty == "urltest" || ty == "fallback" {
            validate_health_check_url(obj, &pointer)?;
        }
    }
    Ok(())
}

fn validate_health_check_url(obj: &Map<String, Value>, pointer: &str) -> Result<(), GuardError> {
    let Some(url) = obj.get("url") else {
        return Ok(());
    };
    let Some(raw) = url.as_str() else {
        return Err(GuardError::new(
            format!("{pointer}/url"),
            "health-check url must be a string",
        ));
    };
    if !health_check_url_is_allowed(raw) {
        return Err(GuardError::new(
            format!("{pointer}/url"),
            format!("health-check url {raw:?} is not allowed"),
        ));
    }
    Ok(())
}

fn reject_wireguard_system(obj: &Map<String, Value>, pointer: &str) -> Result<(), GuardError> {
    let ty = obj.get("type").and_then(|v| v.as_str()).unwrap_or("");
    if ty != "wireguard" {
        return Ok(());
    }
    if obj.get("system").and_then(|v| v.as_bool()) == Some(true) {
        return Err(GuardError::new(
            format!("{pointer}/system"),
            "wireguard system networking is not allowed",
        ));
    }
    if obj.contains_key("interface_name") {
        return Err(GuardError::new(
            format!("{pointer}/interface_name"),
            "wireguard interface_name is not allowed",
        ));
    }
    Ok(())
}

fn validate_dns_servers(cfg: &Value) -> Result<(), GuardError> {
    let Some(dns) = cfg.get("dns") else {
        return Ok(());
    };
    let Some(servers) = dns.get("servers") else {
        return Ok(());
    };
    let servers = servers
        .as_array()
        .ok_or_else(|| GuardError::new("/dns/servers", "dns.servers must be an array"))?;
    for (idx, server) in servers.iter().enumerate() {
        let pointer = format!("/dns/servers/{idx}");
        let obj = server
            .as_object()
            .ok_or_else(|| GuardError::new(&pointer, "dns server must be a JSON object"))?;
        let ty = obj.get("type").and_then(Value::as_str).ok_or_else(|| {
            GuardError::new(format!("{pointer}/type"), "dns server is missing type")
        })?;
        if !ALLOWED_DNS_SERVER_TYPES.contains(&ty) {
            return Err(GuardError::new(
                format!("{pointer}/type"),
                format!("dns server type {ty:?} is not allowed"),
            ));
        }
    }
    Ok(())
}

fn validate_experimental(cfg: &Value) -> Result<(), GuardError> {
    let Some(exp) = cfg.get("experimental") else {
        return Ok(());
    };
    let obj = exp
        .as_object()
        .ok_or_else(|| GuardError::new("/experimental", "experimental must be a JSON object"))?;
    for key in obj.keys() {
        if !matches!(key.as_str(), "clash_api" | "cache_file") {
            return Err(GuardError::new(
                format!("/experimental/{}", json_pointer_escape(key)),
                format!("experimental key {key:?} is not allowed"),
            ));
        }
    }
    Ok(())
}

fn validate_clash_controller(cfg: &Value) -> Result<(), GuardError> {
    let pointer = "/experimental/clash_api/external_controller";
    let controller = cfg
        .get("experimental")
        .and_then(|v| v.get("clash_api"))
        .and_then(|v| v.get("external_controller"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            GuardError::new(
                pointer,
                "experimental.clash_api.external_controller is required",
            )
        })?;
    let host = controller_host(controller).ok_or_else(|| {
        GuardError::new(
            pointer,
            format!("invalid external_controller {controller:?}"),
        )
    })?;
    if !is_loopback_host(host) {
        return Err(GuardError::new(
            pointer,
            format!("external_controller host {host:?} must be loopback"),
        ));
    }
    Ok(())
}

fn validate_rule_set_paths(cfg: &Value, ctx: &GuardContext) -> Result<(), GuardError> {
    let Some(sets) = cfg
        .get("route")
        .and_then(|r| r.get("rule_set"))
        .and_then(|v| v.as_array())
    else {
        return Ok(());
    };
    let roots = allowed_rule_set_roots(ctx)?;
    for (idx, set) in sets.iter().enumerate() {
        let base = format!("/route/rule_set/{idx}");
        let obj = set
            .as_object()
            .ok_or_else(|| GuardError::new(&base, "rule_set must be a JSON object"))?;
        for url_key in ["url", "download_url"] {
            if obj.contains_key(url_key) {
                return Err(GuardError::new(
                    format!("{base}/{url_key}"),
                    format!("rule_set {url_key} is not allowed in an elevated config"),
                ));
            }
        }
        match obj.get("type").and_then(Value::as_str) {
            Some("local") => {}
            Some(other) => {
                return Err(GuardError::new(
                    format!("{base}/type"),
                    format!(
                        "rule_set type {other:?} is not allowed; elevated configs may only use type \"local\""
                    ),
                ));
            }
            None => {
                return Err(GuardError::new(
                    format!("{base}/type"),
                    "rule_set type is required and must be \"local\"",
                ));
            }
        }
        let pointer = format!("{base}/path");
        let Some(raw) = obj.get("path").and_then(|v| v.as_str()) else {
            return Err(GuardError::new(
                &pointer,
                "rule_set path is required for type \"local\"",
            ));
        };
        let p = Path::new(raw);
        let candidates: Vec<PathBuf> = if p.is_absolute() {
            vec![p.to_path_buf()]
        } else {
            vec![ctx.resources_dir.join(p), ctx.data_dir.join(p)]
        };
        let mut last_err = None;
        let mut accepted = false;
        for candidate in candidates {
            match candidate.canonicalize() {
                Ok(canon) => {
                    if roots.iter().any(|root| canon.starts_with(root)) {
                        accepted = true;
                        break;
                    }
                    last_err = Some(GuardError::new(
                        pointer.clone(),
                        format!(
                            "rule_set path {} is outside data_dir {} and resources_dir {}",
                            raw,
                            ctx.data_dir.display(),
                            ctx.resources_dir.display()
                        ),
                    ));
                }
                Err(err) => {
                    last_err = Some(GuardError::new(
                        pointer.clone(),
                        format!("rule_set path {} cannot be canonicalised: {err}", raw),
                    ));
                }
            }
        }
        if !accepted {
            return Err(last_err.unwrap_or_else(|| {
                GuardError::new(&pointer, format!("rule_set path {raw} is not allowed"))
            }));
        }
    }
    Ok(())
}

fn allowed_rule_set_roots(ctx: &GuardContext) -> Result<Vec<PathBuf>, GuardError> {
    let mut roots = Vec::new();
    for path in [&ctx.resources_dir, &ctx.data_dir] {
        if let Ok(p) = path.canonicalize() {
            roots.push(p);
        }
    }
    if roots.is_empty() {
        return Err(GuardError::new(
            "/route/rule_set",
            format!(
                "neither resources_dir {} nor data_dir {} can be canonicalised",
                ctx.resources_dir.display(),
                ctx.data_dir.display()
            ),
        ));
    }
    Ok(roots)
}

fn apply_helper_overrides(cfg: &mut Value, ctx: &GuardContext) {
    if let Some(output) = &ctx.log_output {
        let level = cfg
            .get("log")
            .and_then(|l| l.get("level"))
            .cloned()
            .unwrap_or_else(|| json!("info"));
        let timestamp = cfg
            .get("log")
            .and_then(|l| l.get("timestamp"))
            .cloned()
            .unwrap_or(json!(true));
        if let Some(obj) = cfg.as_object_mut() {
            obj.insert(
                "log".into(),
                json!({
                    "level": level,
                    "timestamp": timestamp,
                    "output": output.to_string_lossy(),
                }),
            );
        }
    }
    if let Some(cache_path) = &ctx.cache_file_path {
        let Some(exp) = cfg.get_mut("experimental").and_then(|v| v.as_object_mut()) else {
            return;
        };
        if let Some(cache) = exp.get_mut("cache_file").and_then(|v| v.as_object_mut()) {
            cache.insert(
                "path".into(),
                Value::String(cache_path.to_string_lossy().into_owned()),
            );
        }
    }
}

fn walk_forbidden(
    value: &Value,
    pointer: &str,
    ctx: Option<&GuardContext>,
) -> Result<(), GuardError> {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                let child_pointer = format!("{pointer}/{}", json_pointer_escape(key));
                if skip_forbidden_at(key, &child_pointer, map, ctx) {
                    if child.is_object() || child.is_array() {
                        walk_forbidden(child, &child_pointer, ctx)?;
                    }
                    continue;
                }
                if is_forbidden_key(key) {
                    return Err(GuardError::new(
                        child_pointer,
                        format!("key {key:?} is not allowed in an elevated config"),
                    ));
                }
                walk_forbidden(child, &child_pointer, ctx)?;
            }
            Ok(())
        }
        Value::Array(items) => {
            for (idx, child) in items.iter().enumerate() {
                walk_forbidden(child, &format!("{pointer}/{idx}"), ctx)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn skip_forbidden_at(
    key: &str,
    pointer: &str,
    parent: &Map<String, Value>,
    ctx: Option<&GuardContext>,
) -> bool {
    if key == "path" && is_url_style_path(pointer, parent) {
        return true;
    }
    if key == "path" && is_rule_set_path_pointer(pointer) {
        return true;
    }
    if key == "output" && pointer == "/log/output" && ctx.is_some_and(|c| c.log_output.is_some()) {
        return true;
    }
    if key == "path"
        && pointer == "/experimental/cache_file/path"
        && ctx.is_some_and(|c| c.cache_file_path.is_some())
    {
        return true;
    }
    if key == "url"
        && matches!(
            parent.get("type").and_then(Value::as_str),
            Some("urltest" | "fallback")
        )
    {
        return true;
    }
    if matches!(key, "process_path" | "process_path_regex") {
        return true;
    }
    false
}

fn is_url_style_path(pointer: &str, parent: &Map<String, Value>) -> bool {
    // WS / HTTP / HTTPUpgrade transport URL path, not a filesystem path.
    // `check_outbound` walks a single outbound from `""`, so the pointer is
    // `/transport/path`; the full-config walk uses `/outbounds/{i}/transport/path`.
    if pointer == "/transport/path" || pointer.ends_with("/transport/path") {
        return true;
    }
    // DoH / DoH3 URL path (typically `/dns-query`). Other DNS server types
    // that use `path` (notably `hosts`) are filesystem references.
    is_dns_server_path_pointer(pointer)
        && matches!(
            parent.get("type").and_then(Value::as_str),
            Some("https" | "h3")
        )
}

fn is_dns_server_path_pointer(pointer: &str) -> bool {
    let Some(rest) = pointer.strip_prefix("/dns/servers/") else {
        return false;
    };
    let Some((idx, key)) = rest.split_once('/') else {
        return false;
    };
    key == "path" && !idx.is_empty() && idx.chars().all(|c| c.is_ascii_digit())
}

fn is_rule_set_path_pointer(pointer: &str) -> bool {
    let Some(rest) = pointer.strip_prefix("/route/rule_set/") else {
        return false;
    };
    let Some((idx, key)) = rest.split_once('/') else {
        return false;
    };
    key == "path" && idx.chars().all(|c| c.is_ascii_digit()) && !idx.is_empty()
}

fn is_forbidden_key(key: &str) -> bool {
    matches!(
        key,
        "path"
            | "executable_path"
            | "data_directory"
            | "state_directory"
            | "working_directory"
            | "home_directory"
            | "output"
            | "external_ui"
            | "external_ui_download_url"
            | "url"
            | "download_url"
    ) || key.ends_with("_path")
}

fn json_pointer_escape(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}

fn controller_host(controller: &str) -> Option<&str> {
    let trimmed = controller.trim();
    if let Some(rest) = trimmed.strip_prefix('[') {
        let end = rest.find(']')?;
        return Some(&rest[..end]);
    }
    let (host, _port) = trimmed.rsplit_once(':')?;
    if host.is_empty() {
        return None;
    }
    Some(host)
}

fn parse_http_url_host(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() || raw.contains(|c: char| c.is_ascii_whitespace() || c == '\\') {
        return None;
    }
    let rest = if raw.len() >= 8 && raw[..8].eq_ignore_ascii_case("https://") {
        &raw[8..]
    } else if raw.len() >= 7 && raw[..7].eq_ignore_ascii_case("http://") {
        &raw[7..]
    } else {
        return None;
    };
    let authority = rest.split(['/', '?', '#']).next()?.trim();
    if authority.is_empty() {
        return None;
    }
    let hostport = authority
        .rsplit_once('@')
        .map(|(_, h)| h)
        .unwrap_or(authority);
    if hostport.is_empty() {
        return None;
    }
    let host = if let Some(inner) = hostport.strip_prefix('[') {
        let end = inner.find(']')?;
        inner[..end].to_string()
    } else if let Some((h, port)) = hostport.rsplit_once(':') {
        if port.is_empty() || !port.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        h.to_string()
    } else {
        hostport.to_string()
    };
    if host.is_empty() {
        return None;
    }
    Some(host)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ice-config-guard-{label}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn ctx(resources: &Path) -> GuardContext {
        GuardContext {
            data_dir: resources.to_path_buf(),
            resources_dir: resources.to_path_buf(),
            log_output: None,
            cache_file_path: None,
        }
    }

    fn sanitize(mut cfg: Value, ctx: &GuardContext) -> Result<Value, GuardError> {
        sanitize_for_elevated_core(&mut cfg, ctx)?;
        Ok(cfg)
    }

    #[test]
    fn minimal_allowed_config_passes_unchanged() {
        let dir = temp_dir("ok");
        let before = minimal_allowed_config();
        let after = sanitize(before.clone(), &ctx(&dir)).expect("ok");
        assert_eq!(before, after);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn tor_outbound_is_rejected_with_pointer() {
        let dir = temp_dir("tor");
        let mut cfg = minimal_allowed_config();
        cfg["outbounds"] = json!([
            { "type": "tor", "tag": "evil", "executable_path": "/usr/bin/tor" }
        ]);
        let err = sanitize(cfg, &ctx(&dir)).expect_err("tor");
        assert_eq!(err.pointer, "/outbounds/0/type");
        assert!(err.to_string().contains("tor"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn certificate_path_is_rejected() {
        let dir = temp_dir("cert");
        let mut cfg = minimal_allowed_config();
        cfg["outbounds"] = json!([{
            "type": "trojan",
            "tag": "n",
            "server": "1.1.1.1",
            "server_port": 443,
            "password": "x",
            "tls": { "enabled": true, "certificate_path": "/tmp/cert.pem" }
        }]);
        let err = sanitize(cfg, &ctx(&dir)).expect_err("cert");
        assert!(err.pointer.contains("certificate_path"), "{}", err.pointer);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn non_loopback_controller_is_rejected() {
        let dir = temp_dir("lan");
        let mut cfg = minimal_allowed_config();
        cfg["experimental"]["clash_api"]["external_controller"] = json!("0.0.0.0:9090");
        let err = sanitize(cfg, &ctx(&dir)).expect_err("lan");
        assert_eq!(err.pointer, "/experimental/clash_api/external_controller");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rule_set_path_outside_resources_is_rejected() {
        let dir = temp_dir("rs");
        fs::write(dir.join("geoip-cn.srs"), b"x").unwrap();
        let mut cfg = minimal_allowed_config();
        cfg["route"]["rule_set"] = json!([{
            "type": "local",
            "tag": "geoip-cn",
            "format": "binary",
            "path": "/etc/passwd"
        }]);
        let err = sanitize(cfg, &ctx(&dir)).expect_err("outside");
        assert_eq!(err.pointer, "/route/rule_set/0/path");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rule_set_path_inside_resources_is_accepted() {
        let dir = temp_dir("rs-ok");
        let srs = dir.join("geoip").join("geoip-cn.srs");
        fs::create_dir_all(srs.parent().unwrap()).unwrap();
        fs::write(&srs, b"x").unwrap();
        let mut cfg = minimal_allowed_config();
        cfg["route"]["rule_set"] = json!([{
            "type": "local",
            "tag": "geoip-cn",
            "format": "binary",
            "path": srs.to_string_lossy(),
        }]);
        sanitize(cfg, &ctx(&dir)).expect("inside resources");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn remote_rule_set_url_is_rejected() {
        let dir = temp_dir("rs-remote");
        let mut cfg = minimal_allowed_config();
        cfg["route"]["rule_set"] = json!([{
            "type": "remote",
            "tag": "evil",
            "format": "binary",
            "url": "https://127.0.0.1/latest/meta-data/"
        }]);
        let err = sanitize(cfg, &ctx(&dir)).expect_err("remote url");
        assert_eq!(err.pointer, "/route/rule_set/0/url");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn remote_rule_set_type_is_rejected() {
        let dir = temp_dir("rs-remote-type");
        let mut cfg = minimal_allowed_config();
        cfg["route"]["rule_set"] = json!([{
            "type": "remote",
            "tag": "evil",
            "format": "binary"
        }]);
        let err = sanitize(cfg, &ctx(&dir)).expect_err("remote type");
        assert_eq!(err.pointer, "/route/rule_set/0/type");
        assert!(err.message.contains("local"), "{}", err.message);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn local_rule_set_download_url_is_rejected() {
        let dir = temp_dir("rs-download-url");
        fs::write(dir.join("geoip-cn.srs"), b"x").unwrap();
        let mut cfg = minimal_allowed_config();
        cfg["route"]["rule_set"] = json!([{
            "type": "local",
            "tag": "geoip-cn",
            "format": "binary",
            "path": dir.join("geoip-cn.srs").to_string_lossy(),
            "download_url": "https://169.254.169.254/latest/meta-data/"
        }]);
        let err = sanitize(cfg, &ctx(&dir)).expect_err("download_url");
        assert_eq!(err.pointer, "/route/rule_set/0/download_url");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn local_rule_set_without_path_is_rejected() {
        let dir = temp_dir("rs-no-path");
        let mut cfg = minimal_allowed_config();
        cfg["route"]["rule_set"] = json!([{
            "type": "local",
            "tag": "geoip-cn",
            "format": "binary"
        }]);
        let err = sanitize(cfg, &ctx(&dir)).expect_err("missing path");
        assert_eq!(err.pointer, "/route/rule_set/0/path");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn tun_inbound_allowlist_matches_generator() {
        let dir = temp_dir("tun");
        let mut cfg = minimal_allowed_config();
        cfg["inbounds"] = json!([
            {
                "type": "mixed",
                "tag": "mixed-in",
                "listen": "127.0.0.1",
                "listen_port": 17890
            },
            {
                "type": "tun",
                "tag": "tun-in",
                "interface_name": "utun420",
                "address": ["172.19.0.1/30", "fdfe:dcba:9876::1/126"],
                "mtu": 9000,
                "auto_route": true,
                "strict_route": true,
                "stack": "system",
                "route_exclude_address": ["10.0.0.0/8"],
                "loopback_address": ["127.0.0.1", "::1"]
            }
        ]);
        sanitize(cfg, &ctx(&dir)).expect("tun inbound");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn helper_overrides_log_output_and_cache_path() {
        let dir = temp_dir("ovr");
        let log = dir.join("core.log");
        let cache = dir.join("cache.db");
        let mut cfg = minimal_allowed_config();
        cfg["experimental"]["cache_file"] = json!({ "enabled": true, "path": "/tmp/evil.db" });
        let ctx = GuardContext {
            data_dir: dir.clone(),
            resources_dir: dir.clone(),
            log_output: Some(log.clone()),
            cache_file_path: Some(cache.clone()),
        };
        let after = sanitize(cfg, &ctx).expect("ovr");
        assert_eq!(after["log"]["output"], log.to_string_lossy().as_ref());
        assert_eq!(
            after["experimental"]["cache_file"]["path"],
            cache.to_string_lossy().as_ref()
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn wireguard_system_is_rejected() {
        let dir = temp_dir("wg");
        let mut cfg = minimal_allowed_config();
        cfg["outbounds"] = json!([{
            "type": "wireguard",
            "tag": "wg",
            "system": true
        }]);
        let err = sanitize(cfg, &ctx(&dir)).expect_err("wg");
        assert!(err.pointer.contains("system"), "{}", err.pointer);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn inbound_unknown_key_is_rejected() {
        let dir = temp_dir("in");
        let mut cfg = minimal_allowed_config();
        cfg["inbounds"][0]["sniff"] = json!(true);
        let err = sanitize(cfg, &ctx(&dir)).expect_err("sniff");
        assert!(err.pointer.contains("sniff"), "{}", err.pointer);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn check_outbound_skips_tor_for_subscription() {
        let err = check_outbound(&json!({"type": "tor", "tag": "x"})).expect_err("tor");
        assert!(err.message.contains("tor"));
        check_outbound(&json!({
            "type": "shadowsocks",
            "tag": "n",
            "server": "1.1.1.1",
            "server_port": 443,
            "method": "aes-128-gcm",
            "password": "x"
        }))
        .expect("ss");
    }

    #[test]
    fn empty_config_is_rejected() {
        let dir = temp_dir("empty");
        let err = sanitize(json!({}), &ctx(&dir)).expect_err("empty");
        assert!(err.pointer.contains("outbounds") || err.message.contains("outbounds"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn ws_transport_path_is_allowed() {
        let dir = temp_dir("ws");
        let mut cfg = minimal_allowed_config();
        cfg["outbounds"] = json!([{
            "type": "vmess",
            "tag": "n",
            "server": "1.1.1.1",
            "server_port": 443,
            "uuid": "00000000-0000-0000-0000-000000000000",
            "transport": { "type": "ws", "path": "/ws", "headers": { "Host": "cdn.example.com" } }
        }]);
        sanitize(cfg.clone(), &ctx(&dir)).expect("ws transport path");
        check_outbound(&cfg["outbounds"][0]).expect("check_outbound ws");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn doh_dns_server_path_is_allowed() {
        let dir = temp_dir("doh");
        let mut cfg = minimal_allowed_config();
        cfg["dns"] = json!({
            "servers": [
                { "type": "local", "tag": "local" },
                { "type": "https", "tag": "remote-dns", "server": "1.1.1.1", "server_port": 443,
                  "path": "/dns-query", "detour": "direct" }
            ],
            "final": "remote-dns"
        });
        sanitize(cfg, &ctx(&dir)).expect("doh path");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn dns_hosts_path_is_rejected() {
        let dir = temp_dir("hosts");
        let mut cfg = minimal_allowed_config();
        cfg["dns"] = json!({
            "servers": [
                { "type": "hosts", "tag": "evil", "path": "/etc/passwd" },
                { "type": "local", "tag": "local" }
            ],
            "final": "local"
        });
        let err = sanitize(cfg, &ctx(&dir)).expect_err("hosts");
        assert!(err.pointer.contains("/dns/servers/0"), "{}", err.pointer);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn dns_hosts_type_is_rejected_without_path() {
        let dir = temp_dir("hosts-type");
        let mut cfg = minimal_allowed_config();
        cfg["dns"] = json!({
            "servers": [
                { "type": "hosts", "tag": "evil", "predefined": { "example.com": ["127.0.0.1"] } },
                { "type": "local", "tag": "local" }
            ],
            "final": "local"
        });
        let err = sanitize(cfg, &ctx(&dir)).expect_err("hosts type");
        assert_eq!(err.pointer, "/dns/servers/0/type");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn doh3_dns_server_path_is_allowed() {
        let dir = temp_dir("h3");
        let mut cfg = minimal_allowed_config();
        cfg["dns"] = json!({
            "servers": [
                { "type": "local", "tag": "local" },
                { "type": "h3", "tag": "remote-dns", "server": "1.1.1.1", "server_port": 443,
                  "path": "/dns-query", "detour": "direct" }
            ],
            "final": "remote-dns"
        });
        sanitize(cfg, &ctx(&dir)).expect("h3 path");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn geoip_rule_set_in_data_dir_is_accepted() {
        let base = temp_dir("geoip-split");
        let data = base.join("data");
        let resources = base.join("app-resources");
        let srs = data.join("geoip").join("geoip-cn.srs");
        fs::create_dir_all(srs.parent().unwrap()).unwrap();
        fs::create_dir_all(&resources).unwrap();
        fs::write(&srs, b"x").unwrap();
        let mut cfg = minimal_allowed_config();
        cfg["route"]["rule_set"] = json!([{
            "type": "local",
            "tag": "geoip-cn",
            "format": "binary",
            "path": srs.to_string_lossy(),
        }]);
        let ctx = GuardContext {
            data_dir: data,
            resources_dir: resources,
            log_output: None,
            cache_file_path: None,
        };
        sanitize(cfg, &ctx).expect("geoip under data_dir");
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn endpoints_and_services_are_rejected() {
        let dir = temp_dir("ep");
        let mut cfg = minimal_allowed_config();
        cfg["endpoints"] = json!([{
            "type": "wireguard",
            "tag": "wg",
            "system": true,
            "state_directory": "/etc/cron.d"
        }]);
        let err = sanitize(cfg, &ctx(&dir)).expect_err("endpoints");
        assert_eq!(err.pointer, "/endpoints");

        let mut cfg = minimal_allowed_config();
        cfg["services"] = json!([{ "type": "derp", "tag": "derp" }]);
        let err = sanitize(cfg, &ctx(&dir)).expect_err("services");
        assert_eq!(err.pointer, "/services");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn check_outbound_accepts_shadowtls() {
        check_outbound(&json!({
            "type": "shadowtls",
            "tag": "st",
            "server": "1.1.1.1",
            "server_port": 443,
            "password": "x",
            "version": 3
        }))
        .expect("shadowtls");
    }

    #[test]
    fn mixed_inbound_requires_loopback_or_unspecified_listen() {
        let dir = temp_dir("mixed-listen");
        let mut missing = minimal_allowed_config();
        missing["inbounds"][0]
            .as_object_mut()
            .unwrap()
            .remove("listen");
        let err = sanitize(missing, &ctx(&dir)).expect_err("missing listen");
        assert_eq!(err.pointer, "/inbounds/0/listen");

        let mut unicast = minimal_allowed_config();
        unicast["inbounds"][0]["listen"] = json!("1.2.3.4");
        let err = sanitize(unicast, &ctx(&dir)).expect_err("unicast");
        assert_eq!(err.pointer, "/inbounds/0/listen");

        let mut lan = minimal_allowed_config();
        lan["inbounds"][0]["listen"] = json!("0.0.0.0");
        sanitize(lan, &ctx(&dir)).expect("allow_lan unspecified");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn mixed_inbound_privileged_port_is_rejected() {
        let dir = temp_dir("mixed-port");
        let mut cfg = minimal_allowed_config();
        cfg["inbounds"][0]["listen_port"] = json!(443);
        let err = sanitize(cfg, &ctx(&dir)).expect_err("privileged");
        assert_eq!(err.pointer, "/inbounds/0/listen_port");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn experimental_v2ray_api_and_debug_are_rejected() {
        let dir = temp_dir("exp");
        let mut cfg = minimal_allowed_config();
        cfg["experimental"]["v2ray_api"] = json!({ "listen": "0.0.0.0:8080" });
        let err = sanitize(cfg, &ctx(&dir)).expect_err("v2ray");
        assert_eq!(err.pointer, "/experimental/v2ray_api");

        let mut cfg = minimal_allowed_config();
        cfg["experimental"]["debug"] = json!({ "listen": "127.0.0.1:1" });
        let err = sanitize(cfg, &ctx(&dir)).expect_err("debug");
        assert_eq!(err.pointer, "/experimental/debug");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn urltest_metadata_url_is_rejected_and_gstatic_http_is_allowed() {
        let dir = temp_dir("urltest");
        let mut cfg = minimal_allowed_config();
        cfg["outbounds"] = json!([
            {
                "type": "urltest",
                "tag": "auto",
                "outbounds": ["direct"],
                "url": "http://169.254.169.254/latest/meta-data"
            },
            { "type": "direct", "tag": "direct" }
        ]);
        let err = sanitize(cfg, &ctx(&dir)).expect_err("metadata");
        assert_eq!(err.pointer, "/outbounds/0/url");

        check_outbound(&json!({
            "type": "urltest",
            "tag": "auto",
            "outbounds": ["n"],
            "url": "http://127.0.0.1/"
        }))
        .expect_err("loopback");

        let mut ok = minimal_allowed_config();
        ok["outbounds"] = json!([
            {
                "type": "urltest",
                "tag": "auto",
                "outbounds": ["direct"],
                "url": "http://www.gstatic.com/generate_204"
            },
            { "type": "direct", "tag": "direct" }
        ]);
        sanitize(ok, &ctx(&dir)).expect("gstatic");

        let mut fallback = minimal_allowed_config();
        fallback["outbounds"] = json!([
            {
                "type": "fallback",
                "tag": "fb",
                "outbounds": ["direct"],
                "url": "http://www.gstatic.com/generate_204"
            },
            { "type": "direct", "tag": "direct" }
        ]);
        sanitize(fallback, &ctx(&dir)).expect("fallback");

        let mut loadbalance = minimal_allowed_config();
        loadbalance["outbounds"] = json!([
            {
                "type": "loadbalance",
                "tag": "lb",
                "outbounds": ["direct"],
                "strategy": "round-robin"
            },
            { "type": "direct", "tag": "direct" }
        ]);
        sanitize(loadbalance, &ctx(&dir)).expect("loadbalance");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn route_process_path_is_allowed() {
        let dir = temp_dir("process-path");
        let mut cfg = minimal_allowed_config();
        cfg["route"]["rules"] = json!([
            { "process_path": ["/Applications/Foo.app/Contents/MacOS/Foo"], "outbound": "direct" }
        ]);
        sanitize(cfg, &ctx(&dir)).expect("process_path");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn retain_helpers_drop_hosts_dns_and_remote_rule_sets() {
        let mut dns = json!({
            "servers": [
                { "type": "local", "tag": "local" },
                { "type": "hosts", "tag": "evil", "path": "/etc/passwd" }
            ]
        });
        retain_allowed_dns_servers(&mut dns);
        assert_eq!(dns["servers"].as_array().unwrap().len(), 1);

        let mut sets = vec![
            json!({ "type": "remote", "tag": "r", "url": "https://evil.example/x" }),
            json!({ "type": "local", "tag": "ok", "path": "geoip/cn.srs" }),
            json!({ "type": "local", "tag": "empty" }),
        ];
        retain_local_rule_sets(&mut sets);
        assert_eq!(sets.len(), 1);
        assert_eq!(sets[0]["tag"], "ok");
    }

    #[test]
    fn read_config_file_rejects_symlink_and_oversize() {
        let dir = temp_dir("read");
        let real = dir.join("real.json");
        fs::write(&real, b"{\"ok\":true}").unwrap();
        assert_eq!(read_config_file(&real).expect("regular"), b"{\"ok\":true}");

        #[cfg(unix)]
        {
            let link = dir.join("link.json");
            std::os::unix::fs::symlink(&real, &link).unwrap();
            let err = read_config_file(&link).expect_err("symlink");
            assert!(
                err.message.contains("open config") || err.message.contains("regular file"),
                "{}",
                err.message
            );

            let fifo = dir.join("fifo.json");
            let c_path = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
            assert_eq!(unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) }, 0);
            let err = read_config_file(&fifo).expect_err("fifo");
            assert!(
                err.message.contains("regular file") || err.message.contains("open config"),
                "{}",
                err.message
            );
        }

        let huge = dir.join("huge.json");
        fs::write(&huge, vec![b'x'; MAX_CONFIG_BYTES + 1]).unwrap();
        let err = read_config_file(&huge).expect_err("huge");
        assert!(err.message.contains("exceeds"), "{}", err.message);
        let _ = fs::remove_dir_all(&dir);
    }
}
