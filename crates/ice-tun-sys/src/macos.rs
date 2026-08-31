//! macOS TUN backend (plan §5 T2; T0 locks §24.5).
//!
//! Native sing-box ownership model (T0 lock): the elevated core — run by the
//! injected [`CoreCoordinator`] — owns the utun adapter, its addresses, and
//! the routes. This backend records every observable mutation boundary in the
//! journal, verifies host state, and never deletes an unverified resource.
//!
//! macOS DNS: sing-box never mutates OS DNS (`scutil --dns` shape is owned
//! by the kernel, not the core). When `dns_hijack` is enabled the backend
//! points the primary network service's DNS at public resolvers so port-53
//! traffic enters the TUN and the config's `hijack-dns` route rule answers
//! from the sing-box DNS engine — a LAN resolver would otherwise bypass the
//! TUN (connected-subnet route) and answers stay GFW-poisoned. The mutation
//! runs through the elevated coordinator and is journaled
//! (`dns_before` / `dns_after`, compare-before-restore).
//!
//! Identity (T0 lock): recovery verifies by the exact interface name *and*
//! the utun numeric index (the kernel's unit number) recorded in the journal
//! — never "any utun".
//!
//! The module is compiled on every platform so the backend logic stays
//! host-free testable on all CI hosts; `create_backend` gates activation per
//! platform.

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use crate::backend::{
    AppliedTun, PreparedTun, RecoveryOutcome, TunBackend, TunCapability, TunConfig, TunHealth,
};
use crate::coordinator::CoreCoordinator;
use crate::error::{TunError, TunErrorCode};
use crate::journal::{steps, CidrRecord, DnsSnapshot, JournalState, RouteRecord, TunJournal};
use crate::routes;
use crate::routes::netmask_to_prefix;

/// Public resolvers the backend points the primary service's DNS at while
/// TUN capture is active with `dns_hijack`: they are not on the LAN, so
/// their port-53 traffic enters the TUN and the `hijack-dns` route rule
/// answers from the sing-box DNS engine. AliDNS + DNSPod are reachable from
/// mainland networks.
const MACOS_TUN_DNS_SERVERS: [&str; 2] = ["223.5.5.5", "119.29.29.29"];

/// Probe floor for a free utun index (T0 spike: keep `utun0..5` used by
/// other software untouched; probe a higher index, else fail closed).
const UTUN_PROBE_FLOOR: u32 = 200;
/// Bounded wait for the kernel to create the adapter after the elevated
/// core starts (dev `sudo` runner / helper hand-off: sing-box may take a
/// moment to bring the utun up).
const INTERFACE_APPEAR_TRIES: u32 = 15;
const INTERFACE_APPEAR_DELAY_MS: u64 = 200;
/// Bounded wait for the adapter to converge to the *required* addresses and
/// routes after it appears (dual-stack + full-route locks: a tun that lost
/// one family or route must fail closed, not be recorded as owned).
const APPLY_CONVERGE_TRIES: u32 = 15;
const APPLY_CONVERGE_DELAY_MS: u64 = 200;
/// Bounded wait for the kernel to tear down the adapter after a core stop
/// (spike: SIGTERM removes routes + interface; `kill -9` flushes them with
/// the fd close within ~2 s).
const INTERFACE_TEARDOWN_TRIES: u32 = 10;
const INTERFACE_TEARDOWN_DELAY_MS: u64 = 200;

/// Host state of one interface as reported by `ifconfig`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacInterfaceState {
    /// Interface is up (flags contain `UP`).
    pub up: bool,
    /// Addresses as CIDRs (e.g. `10.0.0.1/30`, `fdfe:dcba:9876::1/126`).
    pub addresses: Vec<String>,
}

