// SPDX-License-Identifier: GPL-3.0-or-later

//! Deny-by-default sanitizer for sing-box JSON that will be loaded by an
//! elevated core (macOS helper as root, Windows scheduled-task launcher as
//! Administrator).
//!
//! The unprivileged app (and a remote subscription) can write `config.json`.
//! This crate is the privileged-side content filter: unknown inbound keys,
//! disallowed outbound types, and filesystem-referencing keys are rejected
//! with a JSON pointer in the error. Callers write the sanitised object to a
//! root/admin-owned path and start sing-box from that copy.

use std::net::IpAddr;
use std::path::{Path, PathBuf};

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
    /// App data dir (reserved for callers; rule-set paths must still live
    /// under [`Self::resources_dir`]).
    pub data_dir: PathBuf,
    /// Bundled resources directory. `route.rule_set[].path` must canonicalise
    /// inside it (typically the app `resources/` folder that contains `geoip/`).
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

    validate_inbounds(cfg)?;
    validate_outbounds(cfg)?;
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
    }
    Ok(())
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
    if !host_is_loopback(host) {
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
    let resources = ctx.resources_dir.canonicalize().map_err(|err| {
        GuardError::new(
            "/route/rule_set",
            format!(
                "resources_dir {} cannot be canonicalised: {err}",
                ctx.resources_dir.display()
            ),
        )
    })?;
    for (idx, set) in sets.iter().enumerate() {
        let pointer = format!("/route/rule_set/{idx}/path");
        let Some(raw) = set.get("path").and_then(|v| v.as_str()) else {
            continue;
        };
        let candidate = {
            let p = Path::new(raw);
            if p.is_absolute() {
                p.to_path_buf()
            } else {
                ctx.resources_dir.join(p)
            }
        };
        let canon = candidate.canonicalize().map_err(|err| {
            GuardError::new(
                &pointer,
                format!("rule_set path {} cannot be canonicalised: {err}", raw),
            )
        })?;
        if !canon.starts_with(&resources) {
            return Err(GuardError::new(
                pointer,
                format!(
                    "rule_set path {} is outside resources_dir {}",
                    raw,
                    ctx.resources_dir.display()
                ),
            ));
        }
    }
    Ok(())
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
                if skip_forbidden_at(key, &child_pointer, ctx) {
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

fn skip_forbidden_at(key: &str, pointer: &str, ctx: Option<&GuardContext>) -> bool {
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
    false
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
            | "output"
            | "external_ui"
            | "external_ui_download_url"
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

fn host_is_loopback(host: &str) -> bool {
    let host = host.trim().trim_matches(['[', ']']);
    if matches!(host, "127.0.0.1" | "localhost" | "::1") {
        return true;
    }
    host.parse::<IpAddr>()
        .map(|ip| ip.is_loopback())
        .unwrap_or(false)
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
}
