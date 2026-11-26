// SPDX-License-Identifier: GPL-3.0-or-later

//! Linux TUN backend (`docs/tun.md`).
//!
//! Native sing-box ownership model: the elevated core — run by the injected
//! [`CoreCoordinator`] (production systemd `ice-helper` or dev `sudo` runner)
//! — owns the TUN adapter, its dual-stack addresses, and Linux policy routes
//! (`ip rule` + routing table). This backend records every observable mutation
//! boundary in the journal, verifies host state via `iproute2`, and never
//! deletes an unverified resource.
//!
//! Linux DNS:
//! - On `systemd-resolved` hosts (Ubuntu, Fedora, Arch, etc.), the backend
//!   attaches public resolvers and the `~.` global routing domain to the TUN
//!   interface itself via `resolvectl` through the privileged helper. Queries
//!   enter the TUN and are answered by sing-box's `hijack-dns` rule without
//!   mutating any physical NIC or `/etc/resolv.conf`; when the TUN closes
//!   (even on `kill -9`), the kernel and `systemd-resolved` drop the per-link
//!   DNS automatically.
//! - On hosts without `systemd-resolved` whose `/etc/resolv.conf` is a regular
//!   non-symlink file, the backend snapshots and atomically updates
//!   `/etc/resolv.conf` with compare-before-restore semantics.
//!
//! Identity: recovery verifies by the exact interface name *and* the Linux
//! kernel `ifindex` recorded in the journal — never "any tun".
//!
//! Compiled on all platforms so host-free tests run on every CI host.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use crate::backend::{
    AppliedTun, PreparedTun, RecoveryOutcome, TunBackend, TunCapability, TunConfig, TunHealth,
};
use crate::coordinator::CoreCoordinator;
use crate::error::{ErrorCode, TunError};
use crate::journal::{steps, CidrRecord, DnsSnapshot, JournalState, RouteRecord, TunJournal};
use crate::macos::{is_tunnel_interface, pin_outbound_interface};
use crate::routes;

/// Public resolvers routed into the TUN so port-53 traffic enters `icebox0`
/// and is answered by sing-box's `hijack-dns` rule instead of a LAN resolver.
const LINUX_TUN_DNS_SERVERS: [&str; 2] = ["223.5.5.5", "119.29.29.29"];

/// Prefix for auto-selected Linux TUN interfaces (`icebox0..icebox15`).
pub const LINUX_TUN_PREFIX: &str = "icebox";
const LINUX_TUN_MAX_INDEX: u32 = 16;

/// Sentinel service name used in `HelperCommand::SetDns` when managing a
/// static non-symlink `/etc/resolv.conf`.
pub const LINUX_RESOLV_CONF_SERVICE: &str = "resolv.conf";

const INTERFACE_APPEAR_TRIES: u32 = 15;
const INTERFACE_APPEAR_DELAY_MS: u64 = 200;
const APPLY_CONVERGE_TRIES: u32 = 15;
const APPLY_CONVERGE_DELAY_MS: u64 = 200;
const DNS_PROBE_TRIES: u32 = 5;
const DNS_PROBE_DELAY_MS: u64 = 50;
const INTERFACE_TEARDOWN_TRIES: u32 = 10;
const INTERFACE_TEARDOWN_DELAY_MS: u64 = 200;
const DEFAULT_ROUTE_TRIES: u32 = 10;
const DEFAULT_ROUTE_DELAY_MS: u64 = 150;

/// Active Linux host DNS management strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxDnsMode {
    /// `systemd-resolved` is running; attach resolvers + `~.` to the TUN link.
    SystemdResolved,
    /// `/etc/resolv.conf` is a regular file (not a symlink); update atomically.
    StaticResolvConf,
    /// Neither mechanism is safe to mutate (e.g. foreign symlinked resolv.conf).
    Unmanaged,
}

/// Host state of one Linux network interface from `ip -o link` / `ip -o addr`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinuxInterfaceState {
    /// Kernel interface index (`ifindex`), used as the adapter identity lock.
    pub ifindex: u32,
    /// Whether the link flags include `UP`.
    pub up: bool,
    /// Assigned IPv4/IPv6 CIDRs (e.g. `10.0.0.1/30`, `fdfe:dcba:9876::1/126`).
    pub addresses: Vec<String>,
}

/// Read-only Linux host inspection boundary. `ProcessLinuxHost` shells out to
/// `ip` and `resolvectl`; tests inject a fake.
pub trait LinuxHost {
    fn list_interface_names(&self) -> Result<Vec<String>, TunError>;
    fn interface_state(&self, name: &str) -> Result<Option<LinuxInterfaceState>, TunError>;
    fn route_interface(&self, destination: &str) -> Result<Option<String>, TunError>;
    fn default_ipv4_interface(&self) -> Result<Option<String>, TunError>;
    fn dns_mode(&self) -> LinuxDnsMode;
    fn dns_servers(&self, service: &str) -> Result<Vec<String>, TunError>;
}

/// Interface names that must not be pinned as sing-box `route.default_interface`
/// on Linux (tunnels, VPNs, container bridges, veth pairs, and loopback).
pub fn is_linux_virtual_interface(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    is_tunnel_interface(name)
        || lower == "lo"
        || lower.starts_with("icebox")
        || lower.starts_with("wg")
        || lower.starts_with("tailscale")
        || lower.starts_with("docker")
        || lower.starts_with("veth")
        || lower.starts_with("br-")
        || lower.starts_with("virbr")
        || lower.starts_with("cni")
        || lower.starts_with("flannel")
        || lower.starts_with("calico")
        || lower.starts_with("podman")
        || lower.starts_with("zt")
        || lower.starts_with("sit")
        || lower.starts_with("gre")
        || lower.starts_with("ip6tnl")
}

/// Whether `name` is a safe physical Linux NIC name for `route.default_interface`.
pub fn linux_outbound_interface_is_safe(name: &str) -> bool {
    if name.is_empty() || name.len() > 15 || is_linux_virtual_interface(name) {
        return false;
    }
    if name.starts_with('.') || name.starts_with('-') {
        return false;
    }
    name.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_'))
}