/// Host reads the macOS backend needs. Implementations must be read-only:
/// they never mutate the OS. `ProcessMacOsHost` shells out to `ifconfig` /
/// `route`; tests inject a fake.
pub trait MacOsHost {
    /// All interface names (`ifconfig -l`).
    fn list_interface_names(&self) -> Result<Vec<String>, TunError>;
    /// Parsed state of one interface; `None` when it does not exist.
    fn interface_state(&self, name: &str) -> Result<Option<MacInterfaceState>, TunError>;
    /// The interface the kernel routes `destination` through
    /// (`route -n get`). `None` when no route exists (or the command
    /// reports no gateway).
    fn route_interface(&self, destination: &str) -> Result<Option<String>, TunError>;
    /// The network service owning the default route — the service whose DNS
    /// `dns_hijack` mutates. `None` when it cannot be determined.
    fn dns_service(&self) -> Result<Option<String>, TunError>;
    /// Current DNS server list configured for one network service.
    fn dns_servers(&self, service: &str) -> Result<Vec<String>, TunError>;
}

/// The utun numeric index, or `None` when the name is not `utun<N>`.
/// This index is the macOS kernel's adapter identity (T0 lock).
pub fn utun_index(name: &str) -> Option<u32> {
    let digits = name.strip_prefix("utun")?;
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// Host reads via subprocess (`ifconfig`, `route`). Read-only.
#[derive(Debug, Default)]
pub struct ProcessMacOsHost;

fn run_command(program: &str, args: &[&str]) -> Result<CommandOutput, TunError> {
    let output = Command::new(program).args(args).output().map_err(|err| {
        TunError::new(
            TunErrorCode::HealthcheckFailed,
            format!("run {program}: {err}"),
        )
    })?;
    Ok(CommandOutput {
        status: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

struct CommandOutput {
    status: Option<i32>,
    stdout: String,
    stderr: String,
}

impl MacOsHost for ProcessMacOsHost {
    fn list_interface_names(&self) -> Result<Vec<String>, TunError> {
        let out = run_command("ifconfig", &["-l"])?;
        if out.status != Some(0) {
            return Err(TunError::new(
                TunErrorCode::HealthcheckFailed,
                format!("ifconfig -l failed: {}", out.stderr.trim()),
            ));
        }
        Ok(parse_ifconfig_l(&out.stdout))
    }

    fn interface_state(&self, name: &str) -> Result<Option<MacInterfaceState>, TunError> {
        let out = run_command("ifconfig", &[name])?;
        if out.status != Some(0) {
            // `ifconfig <missing>` exits non-zero: the interface does not exist.
            return Ok(None);
        }
        Ok(parse_ifconfig_state(&out.stdout, name))
    }

    fn route_interface(&self, destination: &str) -> Result<Option<String>, TunError> {
        // IPv6 lookups need the explicit `-inet6` family on macOS.
        // `route -n get` accepts an address, not CIDR notation. The backend's
        // journal stores route prefixes because that is the ownership contract,
        // so probe a representative address inside the prefix. This also
        // avoids asking for `::`, which macOS treats as the default route.
        let probe = routes::route_probe_address(destination);
        let args: &[&str] = if probe.contains(':') {
            &["-n", "get", "-inet6", &probe]
        } else {
            &["-n", "get", &probe]
        };
        let out = run_command("route", args)?;
        if out.status != Some(0) {
            // No route to the destination (e.g. after teardown): nothing owned.
            return Ok(None);
        }
        Ok(parse_route_interface(&out.stdout))
    }

    fn dns_service(&self) -> Result<Option<String>, TunError> {
        // Default-route interface (e.g. `en0`).
        let Some(interface) = self.route_interface("0.0.0.0")? else {
            return Ok(None);
        };
        // Hardware port behind the interface (e.g. `Wi-Fi`).
        let ports = run_command("networksetup", &["-listallhardwareports"])?;
        let Some(port) = parse_hardware_port(&ports.stdout, &interface) else {
            return Ok(None);
        };
        // Network service that uses the hardware port (e.g. `Wi-Fi`).
        let order = run_command("networksetup", &["-listnetworkserviceorder"])?;
        Ok(parse_service_order(&order.stdout, &port))
    }

    fn dns_servers(&self, service: &str) -> Result<Vec<String>, TunError> {
        let out = run_command("networksetup", &["-getdnsservers", service])?;
        if out.status != Some(0) {
            return Ok(Vec::new());
        }
        Ok(parse_dns_servers(&out.stdout))
    }
}

/// `ifconfig -l` → space-separated interface names.
pub fn parse_ifconfig_l(output: &str) -> Vec<String> {
    output.split_whitespace().map(str::to_string).collect()
}

/// Parse `ifconfig <name>` for the interface's up-flag and address list.
/// Returns `None` when the output is not for `name`.
pub fn parse_ifconfig_state(output: &str, name: &str) -> Option<MacInterfaceState> {
    let mut lines = output.lines();
    let first = lines.next()?;
    if !first.starts_with(&format!("{name}:")) {
        return None;
    }
    let up = first.contains("UP");
    let mut addresses = Vec::new();
    for line in lines {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("inet ") {
            // `inet 10.0.0.1 --> 10.0.0.2 netmask 0xfffffffc`
            let tokens: Vec<&str> = rest.split_whitespace().collect();
            let addr = tokens.first()?;
            let netmask = tokens
                .iter()
                .position(|t| *t == "netmask")
                .and_then(|i| tokens.get(i + 1))
                .and_then(|nm| netmask_to_prefix(nm));
            if let Some(prefix) = netmask {
                addresses.push(format!("{addr}/{prefix}"));
            }
        } else if let Some(rest) = line.strip_prefix("inet6 ") {
            // `inet6 fdfe:dcba:9876::1 prefixlen 126`
            // link-local lines carry `%utun420 ... scopeid 0xf` — strip the scope.
            let tokens: Vec<&str> = rest.split_whitespace().collect();
            let addr = tokens.first()?.split('%').next()?;
            let prefix = tokens
                .iter()
                .position(|t| *t == "prefixlen")
                .and_then(|i| tokens.get(i + 1))
                .and_then(|p| p.parse::<u32>().ok());
            if let Some(prefix) = prefix {
                addresses.push(format!("{addr}/{prefix}"));
            }
        }
    }
    Some(MacInterfaceState { up, addresses })
}

/// `route -n get <dest>` → the `interface:` line.
pub fn parse_route_interface(output: &str) -> Option<String> {
    output
        .lines()
        .find_map(|line| line.trim().strip_prefix("interface: ").map(str::to_string))
}

/// `networksetup -listallhardwareports` → the hardware port name behind
/// `device` (e.g. `Wi-Fi` behind `en0`).
pub fn parse_hardware_port(output: &str, device: &str) -> Option<String> {
    let mut port: Option<String> = None;
    for line in output.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("Hardware Port: ") {
            port = Some(rest.to_string());
        } else if let Some(rest) = line.strip_prefix("Device: ") {
            if rest.trim() == device {
                return port;
            }
        }
    }
    None
}

/// `networksetup -listnetworkserviceorder` → the network service name that
/// uses `hardware_port` (the `(N) Name` line preceding its hardware-port
/// line).
pub fn parse_service_order(output: &str, hardware_port: &str) -> Option<String> {
    let mut previous: Option<String> = None;
    for line in output.lines() {
        let line = line.trim();
        if line.contains(&format!("Hardware Port: {hardware_port}")) {
            return previous;
        }
        // A service name line is `(N) Name` or `(*) Name`. The hardware-port
        // line above also starts with `(`, so it must be matched first.
        if let Some(rest) = line.strip_prefix('(') {
            if let Some(name) = rest.split_once(')') {
                let label = name.0.trim();
                if label == "*" || label.chars().all(|c| c.is_ascii_digit()) {
                    previous = Some(name.1.trim().to_string());
                }
            }
        }
    }
    None
}

/// `networksetup -getdnsservers <service>` → the configured DNS servers.
/// "There aren't any DNS Servers set on <service>." parses as empty.
pub fn parse_dns_servers(output: &str) -> Vec<String> {
    output
        .lines()
        .skip(1)
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("There aren't"))
        .filter(|line| line.parse::<std::net::IpAddr>().is_ok())
        .map(str::to_string)
        .collect()
}

/// Journal snapshot encoding: `"{service}\n{servers joined by ','}"`. The
/// service name is needed to restore / verify; the newline separator cannot
/// appear in a macOS service name.
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
            TunErrorCode::ApplyFailed,
            format!("tun address must be a CIDR, got {cidr}"),
        )
    })?;
    let prefix: u32 = prefix.parse().map_err(|_| {
        TunError::new(
            TunErrorCode::ApplyFailed,
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
            TunErrorCode::ApplyFailed,
            format!("invalid tun address: {cidr}"),
        )
    })?;
    let max = if ipv6 { 128 } else { 32 };
    if prefix == 0 || prefix > max {
        return Err(TunError::new(
            TunErrorCode::ApplyFailed,
            format!("tun address prefix must be in 1..={max}, got {prefix}"),
        ));
    }
    Ok(())
}