/// Linux `IFNAMSIZ - 1` (15-byte) TUN interface name check.
pub fn linux_tun_interface_name_valid(name: &str) -> bool {
    if name.is_empty() || name.len() > 15 || name == "." || name == ".." {
        return false;
    }
    name.is_ascii()
        && !name
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || matches!(c, '/' | '\\'))
}

/// Read-only Linux host probes backed by `ip`, `resolvectl`, and `/etc/resolv.conf`.
#[derive(Debug, Default)]
pub struct ProcessLinuxHost;

struct CommandOutput {
    status: Option<i32>,
    stdout: String,
    stderr: String,
}

fn run_command(program: &str, args: &[&str]) -> Result<CommandOutput, TunError> {
    let output = Command::new(program).args(args).output().map_err(|err| {
        TunError::new(
            ErrorCode::TunHealthcheckFailed,
            format!("run {program}: {err}"),
        )
    })?;
    Ok(CommandOutput {
        status: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

impl LinuxHost for ProcessLinuxHost {
    fn list_interface_names(&self) -> Result<Vec<String>, TunError> {
        let out = run_command("ip", &["-o", "link", "show"])?;
        if out.status != Some(0) {
            return Err(TunError::new(
                ErrorCode::TunHealthcheckFailed,
                format!("ip -o link show failed: {}", out.stderr.trim()),
            ));
        }
        Ok(parse_ip_link_names(&out.stdout))
    }

    fn interface_state(&self, name: &str) -> Result<Option<LinuxInterfaceState>, TunError> {
        let link = run_command("ip", &["-o", "link", "show", "dev", name])?;
        if link.status != Some(0) {
            return Ok(None);
        }
        let Some((ifindex, up)) = parse_ip_link_header(&link.stdout, name) else {
            return Ok(None);
        };
        let addr = run_command("ip", &["-o", "addr", "show", "dev", name])?;
        let addresses = if addr.status == Some(0) {
            parse_ip_addr_show(&addr.stdout)
        } else {
            Vec::new()
        };
        Ok(Some(LinuxInterfaceState {
            ifindex,
            up,
            addresses,
        }))
    }

    fn route_interface(&self, destination: &str) -> Result<Option<String>, TunError> {
        let probe = routes::route_probe_address(destination);
        let family = if probe.contains(':') { "-6" } else { "-4" };
        let out = run_command("ip", &[family, "route", "get", &probe])?;
        if out.status != Some(0) {
            return Ok(None);
        }
        Ok(parse_ip_route_get_dev(&out.stdout))
    }

    fn default_ipv4_interface(&self) -> Result<Option<String>, TunError> {
        let out = run_command("ip", &["-4", "route", "show", "default"])?;
        if out.status != Some(0) {
            return Ok(None);
        }
        Ok(parse_ip_default_route_interface(&out.stdout))
    }

    fn dns_mode(&self) -> LinuxDnsMode {
        if Command::new("resolvectl")
            .arg("status")
            .output()
            .is_ok_and(|out| out.status.success())
        {
            return LinuxDnsMode::SystemdResolved;
        }
        let resolv = Path::new("/etc/resolv.conf");
        if let Ok(meta) = std::fs::symlink_metadata(resolv) {
            if meta.file_type().is_file() {
                return LinuxDnsMode::StaticResolvConf;
            }
        }
        LinuxDnsMode::Unmanaged
    }

    fn dns_servers(&self, service: &str) -> Result<Vec<String>, TunError> {
        if service == LINUX_RESOLV_CONF_SERVICE {
            let raw = std::fs::read_to_string("/etc/resolv.conf").map_err(|err| {
                TunError::new(
                    ErrorCode::TunHealthcheckFailed,
                    format!("read /etc/resolv.conf: {err}"),
                )
            })?;
            return Ok(parse_resolv_conf_servers(&raw));
        }

        let dns_out = run_command("resolvectl", &["dns", service])?;
        if dns_out.status != Some(0) {
            if !Path::new(&format!("/sys/class/net/{service}")).exists() {
                return Ok(Vec::new());
            }
            return Err(TunError::new(
                ErrorCode::TunHealthcheckFailed,
                format!("resolvectl dns {service} failed: {}", dns_out.stderr.trim()),
            ));
        }
        let domain_out = run_command("resolvectl", &["domain", service])?;
        if domain_out.status != Some(0) || !parse_resolvectl_has_global_domain(&domain_out.stdout) {
            return Ok(Vec::new());
        }
        Ok(parse_resolvectl_dns_servers(&dns_out.stdout))
    }
}

/// Parse `ip -o link show` into interface names (stripping `@peer` suffixes).
pub fn parse_ip_link_names(output: &str) -> Vec<String> {
    let mut names = Vec::new();
    for line in output.lines() {
        let mut tokens = line.split_whitespace();
        let Some(_idx) = tokens.next() else { continue };
        let Some(raw_name) = tokens.next() else {
            continue;
        };
        let clean = clean_link_name(raw_name);
        if !clean.is_empty() {
            names.push(clean.to_string());
        }
    }
    names
}

/// Parse `ip -o link show dev <name>` for `(ifindex, up)`.
pub fn parse_ip_link_header(output: &str, expected_name: &str) -> Option<(u32, bool)> {
    let first = output.lines().next()?;
    let mut tokens = first.split_whitespace();
    let idx_token = tokens.next()?.trim_end_matches(':');
    let ifindex: u32 = idx_token.parse().ok()?;
    let name_token = clean_link_name(tokens.next()?);
    if name_token != expected_name {
        return None;
    }
    let flags_token = tokens.next().unwrap_or("");
    let flags = flags_token
        .trim_start_matches('<')
        .trim_end_matches('>')
        .split(',');
    let up = flags.into_iter().any(|f| f == "UP");
    Some((ifindex, up))
}

fn clean_link_name(raw: &str) -> &str {
    let without_colon = raw.trim_end_matches(':');
    without_colon
        .split_once('@')
        .map(|(name, _)| name)
        .unwrap_or(without_colon)
}

/// Parse `ip -o addr show dev <name>` into normalized CIDRs (`addr/prefix`).
/// Handles both standard (`inet 10.0.0.1/30`) and point-to-point
/// (`inet 10.0.0.1 peer 10.0.0.2/30`) forms.
pub fn parse_ip_addr_show(output: &str) -> Vec<String> {
    let mut addresses = Vec::new();
    for line in output.lines() {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        for (i, tok) in tokens.iter().enumerate() {
            if *tok != "inet" && *tok != "inet6" {
                continue;
            }
            let Some(addr_tok) = tokens.get(i + 1) else {
                continue;
            };
            let cleaned = addr_tok.split('%').next().unwrap_or(addr_tok);
            if cleaned.contains('/') {
                addresses.push(cleaned.to_string());
            } else if tokens.get(i + 2) == Some(&"peer") {
                if let Some(peer_tok) = tokens.get(i + 3) {
                    if let Some((_, prefix)) = peer_tok.split_once('/') {
                        addresses.push(format!("{cleaned}/{prefix}"));
                    }
                }
            }
        }
    }
    addresses
}

/// Parse `ip -4/-6 route get <addr>` for the outgoing `dev <iface>` token.
pub fn parse_ip_route_get_dev(output: &str) -> Option<String> {
    let first = output.lines().next()?;
    let mut tokens = first.split_whitespace();
    while let Some(tok) = tokens.next() {
        if tok == "dev" {
            return tokens.next().map(str::to_string);
        }
    }
    None
}

/// Parse `ip -4 route show default` and pick the first safe physical NIC.
pub fn parse_ip_default_route_interface(output: &str) -> Option<String> {
    for line in output.lines() {
        let mut tokens = line.split_whitespace();
        while let Some(tok) = tokens.next() {
            if tok == "dev" {
                if let Some(iface) = tokens.next() {
                    if linux_outbound_interface_is_safe(iface) {
                        return Some(iface.to_string());
                    }
                }
            }
        }
    }
    None
}

/// Parse `resolvectl dns <link>` output (`Link 5 (icebox0): 223.5.5.5 119.29.29.29`).
pub fn parse_resolvectl_dns_servers(output: &str) -> Vec<String> {
    let mut servers = Vec::new();
    for line in output.lines() {
        let after_colon = line.split_once(':').map(|(_, rhs)| rhs).unwrap_or(line);
        for token in after_colon.split_whitespace() {
            // Strip optional `%iface` or `#hostname` DoT suffix.
            let candidate = token
                .split(['%', '#'])
                .next()
                .unwrap_or(token)
                .trim_matches(['[', ']']);
            if candidate.parse::<std::net::IpAddr>().is_ok() {
                servers.push(candidate.to_string());
            }
        }
    }
    servers
}

/// Whether `resolvectl domain <link>` includes the global routing domain `~.`.
pub fn parse_resolvectl_has_global_domain(output: &str) -> bool {
    output.lines().any(|line| {
        let after_colon = line.split_once(':').map(|(_, rhs)| rhs).unwrap_or(line);
        after_colon.split_whitespace().any(|tok| tok == "~.")
    })
}

/// Parse `nameserver <ip>` lines from `/etc/resolv.conf`.
pub fn parse_resolv_conf_servers(content: &str) -> Vec<String> {
    let mut servers = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }
        let mut tokens = trimmed.split_whitespace();
        if tokens.next() == Some("nameserver") {
            if let Some(addr) = tokens.next() {
                let candidate = addr.split('%').next().unwrap_or(addr);
                if candidate.parse::<std::net::IpAddr>().is_ok() {
                    servers.push(candidate.to_string());
                }
            }
        }
    }
    servers
}

fn dns_snapshot(service: &str, servers: &[String]) -> String {
    format!("{service}\n{}", servers.join(","))
}

fn dns_snapshot_parts(snapshot: &str) -> (String, Vec<String>) {
    let (service, servers) = snapshot.split_once('\n').unwrap_or((snapshot, ""));
    (
        service.to_string(),
        servers
            .split(',')
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect(),
    )
}

fn validate_cidr(cidr: &str, ipv6: bool) -> Result<(), TunError> {
    let (addr, prefix) = cidr.split_once('/').ok_or_else(|| {
        TunError::new(
            ErrorCode::TunApplyFailed,
            format!("tun address must be a CIDR, got {cidr}"),
        )
    })?;
    let prefix: u32 = prefix.parse().map_err(|_| {
        TunError::new(
            ErrorCode::TunApplyFailed,
            format!("tun address has a non-numeric prefix: {cidr}"),
        )
    })?;
    let parsed: Result<(), _> = if ipv6 {
        addr.parse::<std::net::Ipv6Addr>().map(|_| ())
    } else {
        addr.parse::<std::net::Ipv4Addr>().map(|_| ())
    };
    parsed.map_err(|_| {
        TunError::new(
            ErrorCode::TunApplyFailed,
            format!("invalid tun address: {cidr}"),
        )
    })?;
    let max = if ipv6 { 128 } else { 32 };
    if prefix == 0 || prefix > max {
        return Err(TunError::new(
            ErrorCode::TunApplyFailed,
            format!("tun address prefix must be in 1..={max}, got {prefix}"),
        ));
    }
    Ok(())
}

/// Linux TUN backend (native sing-box ownership + helper IPC).
pub struct LinuxTunBackend {
    owner_token: String,
    host: Box<dyn LinuxHost + Send>,
    coordinator: Box<dyn CoreCoordinator + Send>,
    config_path: PathBuf,
    journal_path: Option<PathBuf>,
}

struct OwnedState {
    addresses: Vec<CidrRecord>,
    routes: Vec<RouteRecord>,
}

impl LinuxTunBackend {
    pub fn new(
        owner_token: impl Into<String>,
        host: Box<dyn LinuxHost + Send>,
        coordinator: Box<dyn CoreCoordinator + Send>,
        config_path: PathBuf,
    ) -> Self {
        Self {
            owner_token: owner_token.into(),
            host,
            coordinator,
            config_path,
            journal_path: None,
        }
    }

    pub fn with_journal(mut self, path: PathBuf) -> Self {
        self.journal_path = Some(path);
        self
    }

    fn journal_record(
        &self,
        step: &str,
        mutate: impl FnOnce(&mut TunJournal),
    ) -> Result<(), TunError> {
        let Some(path) = &self.journal_path else {
            return Ok(());
        };
        let mut journal = TunJournal::load(path)?
            .unwrap_or_else(|| TunJournal::new("unknown".into(), self.owner_token.clone()));
        journal.last_completed_step = step.to_string();
        journal.updated_at = ice_tun_journal::now_rfc3339();
        mutate(&mut journal);
        journal.save(path)
    }

    fn rollback_after_apply_failure(&mut self, apply_err: TunError) -> TunError {
        match self.coordinator.stop() {
            Ok(()) => apply_err,
            Err(stop_err) => TunError::new(
                ErrorCode::TunRecoveryRequired,
                format!(
                    "apply failed ({}) and elevated core cleanup was not verified ({})",
                    apply_err.message, stop_err.message
                ),
            ),
        }
    }

    fn rollback_dns_after_apply_failure(
        &mut self,
        service: &str,
        before: &[String],
        apply_err: TunError,
    ) -> TunError {
        let dns_result = self.coordinator.set_dns(service, before);
        let core_result = self.coordinator.stop();
        match (dns_result, core_result) {
            (Ok(()), Ok(())) => apply_err,
            (dns_result, core_result) => {
                let dns_error = dns_result
                    .err()
                    .map(|err| err.message)
                    .unwrap_or_else(|| "ok".into());
                let core_error = core_result
                    .err()
                    .map(|err| err.message)
                    .unwrap_or_else(|| "ok".into());
                TunError::new(
                    ErrorCode::TunRecoveryRequired,
                    format!(
                        "apply failed ({}); DNS cleanup: {}; elevated core cleanup: {}",
                        apply_err.message, dns_error, core_error
                    ),
                )
            }
        }
    }

    fn wait_physical_default_interface(&self) -> Result<String, TunError> {
        let mut last = String::from("no default route");
        for _ in 0..DEFAULT_ROUTE_TRIES {
            match self.host.default_ipv4_interface()? {
                Some(iface) if linux_outbound_interface_is_safe(&iface) => return Ok(iface),
                Some(iface) => last = format!("default route on non-physical interface {iface}"),
                None => last = "no IPv4 default route".into(),
            }
            std::thread::sleep(Duration::from_millis(DEFAULT_ROUTE_DELAY_MS));
        }
        Err(TunError::new(
            ErrorCode::TunApplyFailed,
            format!("no physical default route before TUN start ({last})"),
        ))
    }

    fn probe_free_interface(&self, start_index: u32) -> Result<String, TunError> {
        let existing: std::collections::HashSet<String> =
            self.host.list_interface_names()?.into_iter().collect();
        (start_index..LINUX_TUN_MAX_INDEX)
            .map(|idx| format!("{LINUX_TUN_PREFIX}{idx}"))
            .find(|candidate| !existing.contains(candidate))
            .ok_or_else(|| {
                TunError::new(
                    ErrorCode::TunApplyFailed,
                    format!(
                        "no free Linux TUN interface in {LINUX_TUN_PREFIX}{start_index}..{LINUX_TUN_PREFIX}{}",
                        LINUX_TUN_MAX_INDEX - 1
                    ),
                )
            })
    }

    fn resolve_interface_name(&self, requested: Option<&str>) -> Result<String, TunError> {
        match requested {
            Some(name) => {
                if !linux_tun_interface_name_valid(name) {
                    return Err(TunError::new(
                        ErrorCode::TunApplyFailed,
                        format!("invalid Linux TUN interface name: {name}"),
                    ));
                }
                let existing = self.host.list_interface_names()?;
                if existing.iter().any(|iface| iface == name) {
                    tracing::warn!(
                        name,
                        "Linux TUN interface name already in use; probing a free icebox<N>"
                    );
                    let next_idx = name
                        .strip_prefix(LINUX_TUN_PREFIX)
                        .and_then(|s| s.parse::<u32>().ok())
                        .map(|n| n.saturating_add(1))
                        .unwrap_or(0);
                    self.probe_free_interface(next_idx)
                } else {
                    Ok(name.to_string())
                }
            }
            None => self.probe_free_interface(0),
        }
    }

    fn route_resolves_to(&self, destination: &str, name: &str) -> Result<bool, TunError> {
        for probe in routes::route_probe_addresses(destination) {
            if self.host.route_interface(&probe)?.as_deref() == Some(name) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn owned_routes_remain(&self, applied: &AppliedTun, name: &str) -> Result<bool, TunError> {
        for route in applied.routes.iter().filter(|r| r.owned) {
            if self.route_resolves_to(&route.destination, name)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn observe_owned_state(&self, config: &TunConfig, name: &str) -> Result<OwnedState, TunError> {
        let expected_addresses = &config.addresses;
        let expected_routes = routes::auto_route_destinations(config);
        let mut last_diagnostic = String::from("interface not observed");
        for _ in 0..APPLY_CONVERGE_TRIES {
            let state = self.host.interface_state(name)?;
            let missing_addresses = state.as_ref().map_or_else(
                || expected_addresses.to_vec(),
                |interface| {
                    expected_addresses
                        .iter()
                        .filter(|address| !interface.addresses.contains(*address))
                        .cloned()
                        .collect::<Vec<_>>()
                },
            );
            let addresses_ok = state.is_some() && missing_addresses.is_empty();
            let mut missing_route = None;
            if addresses_ok {
                for destination in &expected_routes {
                    if self.route_resolves_to(destination, name)? {
                        continue;
                    }
                    missing_route = Some(format!(
                        "{destination} -> {}",
                        self.host
                            .route_interface(&routes::route_probe_address(destination))?
                            .unwrap_or_else(|| "<none>".into())
                    ));
                    break;
                }
            }
            let routes_ok = addresses_ok && missing_route.is_none();
            if routes_ok {
                let owned_addresses = expected_addresses
                    .iter()
                    .map(|cidr| CidrRecord {
                        cidr: cidr.clone(),
                        owned: true,
                    })
                    .collect();
                let owned_routes = expected_routes
                    .iter()
                    .map(|destination| RouteRecord {
                        destination: destination.clone(),
                        gateway: None,
                        metric: 0,
                        owned: true,
                    })
                    .collect();
                return Ok(OwnedState {
                    addresses: owned_addresses,
                    routes: owned_routes,
                });
            }
            let interface_summary = state.map_or_else(
                || "<missing>".to_string(),
                |interface| {
                    format!(
                        "ifindex={} up={} addresses={:?}",
                        interface.ifindex, interface.up, interface.addresses
                    )
                },
            );
            last_diagnostic = format!(
                "{interface_summary}; missing_addresses={missing_addresses:?}; missing_route={missing_route:?}"
            );
            std::thread::sleep(Duration::from_millis(APPLY_CONVERGE_DELAY_MS));
        }
        Err(TunError::new(
            ErrorCode::TunHealthcheckFailed,
            format!(
                "interface {name} did not converge to the required addresses and routes within {} ms; last observation: {last_diagnostic}",
                APPLY_CONVERGE_TRIES * APPLY_CONVERGE_DELAY_MS as u32
            ),
        ))
    }

    fn dns_matches_after(&self, after: &DnsSnapshot) -> Option<bool> {
        self.dns_matches_after_retries(after, DNS_PROBE_TRIES, DNS_PROBE_DELAY_MS)
    }

    fn dns_matches_after_until_deadline(&self, after: &DnsSnapshot) -> Option<bool> {
        self.dns_matches_after_retries(after, INTERFACE_APPEAR_TRIES, INTERFACE_APPEAR_DELAY_MS)
    }

    fn dns_matches_after_retries(
        &self,
        after: &DnsSnapshot,
        tries: u32,
        delay_ms: u64,
    ) -> Option<bool> {
        let (service, expected) = dns_snapshot_parts(&after.platform_snapshot);
        let mut last_err = None;
        for attempt in 0..tries {
            match self.host.dns_servers(&service) {
                Ok(current) => return Some(current == expected),
                Err(err) => {
                    last_err = Some(err);
                    if attempt + 1 < tries {
                        std::thread::sleep(Duration::from_millis(delay_ms));
                    }
                }
            }
        }
        if let Some(err) = last_err {
            tracing::error!(
                error = %err,
                service = %service,
                tries,
                "linux tun dns probe failed"
            );
        }
        None
    }

    /// `(dns_consistent, dns_owned)` for an applied Linux capture.
    ///
    /// When `dns_after` targets the TUN link itself (`systemd-resolved` per-link)
    /// and the kernel has already destroyed the interface, per-link DNS is gone
    /// too (`dns_owned = false`).
    fn dns_verify_flags(&self, applied: &AppliedTun, interface_gone: bool) -> (bool, bool) {
        match &applied.dns_after {
            None => (true, false),
            Some(after) => {
                let (service, _) = dns_snapshot_parts(&after.platform_snapshot);
                if interface_gone
                    && service != LINUX_RESOLV_CONF_SERVICE
                    && applied.interface_name.as_deref() == Some(service.as_str())
                {
                    return (false, false);
                }
                match self.dns_matches_after(after) {
                    Some(true) => (true, true),
                    Some(false) => (false, false),
                    None => (false, true),
                }
            }
        }
    }
}

impl TunBackend for LinuxTunBackend {
    fn capability(&self) -> TunCapability {
        TunCapability {
            supported: true,
            reason: None,
            ipv4: true,
            ipv6: true,
            dns_hijack: true,
        }
    }

    fn prepare(&self, config: &TunConfig) -> Result<PreparedTun, TunError> {
        if config.addresses.is_empty() {
            return Err(TunError::new(
                ErrorCode::TunApplyFailed,
                "tun config requires at least one address",
            ));
        }
        if !routes::has_v4(&config.addresses) {
            return Err(TunError::new(
                ErrorCode::TunApplyFailed,
                "tun config must include an IPv4 address (IPv4 is mandatory)",
            ));
        }
        if !routes::has_v6(&config.addresses) {
            return Err(TunError::new(
                ErrorCode::TunApplyFailed,
                "tun config must include an IPv6 address (dual-stack lock: an IPv4-only tun silently leaks IPv6)",
            ));
        }
        for cidr in &config.addresses {
            validate_cidr(cidr, cidr.contains(':'))?;
        }
        if !(1280..=9000).contains(&config.mtu) {
            return Err(TunError::new(
                ErrorCode::TunApplyFailed,
                format!("tun mtu must be in 1280..=9000, got {}", config.mtu),
            ));
        }
        let interface_name = self.resolve_interface_name(config.interface_name.as_deref())?;
        Ok(PreparedTun {
            config: TunConfig {
                interface_name: Some(interface_name),
                ..config.clone()
            },
        })
    }

    fn apply(&mut self, prepared: &PreparedTun) -> Result<AppliedTun, TunError> {
        let config = &prepared.config;
        let Some(name) = config.interface_name.as_deref() else {
            return Err(TunError::new(
                ErrorCode::TunApplyFailed,
                "prepare must resolve the interface name before apply",
            ));
        };
        let expected_addresses = config.addresses.clone();
        let expected_routes = routes::auto_route_destinations(config);

        // Pin physical default NIC before starting sing-box so Direct / proxy
        // dials never loop into the TUN interface.
        let default_iface = self.wait_physical_default_interface()?;
        pin_outbound_interface(&self.config_path, &default_iface)?;

        let dns_mode = if config.dns_hijack {
            self.host.dns_mode()
        } else {
            LinuxDnsMode::Unmanaged
        };

        let core_pid = self.coordinator.start_with_config(&self.config_path)?;
        let mut state = None;
        for _ in 0..INTERFACE_APPEAR_TRIES {
            match self.host.interface_state(name) {
                Ok(Some(found)) => {
                    state = Some(found);
                    break;
                }
                Ok(None) => {}
                Err(err) => {
                    return Err(self.rollback_after_apply_failure(err));
                }
            }
            std::thread::sleep(Duration::from_millis(INTERFACE_APPEAR_DELAY_MS));
        }
        let Some(initial_state) = state else {
            return Err(self.rollback_after_apply_failure(TunError::new(
                ErrorCode::TunHealthcheckFailed,
                format!(
                    "core started but interface {name} is not present after {} ms",
                    INTERFACE_APPEAR_TRIES * INTERFACE_APPEAR_DELAY_MS as u32
                ),
            )));
        };
        let interface_id = Some(initial_state.ifindex.to_string());

        if let Err(err) = self.journal_record(steps::INTERFACE_CREATED, |journal| {
            journal.interface_name = Some(name.to_string());
            journal.interface_id = interface_id.clone();
            journal.expected_addresses = expected_addresses.clone();
            journal.expected_routes = expected_routes.clone();
        }) {
            return Err(self.rollback_after_apply_failure(err));
        }

        let observed = match self.observe_owned_state(config, name) {
            Ok(observed) => observed,
            Err(err) => {
                return Err(self.rollback_after_apply_failure(err));
            }
        };

        if let Err(err) = self.journal_record(steps::ADDRESSES_ASSIGNED, |journal| {
            journal.addresses = observed.addresses.clone();
        }) {
            return Err(self.rollback_after_apply_failure(err));
        }

        if let Err(err) = self.journal_record(steps::ROUTES_ADDED, |journal| {
            journal.routes = observed.routes.clone();
        }) {
            return Err(self.rollback_after_apply_failure(err));
        }

        // DNS interception (last mutation).
        let mut dns_before = None;
        let mut dns_after = None;
        let dns_target: Option<(&str, Vec<String>)> = match dns_mode {
            LinuxDnsMode::SystemdResolved => Some((name, Vec::new())),
            LinuxDnsMode::StaticResolvConf => {
                match self.host.dns_servers(LINUX_RESOLV_CONF_SERVICE) {
                    Ok(servers) => Some((LINUX_RESOLV_CONF_SERVICE, servers)),
                    Err(err) => return Err(self.rollback_after_apply_failure(err)),
                }
            }
            LinuxDnsMode::Unmanaged => {
                if config.dns_hijack {
                    tracing::warn!(
                        "Linux host uses neither systemd-resolved nor a regular /etc/resolv.conf; relying on sing-box hijack-dns without OS DNS mutation"
                    );
                }
                None
            }
        };

        if let Some((service, before)) = dns_target {
            let before_snap = DnsSnapshot {
                platform_snapshot: dns_snapshot(service, &before),
            };
            let target: Vec<String> = LINUX_TUN_DNS_SERVERS
                .iter()
                .map(|s| s.to_string())
                .collect();
            if let Err(err) = self.coordinator.set_dns(service, &target) {
                return Err(self.rollback_dns_after_apply_failure(service, &before, err));
            }
            let after_snap = DnsSnapshot {
                platform_snapshot: dns_snapshot(service, &target),
            };
            dns_before = Some(before_snap);
            dns_after = Some(after_snap);
            if let Err(err) = self.journal_record(steps::DNS_APPLIED, |journal| {
                journal.dns_before = dns_before.clone();
                journal.dns_after = dns_after.clone();
            }) {
                return Err(self.rollback_dns_after_apply_failure(service, &before, err));
            }
            if let Some(after) = &dns_after {
                if self.dns_matches_after_until_deadline(after) != Some(true) {
                    return Err(self.rollback_dns_after_apply_failure(
                        service,
                        &before,
                        TunError::new(
                            ErrorCode::TunHealthcheckFailed,
                            format!(
                                "DNS on {service} did not match the applied snapshot after {} ms",
                                INTERFACE_APPEAR_TRIES * INTERFACE_APPEAR_DELAY_MS as u32
                            ),
                        ),
                    ));
                }
            }
        }

        Ok(AppliedTun {
            interface_name: Some(name.to_string()),
            interface_id,
            addresses: observed.addresses,
            routes: observed.routes,
            expected_addresses,
            expected_routes,
            dns_before,
            dns_after,
            core_pid: Some(core_pid),
        })
    }

    fn verify(&self, applied: &AppliedTun) -> Result<TunHealth, TunError> {
        let Some(name) = applied.interface_name.as_deref() else {
            let (_dns_consistent, dns_owned) = self.dns_verify_flags(applied, true);
            return Ok(TunHealth {
                interface_up: false,
                addresses_present: false,
                routes_owned: false,
                dns_consistent: !dns_owned,
                control_path_reachable: true,
                nothing_owned: !dns_owned,
            });
        };
        let expected_id = applied.interface_id.as_deref();
        let state = self.host.interface_state(name)?;
        // Identity lock: exact name AND kernel ifindex must match the journal.
        let id_matches = match (&state, expected_id) {
            (Some(st), Some(id)) => st.ifindex.to_string() == id,
            _ => false,
        };
        let interface_up = state.as_ref().is_some_and(|st| st.up && id_matches);
        let addresses_present = state.as_ref().is_some_and(|st| {
            applied
                .expected_addresses
                .iter()
                .all(|addr| st.addresses.contains(addr))
        });
        let mut routes_owned = true;
        for destination in &applied.expected_routes {
            if !self.route_resolves_to(destination, name)? {
                routes_owned = false;
                break;
            }
        }
        let control_path = self.host.route_interface("127.0.0.1")?;
        let control_path_reachable = control_path.as_deref().is_some_and(|iface| iface != name);
        let interface_gone = state.is_none();
        let (dns_consistent, dns_owned) = self.dns_verify_flags(applied, interface_gone);
        let owned_routes_remain = self.owned_routes_remain(applied, name)?;
        let nothing_owned = interface_gone && !owned_routes_remain && !dns_owned;
        let health = TunHealth {
            interface_up,
            addresses_present,
            routes_owned,
            dns_consistent,
            control_path_reachable,
            nothing_owned,
        };
        if !health.all_ok() {
            tracing::warn!(
                ?health,
                name,
                "linux tun verify disagrees; expected addresses {:?}, routes {:?}",
                applied.expected_addresses,
                applied.expected_routes
            );
        }
        Ok(health)
    }

    fn restore(&mut self, applied: &AppliedTun) -> Result<(), TunError> {
        let mut restore_error: Option<TunError> = None;

        // For systemd-resolved per-link DNS (attached to the TUN link itself),
        // revert before stopping the core while the link is still present.
        let mut dns_already_restored = false;
        if let Some(after) = &applied.dns_after {
            let (service, expected) = dns_snapshot_parts(&after.platform_snapshot);
            if service != LINUX_RESOLV_CONF_SERVICE
                && applied.interface_name.as_deref() == Some(service.as_str())
            {
                if let Ok(Some(_)) = self.host.interface_state(&service) {
                    if let Ok(current) = self.host.dns_servers(&service) {
                        if current == expected {
                            if let Err(err) = self.coordinator.set_dns(&service, &[]) {
                                restore_error = Some(err);
                            } else {
                                dns_already_restored = true;
                            }
                        } else {
                            restore_error = Some(TunError::new(
                                ErrorCode::TunRecoveryRequired,
                                "system DNS on TUN link no longer matches dns_after; external change preserved",
                            ));
                            dns_already_restored = true;
                        }
                    }
                } else {
                    // Link already gone: flush caches best-effort and mark clean.
                    let _ = self.coordinator.set_dns(&service, &[]);
                    dns_already_restored = true;
                }
            }
        }

        self.coordinator.stop().map_err(|err| {
            TunError::new(
                err.code,
                format!("stop core during restore: {}", err.message),
            )
        })?;
        tracing::info!("linux restore: elevated core stopped");

        if let Err(err) = self.journal_record(steps::RESTORE_STARTED, |_| {}) {
            if restore_error.is_none() {
                restore_error = Some(err);
            }
        }

        let name = applied.interface_name.as_deref();
        let mut interface_gone = false;
        for _ in 0..INTERFACE_TEARDOWN_TRIES {
            let state = match name {
                Some(name) => self.host.interface_state(name)?,
                None => None,
            };
            if state.is_none() {
                interface_gone = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(INTERFACE_TEARDOWN_DELAY_MS));
        }
        if !interface_gone {
            return Err(TunError::new(
                ErrorCode::TunRecoveryRequired,
                format!(
                    "interface {} still present after core stop; removal needs the privileged helper",
                    name.unwrap_or("<unknown>")
                ),
            ));
        }
        tracing::info!("linux restore: interface teardown verified");

        if let Some(name) = name {
            if self.owned_routes_remain(applied, name)? {
                return Err(TunError::new(
                    ErrorCode::TunRecoveryRequired,
                    format!("owned routes still resolve to {name} after core stop"),
                ));
            }
        }

        if let Err(err) = self.journal_record(steps::ROUTES_REMOVED, |journal| {
            journal.routes.clear();
            journal.expected_routes.clear();
        }) {
            if restore_error.is_none() {
                restore_error = Some(err);
            }
        }
        if let Err(err) = self.journal_record(steps::INTERFACE_REMOVED, |journal| {
            journal.interface_name = None;
            journal.interface_id = None;
            journal.addresses.clear();
            journal.expected_addresses.clear();
        }) {
            if restore_error.is_none() {
                restore_error = Some(err);
            }
        }

        if let Some(after) = &applied.dns_after {
            if dns_already_restored {
                if restore_error.is_none() {
                    if let Err(err) = self.journal_record(steps::DNS_RESTORED, |journal| {
                        journal.dns_before = None;
                        journal.dns_after = None;
                    }) {
                        restore_error = Some(err);
                    }
                }
            } else {
                // Static `/etc/resolv.conf` compare-before-restore.
                let (service, expected) = dns_snapshot_parts(&after.platform_snapshot);
                let current = self.host.dns_servers(&service)?;
                if current == expected {
                    let (_, before) = dns_snapshot_parts(
                        applied
                            .dns_before
                            .as_ref()
                            .map(|b| b.platform_snapshot.as_str())
                            .unwrap_or(""),
                    );
                    if let Err(err) = self.coordinator.set_dns(&service, &before) {
                        if restore_error.is_none() {
                            restore_error = Some(err);
                        }
                    } else if let Err(err) = self.journal_record(steps::DNS_RESTORED, |journal| {
                        journal.dns_before = None;
                        journal.dns_after = None;
                    }) {
                        if restore_error.is_none() {
                            restore_error = Some(err);
                        }
                    }
                } else {
                    let err = TunError::new(
                        ErrorCode::TunRecoveryRequired,
                        "system DNS no longer matches the journal's dns_after snapshot; external change preserved",
                    );
                    if restore_error.is_none() {
                        restore_error = Some(err);
                    }
                }
            }
        }

        match restore_error {
            Some(err) => Err(TunError::new(
                ErrorCode::TunRecoveryRequired,
                format!(
                    "restore cleanup completed with unrecoverable state: {}",
                    err.message
                ),
            )),
            None => Ok(()),
        }
    }

    fn recover(&mut self, journal: &TunJournal) -> Result<RecoveryOutcome, TunError> {
        if journal.state == JournalState::Clean {
            return Ok(RecoveryOutcome::NothingToDo);
        }
        let applied = AppliedTun::from_journal(journal);
        let health = self.verify(&applied)?;
        if health.nothing_owned {
            return Ok(RecoveryOutcome::Cleaned);
        }
        match self.restore(&applied) {
            Ok(()) => {}
            Err(err) if err.code == ErrorCode::TunRecoveryRequired => {
                return Ok(RecoveryOutcome::RecoveryRequired);
            }
            Err(err) => return Err(err),
        }
        let health = self.verify(&applied)?;
        if health.nothing_owned {
            Ok(RecoveryOutcome::Cleaned)
        } else {
            Ok(RecoveryOutcome::RecoveryRequired)
        }
    }

    fn stop_elevated_core(&mut self) -> Result<(), TunError> {
        self.coordinator.stop()
    }

    fn reapply_dns_if_stale(&mut self, applied: &AppliedTun) -> Result<bool, TunError> {
        let Some(after) = &applied.dns_after else {
            return Ok(false);
        };
        let (service, expected) = dns_snapshot_parts(&after.platform_snapshot);
        let current = self.host.dns_servers(&service)?;
        if current == expected {
            return Ok(false);
        }
        self.coordinator.set_dns(&service, &expected)?;
        tracing::info!(
            service,
            ?expected,
            "re-applied Linux TUN DNS after wake / network change"
        );
        Ok(true)
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn attach_journal(&mut self, path: PathBuf) {
        self.journal_path = Some(path);
    }
}

#[cfg(test)]
mod parsing_tests {
    use super::*;

    #[test]
    fn linux_interface_classification_filters_virtual_and_tunnels() {
        assert!(linux_outbound_interface_is_safe("eth0"));
        assert!(linux_outbound_interface_is_safe("enp0s3"));
        assert!(linux_outbound_interface_is_safe("wlp2s0"));
        assert!(linux_outbound_interface_is_safe("eth0.100"));
        assert!(linux_outbound_interface_is_safe("usb-0"));

        for virtual_iface in [
            "lo",
            "tun0",
            "icebox0",
            "wg0",
            "tailscale0",
            "docker0",
            "veth1a2b3c",
            "br-abcdef1234",
            "virbr0",
            "utun4",
            "ppp0",
        ] {
            assert!(
                !linux_outbound_interface_is_safe(virtual_iface),
                "must reject {virtual_iface}"
            );
        }
        assert!(!linux_outbound_interface_is_safe("too-long-interface-name"));
        assert!(!linux_outbound_interface_is_safe("../eth0"));
    }

    #[test]
    fn parses_ip_link_and_addr_outputs() {
        let link = "1: lo: <LOOPBACK,UP,LOWER_UP> mtu 65536 qdisc noqueue state UNKNOWN mode DEFAULT\n\
                    2: enp0s3: <BROADCAST,MULTICAST,UP,LOWER_UP> mtu 1500 state UP\n\
                    5: veth123@if4: <BROADCAST,UP> mtu 1500\n\
                    9: icebox0: <POINTOPOINT,MULTICAST,NOARP,UP,LOWER_UP> mtu 9000 qdisc fq_codel state UNKNOWN\n";
        assert_eq!(
            parse_ip_link_names(link),
            vec!["lo", "enp0s3", "veth123", "icebox0"]
        );

        let one = "9: icebox0: <POINTOPOINT,MULTICAST,NOARP,UP,LOWER_UP> mtu 9000\n";
        assert_eq!(parse_ip_link_header(one, "icebox0"), Some((9, true)));
        assert_eq!(parse_ip_link_header(one, "icebox1"), None);

        let down = "9: icebox0: <POINTOPOINT,MULTICAST,NOARP> mtu 9000\n";
        assert_eq!(parse_ip_link_header(down, "icebox0"), Some((9, false)));

        let addr = "9: icebox0    inet 10.0.0.1/30 scope global icebox0\\ valid_lft forever\n\
                    9: icebox0    inet 172.19.0.1 peer 172.19.0.2/30 scope global icebox0\n\
                    9: icebox0    inet6 fdfe:dcba:9876::1/126 scope global\\ valid_lft forever\n";
        assert_eq!(
            parse_ip_addr_show(addr),
            vec!["10.0.0.1/30", "172.19.0.1/30", "fdfe:dcba:9876::1/126"]
        );
    }

    #[test]
    fn parses_ip_route_outputs() {
        assert_eq!(
            parse_ip_route_get_dev("8.8.8.8 dev icebox0 table 2022 src 10.0.0.1 uid 1000\n")
                .as_deref(),
            Some("icebox0")
        );
        assert_eq!(
            parse_ip_route_get_dev("local 127.0.0.1 dev lo src 127.0.0.1 uid 1000\n").as_deref(),
            Some("lo")
        );
        let default_routes = "default dev wg0 scope link metric 50\n\
                              default via 192.168.1.1 dev wlp2s0 proto dhcp metric 600\n";
        assert_eq!(
            parse_ip_default_route_interface(default_routes).as_deref(),
            Some("wlp2s0")
        );
    }

    #[test]
    fn parses_resolvectl_and_resolv_conf() {
        assert_eq!(
            parse_resolvectl_dns_servers("Link 9 (icebox0): 223.5.5.5 119.29.29.29\n"),
            vec!["223.5.5.5", "119.29.29.29"]
        );
        assert!(parse_resolvectl_dns_servers("Link 9 (icebox0):\n").is_empty());
        assert!(parse_resolvectl_has_global_domain("Link 9 (icebox0): ~.\n"));
        assert!(!parse_resolvectl_has_global_domain(
            "Link 2 (enp0s3): corp.local\n"
        ));

        let conf =
            "# comment\nsearch lan\nnameserver 192.168.1.1\nnameserver 2001:4860:4860::8888\n";
        assert_eq!(
            parse_resolv_conf_servers(conf),
            vec!["192.168.1.1", "2001:4860:4860::8888"]
        );
    }
}