/// The macOS TUN backend (native sing-box ownership).
pub struct MacosTunBackend {
    owner_token: String,
    host: Box<dyn MacOsHost + Send>,
    coordinator: Box<dyn CoreCoordinator + Send>,
    config_path: PathBuf,
    journal_path: Option<PathBuf>,
}

/// Owned resources observed after full convergence: the required addresses
/// and routes the transition claims.
struct OwnedState {
    addresses: Vec<CidrRecord>,
    routes: Vec<RouteRecord>,
}

impl MacosTunBackend {
    pub fn new(
        owner_token: impl Into<String>,
        host: Box<dyn MacOsHost + Send>,
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

    /// Attach the journal file the recovery driver uses; the backend updates
    /// it after every granular mutation boundary, exactly like a real backend.
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
        journal.updated_at = chrono::Utc::now().to_rfc3339();
        mutate(&mut journal);
        journal.save(path)
    }

    /// An apply or journal failure can happen after the elevated core has
    /// already created the adapter. If stopping that core also fails, the
    /// ownership boundary is unknowable and the caller must remain fail-closed.
    fn rollback_after_apply_failure(&mut self, apply_err: TunError) -> TunError {
        match self.coordinator.stop() {
            Ok(()) => apply_err,
            Err(stop_err) => TunError::new(
                TunErrorCode::RecoveryRequired,
                format!(
                    "apply failed ({}) and elevated core cleanup was not verified ({})",
                    apply_err.message, stop_err.message
                ),
            ),
        }
    }

    /// Probe the lowest free `utun<N>` index at or above `from`. Read-only.
    fn probe_free_utun(&self, from: u32) -> Result<String, TunError> {
        let existing = self.host.list_interface_names()?;
        let used: std::collections::HashSet<u32> = existing
            .iter()
            .filter_map(|name| utun_index(name))
            .collect();
        (from..1000)
            .find(|index| !used.contains(index))
            .map(|index| format!("utun{index}"))
            .ok_or_else(|| {
                TunError::new(
                    TunErrorCode::ApplyFailed,
                    format!("no free utun index in {from}..1000"),
                )
            })
    }

    /// Resolve the interface name for a transition: the explicit name when it
    /// is free, otherwise the collision-fallback probe at a higher index (T0
    /// spike §7 open item 2: probe a higher index, else fail closed).
    fn resolve_interface_name(&self, requested: Option<&str>) -> Result<String, TunError> {
        match requested {
            Some(name) => {
                let Some(index) = utun_index(name) else {
                    return Err(TunError::new(
                        TunErrorCode::ApplyFailed,
                        format!("macOS requires a utun<N> interface name, got {name}"),
                    ));
                };
                if self
                    .host
                    .list_interface_names()?
                    .iter()
                    .any(|existing| existing == name)
                {
                    tracing::warn!(
                        name,
                        "utun interface name already in use; probing a free index"
                    );
                    self.probe_free_utun(index + 1)
                } else {
                    Ok(name.to_string())
                }
            }
            None => self.probe_free_utun(UTUN_PROBE_FLOOR),
        }
    }

    /// Owned resources observed after full convergence: the required
    /// addresses and routes the transition claims.
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
                |interface| format!("up={} addresses={:?}", interface.up, interface.addresses),
            );
            last_diagnostic = format!(
                "{interface_summary}; missing_addresses={missing_addresses:?}; missing_route={missing_route:?}"
            );
            std::thread::sleep(Duration::from_millis(APPLY_CONVERGE_DELAY_MS));
        }
        Err(TunError::new(
            TunErrorCode::HealthcheckFailed,
            format!(
                "interface {name} did not converge to the required addresses and routes within {} ms; last observation: {last_diagnostic}",
                APPLY_CONVERGE_TRIES * APPLY_CONVERGE_DELAY_MS as u32
            ),
        ))
    }

    /// Whether any probe inside `destination` resolves to `name`.
    ///
    /// A single probe can be shadowed by a more-specific local route:
    /// sing-box keeps the LAN subnet on the host route, so a LAN inside a
    /// broad auto-route range (e.g. `128.0.0.0/1`) resolves the base probe to
    /// the LAN interface instead of the utun. Spreading the probes accepts
    /// the route as owned when any probe still resolves to the adapter; a
    /// fully missing route fails every probe.
    fn route_resolves_to(&self, destination: &str, name: &str) -> Result<bool, TunError> {
        for probe in routes::route_probe_addresses(destination) {
            if self.host.route_interface(&probe)?.as_deref() == Some(name) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Whether any journaled owned route still resolves to `name`.
    fn owned_routes_remain(&self, applied: &AppliedTun, name: &str) -> Result<bool, TunError> {
        for route in applied.routes.iter().filter(|r| r.owned) {
            if self.route_resolves_to(&route.destination, name)? {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

impl TunBackend for MacosTunBackend {
    fn capability(&self) -> TunCapability {
        TunCapability {
            supported: true,
            reason: None,
            ipv4: true,
            ipv6: true,
            // The elevated coordinator can run `networksetup`; `dns_hijack`
            // routes DNS through the sing-box engine (see the module docs).
            dns_hijack: true,
        }
    }

    fn prepare(&self, config: &TunConfig) -> Result<PreparedTun, TunError> {
        if config.addresses.is_empty() {
            return Err(TunError::new(
                TunErrorCode::ApplyFailed,
                "tun config requires at least one address",
            ));
        }
        // Dual-stack lock (§24.5 point 4): an IPv4-only tun installs no IPv6
        // routes and silently leaks IPv6; IPv4 itself is mandatory.
        if !routes::has_v4(&config.addresses) {
            return Err(TunError::new(
                TunErrorCode::ApplyFailed,
                "tun config must include an IPv4 address (IPv4 is mandatory)",
            ));
        }
        if !routes::has_v6(&config.addresses) {
            return Err(TunError::new(
                TunErrorCode::ApplyFailed,
                "tun config must include an IPv6 address (dual-stack lock: an IPv4-only tun silently leaks IPv6)",
            ));
        }
        for cidr in &config.addresses {
            validate_cidr(cidr, cidr.contains(':'))?;
        }
        if !(1280..=9000).contains(&config.mtu) {
            return Err(TunError::new(
                TunErrorCode::ApplyFailed,
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
                TunErrorCode::ApplyFailed,
                "prepare must resolve the interface name before apply",
            ));
        };
        // The required address / route sets are the verification contract:
        // the journal records both what we own and what the config required,
        // and every later check compares against the required sets.
        let expected_addresses = config.addresses.clone();
        let expected_routes = routes::auto_route_destinations(config);

        // Mutation boundary: the elevated core starts and sing-box creates
        // the adapter, assigns addresses, and installs routes in one go.
        let core_pid = self.coordinator.start_with_config(&self.config_path)?;
        let interface_id = utun_index(name).map(|index| index.to_string());
        // Bounded wait for the adapter to appear (kernel hand-off can lag
        // the process start by a moment).
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
        if state.is_none() {
            // The core claims success but the adapter never appeared: stop
            // it (fail closed) so nothing half-owned survives.
            return Err(self.rollback_after_apply_failure(TunError::new(
                TunErrorCode::HealthcheckFailed,
                format!(
                    "core started but interface {name} is not present after {} ms",
                    INTERFACE_APPEAR_TRIES * INTERFACE_APPEAR_DELAY_MS as u32
                ),
            )));
        }

        // Journal INTERFACE_CREATED; a failed journal write rolls the mutation
        // back (stop the core — the kernel removes the adapter).
        if let Err(err) = self.journal_record(steps::INTERFACE_CREATED, |journal| {
            journal.interface_name = Some(name.to_string());
            journal.interface_id = interface_id.clone();
            journal.expected_addresses = expected_addresses.clone();
            journal.expected_routes = expected_routes.clone();
        }) {
            return Err(self.rollback_after_apply_failure(err));
        }

        // Dual-stack + full-route locks at apply time: the interface must
        // carry every required address and every required route must resolve
        // to it before anything is recorded as owned.
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

        // DNS interception (last mutation): point the primary service's DNS
        // at public resolvers so queries enter the TUN and the config's
        // `hijack-dns` rule answers from the DNS engine. Journal the
        // snapshot; a failed journal write restores the previous resolvers.
        let mut dns_before = None;
        let mut dns_after = None;
        if config.dns_hijack {
            let service = self.host.dns_service()?.ok_or_else(|| {
                TunError::new(
                    TunErrorCode::ApplyFailed,
                    "no network service found to point DNS at public resolvers",
                )
            })?;
            let before = self.host.dns_servers(&service)?;
            dns_before = Some(DnsSnapshot {
                platform_snapshot: dns_snapshot(&service, &before),
            });
            let target: Vec<String> = MACOS_TUN_DNS_SERVERS
                .iter()
                .map(|s| s.to_string())
                .collect();
            self.coordinator.set_dns(&service, &target)?;
            dns_after = Some(DnsSnapshot {
                platform_snapshot: dns_snapshot(&service, &target),
            });
            if let Err(err) = self.journal_record(steps::DNS_APPLIED, |journal| {
                journal.dns_before = dns_before.clone();
                journal.dns_after = dns_after.clone();
            }) {
                let _ = self.coordinator.set_dns(&service, &before);
                return Err(self.rollback_after_apply_failure(err));
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
        let name = applied.interface_name.as_deref();
        let Some(name) = name else {
            // No interface was ever claimed: nothing owned.
            return Ok(TunHealth {
                interface_up: false,
                addresses_present: false,
                routes_owned: false,
                dns_consistent: true,
                control_path_reachable: true,
                nothing_owned: true,
            });
        };
        let expected_id = applied.interface_id.as_deref();
        let state = self.host.interface_state(name)?;
        // Identity lock: exact name AND utun index must match the journal.
        let id_matches = expected_id
            .is_some_and(|id| utun_index(name).map(|i| i.to_string()).as_deref() == Some(id));
        let interface_up = state.as_ref().is_some_and(|state| state.up && id_matches);
        // Exact-address lock: every address the config *required* must still
        // be on the interface. Checking only the recorded owned subset would
        // accept a tun that lost an address family before it was recorded.
        let addresses_present = state.as_ref().is_some_and(|state| {
            applied
                .expected_addresses
                .iter()
                .all(|addr| state.addresses.contains(addr))
        });
        // Full-route lock: every required destination must still resolve to
        // us; a partially missing route set is never healthy.
        let mut routes_owned = true;
        for destination in &applied.expected_routes {
            if !self.route_resolves_to(destination, name)? {
                routes_owned = false;
                break;
            }
        }
        let control_path = self.host.route_interface("127.0.0.1")?;
        let control_path_reachable = control_path.as_deref().is_some_and(|iface| iface != name);
        // DNS consistency: the primary service must still carry the resolvers
        // the apply recorded (compare against the *after* snapshot; an
        // external DNS change is never silently overwritten by restore).
        let dns_consistent = match &applied.dns_after {
            Some(after) => {
                let (service, expected) = dns_snapshot_parts(&after.platform_snapshot);
                self.host
                    .dns_servers(&service)
                    .map(|current| current == expected)
                    .unwrap_or(true)
            }
            None => true,
        };
        let interface_gone = state.is_none();
        let owned_routes_remain = self.owned_routes_remain(applied, name)?;
        // DNS is still "owned" only while the platform carries the applied
        // `after` snapshot; after a restore (or an external change) it is not.
        let dns_owned = match &applied.dns_after {
            Some(after) => {
                let (service, expected) = dns_snapshot_parts(&after.platform_snapshot);
                self.host
                    .dns_servers(&service)
                    .map(|current| current == expected)
                    .unwrap_or(false)
            }
            None => false,
        };
        let nothing_owned = interface_gone && !owned_routes_remain && !dns_owned;
        Ok(TunHealth {
            interface_up,
            addresses_present,
            routes_owned,
            dns_consistent,
            control_path_reachable,
            nothing_owned,
        })
    }

    fn restore(&mut self, applied: &AppliedTun) -> Result<(), TunError> {
        // Release first: stopping the elevated core must never depend on
        // journal durability. A journal write that keeps failing (e.g. disk
        // full) must not abort restore before the core is stopped — that
        // would leave the OS captured with no way through the UI. The
        // controller already journals `restore_started` before calling us on
        // the disable path; on the recovery path the idempotent stop
        // converges even if this granular record cannot be persisted.
        self.coordinator.stop().map_err(|err| {
            TunError::new(
                err.code,
                format!("stop core during restore: {}", err.message),
            )
        })?;

        self.journal_record(steps::RESTORE_STARTED, |_| {})?;

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
            std::thread::sleep(std::time::Duration::from_millis(
                INTERFACE_TEARDOWN_DELAY_MS,
            ));
        }
        if !interface_gone {
            // The adapter survives the core stop: removal needs the
            // privileged helper. Fail closed — the driver persists
            // recovery_required and no new capture starts.
            return Err(TunError::new(
                TunErrorCode::RecoveryRequired,
                format!(
                    "interface {} still present after core stop; removal needs the privileged helper",
                    name.unwrap_or("<unknown>")
                ),
            ));
        }
        if let Some(name) = name {
            if self.owned_routes_remain(applied, name)? {
                return Err(TunError::new(
                    TunErrorCode::RecoveryRequired,
                    format!("owned routes still resolve to {name} after core stop"),
                ));
            }
        }

        // The kernel flushed the interface and its routes; journal the
        // observed removal boundaries.
        self.journal_record(steps::ROUTES_REMOVED, |journal| {
            journal.routes.clear();
            journal.expected_routes.clear();
        })?;
        self.journal_record(steps::INTERFACE_REMOVED, |journal| {
            journal.interface_name = None;
            journal.interface_id = None;
            journal.addresses.clear();
            journal.expected_addresses.clear();
        })?;

        // DNS restore (compare-before-restore): only when the platform still
        // carries the applied resolvers; an external DNS change is preserved
        // and surfaces as recovery_required instead of being overwritten.
        if let Some(after) = &applied.dns_after {
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
                self.coordinator.set_dns(&service, &before)?;
                self.journal_record(steps::DNS_RESTORED, |journal| {
                    journal.dns_before = None;
                    journal.dns_after = None;
                })?;
            } else {
                return Err(TunError::new(
                    TunErrorCode::RecoveryRequired,
                    "system DNS no longer matches the journal's dns_after snapshot; external change preserved",
                ));
            }
        }
        Ok(())
    }

    fn recover(&mut self, journal: &TunJournal) -> Result<RecoveryOutcome, TunError> {
        if journal.state == JournalState::Clean {
            return Ok(RecoveryOutcome::NothingToDo);
        }
        let applied = AppliedTun::from_journal(journal);
        // kill -9 residue: the kernel already removed the adapter and flushed
        // its routes; verification alone proves cleanup.
        let health = self.verify(&applied)?;
        if health.nothing_owned {
            return Ok(RecoveryOutcome::Cleaned);
        }
        // Otherwise run the idempotent release (stop any remaining core) and
        // re-verify. Never enables capture.
        match self.restore(&applied) {
            Ok(()) => {}
            Err(err) if err.code == TunErrorCode::RecoveryRequired => {
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
    fn ifconfig_l_splits_names() {
        assert_eq!(
            parse_ifconfig_l("lo0 gif0 stf0 en0 en1 utun0 utun1\n"),
            ["lo0", "gif0", "stf0", "en0", "en1", "utun0", "utun1"]
        );
        assert!(parse_ifconfig_l("").is_empty());
    }

    #[test]
    fn ifconfig_state_parses_up_flag_and_dual_stack_addresses() {
        let output = "utun420: flags=8051<UP,POINTOPOINT,RUNNING,MULTICAST> mtu 9000\n\
\tinet 10.0.0.1 --> 10.0.0.2 netmask 0xfffffffc\n\
\tinet6 fe80::1234:5678:9abc:def0%utun420 prefixlen 64 scopeid 0xf\n\
\tinet6 fdfe:dcba:9876::1 prefixlen 126\n";
        let state = parse_ifconfig_state(output, "utun420").expect("parse");
        assert!(state.up);
        assert_eq!(
            state.addresses,
            [
                "10.0.0.1/30",
                "fe80::1234:5678:9abc:def0/64",
                "fdfe:dcba:9876::1/126"
            ],
            "link-local scope suffix stripped, netmask converted to prefix"
        );
    }

    #[test]
    fn ifconfig_state_down_and_wrong_name() {
        let down = "utun9: flags=8002<POINTOPOINT,MULTICAST> mtu 9000\n";
        let state = parse_ifconfig_state(down, "utun9").expect("parse");
        assert!(!state.up);
        assert!(state.addresses.is_empty());
        assert!(
            parse_ifconfig_state(down, "utun10").is_none(),
            "output for another interface is not claimed"
        );
    }

    #[test]
    fn route_get_parses_interface() {
        let output = "   route to: 8.8.8.8\n\
     destination: default\n\
            mask: default\n\
         gateway: 192.168.5.1\n\
       interface: en0\n\
           flags: <UP,GATEWAY,DONE,STATIC,PRCLONING>\n";
        assert_eq!(parse_route_interface(output).as_deref(), Some("en0"));
        assert_eq!(parse_route_interface("no route here\n"), None);
    }

    #[test]
    fn utun_index_parses_numeric_suffix_only() {
        assert_eq!(utun_index("utun420"), Some(420));
        assert_eq!(utun_index("utun0"), Some(0));
        assert_eq!(utun_index("utun"), None);
        assert_eq!(utun_index("tun0"), None);
        assert_eq!(utun_index("utunx"), None);
    }

    #[test]
    fn hardware_port_parser_maps_device_to_port() {
        let output = "Hardware Port: Wi-Fi\n\
            Device: en0\n\
            Ethernet Address: aa:bb\n\
            \n\
            Hardware Port: USB 10/100/1000 LAN\n\
            Device: en7\n\
            Ethernet Address: cc:dd\n";
        assert_eq!(parse_hardware_port(output, "en0").as_deref(), Some("Wi-Fi"));
        assert_eq!(
            parse_hardware_port(output, "en7").as_deref(),
            Some("USB 10/100/1000 LAN")
        );
        assert_eq!(parse_hardware_port(output, "en9"), None);
        assert_eq!(parse_hardware_port("no ports", "en0"), None);
    }

    #[test]
    fn service_order_parser_maps_hardware_port_to_service() {
        let output = "An asterisk (*) denotes that a network service is disabled.\n\
            (1) Wi-Fi\n\
            (Hardware Port: Wi-Fi, Device: en0)\n\
            \n\
            (2) USB 10/100/1000 LAN\n\
            (Hardware Port: USB 10/100/1000 LAN, Device: en7)\n";
        assert_eq!(
            parse_service_order(output, "Wi-Fi").as_deref(),
            Some("Wi-Fi")
        );
        assert_eq!(
            parse_service_order(output, "USB 10/100/1000 LAN").as_deref(),
            Some("USB 10/100/1000 LAN")
        );
        assert_eq!(parse_service_order(output, "Thunderbolt"), None);
    }

    #[test]
    fn dns_servers_parser_reads_configured_list_and_empty_state() {
        let output = "DNS Servers:\n223.5.5.5\n119.29.29.29\n";
        assert_eq!(parse_dns_servers(output), ["223.5.5.5", "119.29.29.29"]);
        let empty = "There aren't any DNS Servers set on Wi-Fi.\n";
        assert!(parse_dns_servers(empty).is_empty());
        assert!(parse_dns_servers("garbage\nnot-an-ip\n").is_empty());
    }

    #[test]
    fn dns_snapshot_roundtrips_service_and_servers() {
        let servers: Vec<String> = ["223.5.5.5", "119.29.29.29"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let snap = dns_snapshot("Wi-Fi", &servers);
        assert_eq!(
            dns_snapshot_parts(&snap),
            ("Wi-Fi".to_string(), servers.clone())
        );
        let empty = dns_snapshot("Wi-Fi", &[]);
        assert_eq!(dns_snapshot_parts(&empty), ("Wi-Fi".to_string(), vec![]));
    }
}
