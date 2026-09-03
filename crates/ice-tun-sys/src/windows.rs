//! Windows TUN backend (plan §5 T2; T0 gate `windows_tun_ready` flipped
//! 2026-09-03, design note tun-windows-t0 §1.2).
//!
//! Ownership model (locked by the Windows T0 spike): the elevated sing-box
//! process — run by an injected [`CoreCoordinator`] — owns the WinTUN
//! adapter, its addresses, its routes, and its DNS. The wintun driver is
//! embedded in the pinned sing-box binary (sing-tun `internal/wintun` loads
//! it from memory), so no side-by-side DLL has to ship; adapter creation
//! requires an Administrator context, which is why the core runs elevated.
//!
//! Windows identity: the interface **index** from
//! `netsh interface ipv4 show interfaces` is the adapter identity token
//! (the analogue of the macOS utun index). Routes are matched through the
//! route table's own identity: IPv4 `route print -4` rows carry the owning
//! interface's IP in the `Interface` column, IPv6 `route print -6` rows
//! carry the interface index in the `If` column.
//!
//! Windows DNS (locked by the spike, §1.1/§4): sing-box sets the TUN
//! adapter's DNS servers to the TUN peers, so every system query enters the
//! engine (proven by `nslookup` resolving through the TUN peer). The backend
//! claims ownership the macOS way — journal `dns_before` / `dns_after`
//! snapshots and compare-before-restore — but observes only: the adapter's
//! DNS dies with the adapter, and the backend never mutates DNS itself (no
//! elevated context; `set_dns` is unsupported). A third-party DNS change
//! during the session is preserved, mirroring macOS.
//!
//! Expected routes: the observed, stable Windows auto-route set
//! (10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16, 100.64.0.0/10 +
//! fdfe:dcba:9876::/126) is probed after convergence and journaled as owned.
//!
//! The module is compiled on every platform so the backend logic stays
//! host-free testable on all CI hosts; `create_backend` gates activation per
//! platform.

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::backend::{
    AppliedTun, PreparedTun, RecoveryOutcome, TunBackend, TunCapability, TunConfig, TunHealth,
};
use crate::coordinator::CoreCoordinator;
use crate::error::{TunError, TunErrorCode};
use crate::journal::{steps, CidrRecord, DnsSnapshot, JournalState, RouteRecord, TunJournal};
use crate::routes;

/// Default WinTUN adapter name (sing-box default on Windows).
pub const DEFAULT_WINTUN_NAME: &str = "Wintun";
/// Maximum WinTUN adapter name length (`wintun.h` `WINTUN_MAX_ADAPTER_NAME`).
const ADAPTER_NAME_MAX: usize = 128;
/// Bounded wait for the adapter to appear after the elevated core starts.
const INTERFACE_APPEAR_TRIES: u32 = 15;
const INTERFACE_APPEAR_DELAY_MS: u64 = 200;
/// Bounded wait for the adapter to converge to the required addresses and
/// routes after it appears (dual-stack + route locks).
const APPLY_CONVERGE_TRIES: u32 = 15;
const APPLY_CONVERGE_DELAY_MS: u64 = 200;
/// Bounded wait for the adapter to disappear after the core stops.
const INTERFACE_TEARDOWN_TRIES: u32 = 10;
const INTERFACE_TEARDOWN_DELAY_MS: u64 = 200;

/// Host state of one interface as reported by the `netsh` probes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsInterfaceState {
    /// Media is connected (`State` column of `netsh interface show
    /// interface`).
    pub up: bool,
    /// Addresses as CIDRs. IPv4 entries carry the prefix from the
    /// `Subnet Prefix` line; IPv6 entries are bare addresses (netsh does not
    /// report an IPv6 prefix), compared by [`routes::address_key`].
    pub addresses: Vec<String>,
    /// Interface index (identity token) from `netsh interface ipv4 show
    /// interfaces`.
    pub index: Option<u32>,
}

/// How an interface's IPv4 DNS servers were configured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DnsSource {
    Dhcp,
    Static,
}

/// Per-interface IPv4 DNS state (`netsh interface ipv4 show dnsservers`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterfaceDns {
    pub name: String,
    pub source: DnsSource,
    pub servers: Vec<String>,
}

/// Host reads the Windows backend needs. Implementations must be read-only:
/// they never mutate the OS. `ProcessWindowsHost` shells out to `netsh` /
/// `route print`; tests inject a fake.
pub trait WindowsHost {
    /// All interface names (`netsh interface ipv4 show interfaces`).
    fn list_interface_names(&self) -> Result<Vec<String>, TunError>;
    /// Parsed state of one interface; `None` when it does not exist.
    fn interface_state(&self, name: &str) -> Result<Option<WindowsInterfaceState>, TunError>;
    /// The interface identity the route table resolves `destination`
    /// through. IPv4 returns the owning interface's IP; IPv6 returns the
    /// owning interface's index as a string. `None` when no route exists.
    fn route_interface(&self, destination: &str) -> Result<Option<String>, TunError>;
    /// Per-interface IPv4 DNS servers (`netsh interface ipv4 show
    /// dnsservers`). The TUN adapter appears here with the TUN peers once
    /// sing-box has claimed DNS.
    fn dns_v4_servers(&self) -> Result<Vec<InterfaceDns>, TunError>;
}

/// Host reads via subprocess (`netsh`, `route print`). Read-only.
#[derive(Debug, Default)]
pub struct ProcessWindowsHost;

fn run_command(program: &str, args: &[&str]) -> Result<CommandOutput, TunError> {
    let mut command = Command::new(program);
    command.args(args);
    #[cfg(target_os = "windows")]
    {
        // CREATE_NO_WINDOW: the netsh probes run from the GUI app; without it
        // every probe flashes a console window (an infinite storm while the
        // recovery watchdog retries).
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let output = command.output().map_err(|err| {
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

/// Whether a netsh failure output means the named interface does not exist.
/// Only this case lets `interface_state` report the interface as "gone";
/// every other non-zero exit is a probe failure and fails closed, so a
/// transient netsh error can never be misread as a verified missing
/// interface (which would let restore/recovery journal cleanup that never
/// happened). netsh error text is localized; the markers cover the English
/// output the parsers in this module already assume.
fn netsh_interface_missing(stderr: &str, stdout: &str) -> bool {
    let haystack = format!("{stderr}\n{stdout}").to_lowercase();
    [
        "does not exist",
        "not found",
        "cannot be found",
        "is not present",
        "not enabled on this system",
        "not a valid interface",
    ]
    .iter()
    .any(|marker| haystack.contains(marker))
}

/// Whether a failed probe should be read as "the interface is verified
/// gone". The English error markers are locale-dependent — a zh-CN netsh
/// reports `此名称的接口未与路由器一起注册` ("the interface with this name is
/// not registered with the router"), which matches no marker and would fail
/// closed forever. The authoritative, locale-proof cross-check is the
/// `netsh interface ipv4 show interfaces` listing: the interface absent from
/// the listing is verified gone regardless of the error text. `listing` is
/// `None` when the listing probe itself failed → fail closed (not verified
/// gone).
fn probe_means_interface_gone(
    stderr: &str,
    stdout: &str,
    listing: Option<Vec<String>>,
    name: &str,
) -> bool {
    if netsh_interface_missing(stderr, stdout) {
        return true;
    }
    listing.is_some_and(|names| !names.iter().any(|existing| existing == name))
}

/// A `HealthcheckFailed` error describing a failed `netsh` probe.
fn probe_error(program: &str, out: &CommandOutput) -> TunError {
    let detail = out.stderr.trim();
    let detail = if detail.is_empty() {
        out.stdout.trim()
    } else {
        detail
    };
    TunError::new(
        TunErrorCode::HealthcheckFailed,
        format!("{program} failed: {detail}"),
    )
}

impl WindowsHost for ProcessWindowsHost {
    fn list_interface_names(&self) -> Result<Vec<String>, TunError> {
        let out = run_command("netsh", &["interface", "ipv4", "show", "interfaces"])?;
        if out.status != Some(0) {
            return Err(TunError::new(
                TunErrorCode::HealthcheckFailed,
                format!(
                    "netsh interface ipv4 show interfaces failed: {}",
                    out.stderr.trim()
                ),
            ));
        }
        Ok(parse_netsh_interfaces(&out.stdout)
            .into_iter()
            .map(|(_, name)| name)
            .collect())
    }

    fn interface_state(&self, name: &str) -> Result<Option<WindowsInterfaceState>, TunError> {
        let up_out = run_command(
            "netsh",
            &["interface", "show", "interface", &format!("name={name}")],
        )?;
        if up_out.status != Some(0) {
            // Only a confirmed missing interface reports `Ok(None)`. Any
            // other failure is a probe error and must fail closed:
            // misreading a transient netsh error as "interface gone" would
            // let restore / recovery journal a verified cleanup that never
            // happened. The missing check is locale-proof: it cross-checks
            // the interface listing (zh-CN error text matches no English
            // marker).
            let listing = self.list_interface_names().ok();
            if probe_means_interface_gone(&up_out.stderr, &up_out.stdout, listing, name) {
                return Ok(None);
            }
            return Err(probe_error("netsh interface show interface", &up_out));
        }
        let up = parse_netsh_interface_show(&up_out.stdout)
            .iter()
            .find(|(existing, _)| existing == name)
            .map(|(_, up)| *up)
            .unwrap_or(false);
        if !up {
            tracing::warn!(
                interface = name,
                raw = %up_out.stdout,
                parsed = ?parse_netsh_interface_show(&up_out.stdout),
                "interface state: up probe reported the adapter down or not found"
            );
        }

        let v4_out = run_command(
            "netsh",
            &[
                "interface",
                "ipv4",
                "show",
                "addresses",
                &format!("name={name}"),
            ],
        )?;
        let mut addresses = if v4_out.status == Some(0) {
            parse_netsh_ipv4_addresses(&v4_out.stdout, name)
        } else if probe_means_interface_gone(
            &v4_out.stderr,
            &v4_out.stdout,
            self.list_interface_names().ok(),
            name,
        ) {
            // The interface vanished between probes; report it as gone.
            return Ok(None);
        } else {
            return Err(probe_error("netsh interface ipv4 show addresses", &v4_out));
        };

        // IPv6 probe: `netsh interface ipv6` accepts only `interface=` (not
        // `name=`; netsh probe syntax fact, design-note item 8).
        let v6_out = run_command(
            "netsh",
            &[
                "interface",
                "ipv6",
                "show",
                "addresses",
                &format!("interface={name}"),
            ],
        )?;
        if v6_out.status == Some(0) {
            addresses.extend(parse_netsh_ipv6_addresses(&v6_out.stdout, name));
        } else if probe_means_interface_gone(
            &v6_out.stderr,
            &v6_out.stdout,
            self.list_interface_names().ok(),
            name,
        ) {
            return Ok(None);
        } else {
            return Err(probe_error("netsh interface ipv6 show addresses", &v6_out));
        }

        // The adapter's interface index (identity lock for verify + the
        // IPv6 route probe): re-run the listing probe and parse the index
        // from the raw table — `list_interface_names` discards the indices.
        let listing = run_command("netsh", &["interface", "ipv4", "show", "interfaces"])?;
        let index = if listing.status == Some(0) {
            parse_netsh_interfaces(&listing.stdout)
                .into_iter()
                .find(|(_, existing)| existing == name)
                .map(|(index, _)| index)
        } else {
            None
        };
        Ok(Some(WindowsInterfaceState {
            up,
            addresses,
            index,
        }))
    }

    fn route_interface(&self, destination: &str) -> Result<Option<String>, TunError> {
        let probe = routes::route_probe_address(destination);
        if probe.contains(':') {
            let out = run_command("route", &["print", "-6"])?;
            if out.status != Some(0) {
                return Err(TunError::new(
                    TunErrorCode::HealthcheckFailed,
                    format!("route print -6 failed: {}", out.stderr.trim()),
                ));
            }
            Ok(parse_route_print_v6(&out.stdout, &probe).map(|index| index.to_string()))
        } else {
            let out = run_command("route", &["print", "-4"])?;
            if out.status != Some(0) {
                return Err(TunError::new(
                    TunErrorCode::HealthcheckFailed,
                    format!("route print -4 failed: {}", out.stderr.trim()),
                ));
            }
            Ok(parse_route_print_v4(&out.stdout, &probe))
        }
    }

    fn dns_v4_servers(&self) -> Result<Vec<InterfaceDns>, TunError> {
        let out = run_command("netsh", &["interface", "ipv4", "show", "dnsservers"])?;
        if out.status != Some(0) {
            return Err(TunError::new(
                TunErrorCode::HealthcheckFailed,
                format!(
                    "netsh interface ipv4 show dnsservers failed: {}",
                    out.stderr.trim()
                ),
            ));
        }
        Ok(parse_netsh_dnsservers(&out.stdout))
    }
}

/// `netsh interface ipv4 show dnsservers` → per-interface IPv4 DNS state.
///
/// Locale-proof: the interface boundary is any quoted name on the line
/// (zh-CN: `接口 "以太网" 的配置`, English: `Configuration for interface
/// "Ethernet"`), the servers are the IPv4 tokens in the block (zh-CN:
/// `通过 DHCP 配置的 DNS 服务器: 223.6.6.6`), and the source marker is
/// `DHCP` / `Static` / `静态` (both locales spell DHCP the same). Interfaces
/// without DNS servers appear with an empty server list.
pub fn parse_netsh_dnsservers(output: &str) -> Vec<InterfaceDns> {
    let mut result = Vec::new();
    let mut current: Option<InterfaceDns> = None;
    for line in output.lines() {
        let name = line.split('"').nth(1).filter(|name| !name.is_empty());
        if let Some(name) = name {
            if let Some(prev) = current.take() {
                result.push(prev);
            }
            current = Some(InterfaceDns {
                name: name.to_string(),
                source: DnsSource::Dhcp,
                servers: Vec::new(),
            });
            continue;
        }
        if let Some(entry) = current.as_mut() {
            let trimmed = line.trim();
            if trimmed.contains("DHCP") {
                entry.source = DnsSource::Dhcp;
            }
            if trimmed.contains("Static") || trimmed.contains("静态") {
                entry.source = DnsSource::Static;
            }
            for word in trimmed.split_whitespace() {
                if word.parse::<std::net::Ipv4Addr>().is_ok() {
                    entry.servers.push(word.to_string());
                }
            }
        }
    }
    if let Some(entry) = current.take() {
        result.push(entry);
    }
    result
}

/// `netsh interface ipv4 show interfaces` → `(index, name)` pairs.
pub fn parse_netsh_interfaces(output: &str) -> Vec<(u32, String)> {
    parse_netsh_interfaces_names(
        &output
            .lines()
            .skip(2)
            .map(str::to_string)
            .collect::<Vec<_>>(),
    )
}

fn parse_netsh_interfaces_names(lines: &[String]) -> Vec<(u32, String)> {
    let mut result = Vec::new();
    for line in lines {
        let mut tokens = line.split_whitespace();
        let Some(index) = tokens.next().and_then(|t| t.parse::<u32>().ok()) else {
            continue;
        };
        let _metric = tokens.next();
        let _mtu = tokens.next();
        let _state = tokens.next();
        let name = tokens.collect::<Vec<_>>().join(" ");
        if !name.is_empty() {
            result.push((index, name));
        }
    }
    result
}

/// `netsh interface show interface` → `(name, up)` pairs. Handles both the
/// table form (`Admin State | State | Type | Interface Name`) and the
/// single-interface block form (`<name>` on the first line, key/value lines
/// after). Locale-proof: the `State` values are matched on `Connected` /
/// `已连接` and the table-vs-block shapes are told apart by the separator
/// line only the table form carries.
pub fn parse_netsh_interface_show(output: &str) -> Vec<(String, bool)> {
    let lines: Vec<&str> = output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let is_separator = |line: &str| line.starts_with("---") || line.starts_with("===");
    let is_up = |word: &str| matches!(word, "Connected" | "connected" | "已连接");
    if lines.iter().any(|line| is_separator(line)) {
        // Table form: the first line is the column header, the rows follow.
        let mut result = Vec::new();
        for line in lines.iter().skip(1) {
            if is_separator(line) {
                continue;
            }
            let tokens: Vec<&str> = line.split_whitespace().collect();
            if tokens.len() < 4 {
                continue;
            }
            let name = tokens[3..].join(" ");
            if !name.is_empty() {
                result.push((name, is_up(tokens[1])));
            }
        }
        return result;
    }
    // Block form: the first non-empty line is the interface name; the
    // connect-state line decides `up`.
    let mut up = false;
    let mut name: Option<String> = None;
    for line in &lines {
        if name.is_none() {
            name = Some(line.to_string());
        }
        if line.to_lowercase().contains("connected") || line.contains("已连接") {
            up = true;
        }
    }
    name.map(|name| vec![(name, up)]).unwrap_or_default()
}

/// `netsh interface ipv4 show addresses name="<name>"` → CIDR addresses.
///
/// Locale-proof: the interface boundary is the quoted name (zh-CN:
/// `接口 "以太网" 的配置`, English: `Configuration for interface "Ethernet"`).
/// Both locales lay out the interface IP on the line right before the
/// subnet-prefix line (`IP 地址:` / `IP Address:` then `子网前缀: .../24 (掩码
/// ...)` / `Subnet Prefix: .../24 (mask ...)`), so the interface IP is the
/// bare IPv4 token of the line preceding the CIDR-token line.
pub fn parse_netsh_ipv4_addresses(output: &str, name: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut in_interface = false;
    let mut last_ip: Option<String> = None;
    for line in output.lines() {
        let line = line.trim();
        if line.contains(&format!("\"{name}\"")) {
            in_interface = true;
            last_ip = None;
            continue;
        }
        if !in_interface {
            continue;
        }
        let tokens: Vec<&str> = line.split_whitespace().collect();
        let cidr = tokens.iter().find_map(|token| {
            let (ip, prefix) = token.split_once('/')?;
            if ip.parse::<std::net::Ipv4Addr>().is_ok() && prefix.parse::<u32>().is_ok() {
                Some((ip.to_string(), prefix.to_string()))
            } else {
                None
            }
        });
        if let Some((_, prefix)) = cidr {
            if let Some(ip) = last_ip.take() {
                result.push(format!("{ip}/{prefix}"));
            }
            continue;
        }
        for token in tokens {
            if !token.contains('/') && token.parse::<std::net::Ipv4Addr>().is_ok() {
                last_ip = Some(token.to_string());
                break;
            }
        }
    }
    result
}

/// `netsh interface ipv6 show addresses interface="<name>"` → bare IPv6
/// addresses (netsh reports no prefix length).
///
/// Locale-proof: the probe is interface-scoped, so every IPv6 token in the
/// output is claimed (zh-CN: `地址 fe80::...%5 参数` — the address precedes
/// the `接口 Luid : 以太网` name line — English: `Address fdfe:dcba:9876::1`).
/// Link-local zone suffixes (`%5`) are stripped.
pub fn parse_netsh_ipv6_addresses(output: &str, _name: &str) -> Vec<String> {
    let mut result = Vec::new();
    for line in output.lines() {
        for token in line.split_whitespace() {
            let bare = token.split('%').next().unwrap_or(token);
            if bare.parse::<std::net::Ipv6Addr>().is_ok() {
                result.push(bare.to_string());
            }
        }
    }
    result
}

/// `route print -4` → for the probe address, the `Interface` column of the
/// most specific matching route (the owning interface's IP).
pub fn parse_route_print_v4(output: &str, probe: &str) -> Option<String> {
    let rows = route_print_v4_rows(output);
    let table: Vec<(String, u32)> = rows
        .iter()
        .map(|row| (row.network.clone(), row.netmask_bits))
        .collect();
    let index = routes::longest_prefix_route(&table, probe)?;
    Some(rows[index].interface_ip.clone())
}

/// `route print -6` → for the probe address, the `If` column (interface
/// index) of the most specific matching route.
pub fn parse_route_print_v6(output: &str, probe: &str) -> Option<u32> {
    let rows = route_print_v6_rows(output);
    let table: Vec<(String, u32)> = rows
        .iter()
        .map(|row| (row.destination.clone(), row.prefix_bits))
        .collect();
    let index = routes::longest_prefix_route(&table, probe)?;
    Some(rows[index].if_index)
}

/// One IPv4 route table row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutePrintV4Row {
    pub network: String,
    pub netmask_bits: u32,
    pub gateway: String,
    pub interface_ip: String,
}

/// Parse the `Active Routes:` section of `route print -4`.
///
/// Locale-proof: the section markers (`Active Routes:` / `活动路由:`) are
/// localized, so the table is located structurally — the `=====`-delimited
/// blocks; the active table is the first block containing valid rows. Row
/// validity is decided by the dotted netmask token (column 2), which is
/// locale-independent; the interface-list block and the `Persistent Routes:`
/// block (English `Persistent Routes:` / zh-CN `永久路由:`) never match.
pub fn route_print_v4_rows(output: &str) -> Vec<RoutePrintV4Row> {
    for block in route_print_blocks(output) {
        let mut rows = Vec::new();
        for line in block {
            let tokens: Vec<&str> = line.split_whitespace().collect();
            if tokens.len() < 5 {
                continue;
            }
            let Some(netmask_bits) = routes::dotted_netmask_to_prefix(tokens[1]) else {
                continue;
            };
            rows.push(RoutePrintV4Row {
                network: tokens[0].to_string(),
                netmask_bits,
                gateway: tokens[2].to_string(),
                interface_ip: tokens[3].to_string(),
            });
        }
        if !rows.is_empty() {
            return rows;
        }
    }
    Vec::new()
}

/// One IPv6 route table row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutePrintV6Row {
    pub if_index: u32,
    pub metric: u32,
    pub destination: String,
    pub prefix_bits: u32,
    pub gateway: String,
}

/// Parse the `Active Routes:` section of `route print -6`.
///
/// Locale-proof like [`route_print_v4_rows`]: the active table is the first
/// `=====`-delimited block containing rows whose first two columns are
/// numeric (interface index, metric). Wrapped rows (destination on one line,
/// gateway on the next — the zh-CN output wraps long destinations) are
/// accepted with an empty gateway: only the interface index matters.
pub fn route_print_v6_rows(output: &str) -> Vec<RoutePrintV6Row> {
    for block in route_print_blocks(output) {
        let mut rows = Vec::new();
        for line in block {
            let tokens: Vec<&str> = line.split_whitespace().collect();
            if tokens.len() < 3 {
                continue;
            }
            let Some(if_index) = tokens[0].parse::<u32>().ok() else {
                continue;
            };
            let Ok(metric) = tokens[1].parse::<u32>() else {
                continue;
            };
            let (destination, prefix_bits) = split_prefix(tokens[2]);
            rows.push(RoutePrintV6Row {
                if_index,
                metric,
                destination,
                prefix_bits,
                gateway: tokens.get(3).unwrap_or(&"").to_string(),
            });
        }
        if !rows.is_empty() {
            return rows;
        }
    }
    Vec::new()
}

/// Split `route print` output into its `=====`-delimited blocks.
fn route_print_blocks(output: &str) -> Vec<Vec<&str>> {
    let mut blocks = Vec::new();
    let mut current = Vec::new();
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("=====") {
            if !current.is_empty() {
                blocks.push(std::mem::take(&mut current));
            }
            continue;
        }
        if !trimmed.is_empty() {
            current.push(trimmed);
        }
    }
    if !current.is_empty() {
        blocks.push(current);
    }
    blocks
}

/// `dest/prefix` → `(dest, prefix_bits)`; `default` → `(::, 0)`.
fn split_prefix(token: &str) -> (String, u32) {
    if token == "default" || token == "::/0" {
        return ("::".to_string(), 0);
    }
    match token.split_once('/') {
        Some((dest, prefix)) => {
            let bits = prefix.parse::<u32>().unwrap_or(0);
            (dest.to_string(), bits)
        }
        None => (token.to_string(), 128),
    }
}

/// Whether `name` is a valid WinTUN adapter name (at most 128 chars, no
/// control characters, no Windows path separators).
fn valid_adapter_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= ADAPTER_NAME_MAX
        && !name.chars().any(|c| {
            c.is_control() || matches!(c, '<' | '>' | '"' | '/' | '\\' | '|' | ':' | '*' | '?')
        })
}

/// The Windows TUN backend (native sing-box ownership, planned T0 lock).
pub struct WindowsTunBackend {
    owner_token: String,
    host: Box<dyn WindowsHost + Send>,
    coordinator: Box<dyn CoreCoordinator + Send>,
    config_path: PathBuf,
    journal_path: Option<PathBuf>,
}

/// Owned resources observed after full convergence.
struct OwnedState {
    addresses: Vec<CidrRecord>,
    routes: Vec<RouteRecord>,
}

impl WindowsTunBackend {
    pub fn new(
        owner_token: impl Into<String>,
        host: Box<dyn WindowsHost + Send>,
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

    /// Serialized per-interface IPv4 DNS snapshot (JSON, human-readable and
    /// parseable for the compare-before-restore in [`restore`](TunBackend::restore)).
    fn dns_snapshot_string(&self) -> Result<String, TunError> {
        let entries = self.host.dns_v4_servers()?;
        serde_json::to_string(&entries).map_err(|err| {
            TunError::new(
                TunErrorCode::HealthcheckFailed,
                format!("serialize DNS snapshot: {err}"),
            )
        })
    }

    /// Resolve the adapter name: the requested name when free, else a
    /// deterministic collision probe (`Wintun 2`, `Wintun 3`, ...), else fail
    /// closed. Mirrors the macOS utun fallback probe.
    fn resolve_interface_name(&self, requested: Option<&str>) -> Result<String, TunError> {
        let base = match requested {
            Some(name) => {
                if !valid_adapter_name(name) {
                    return Err(TunError::new(
                        TunErrorCode::ApplyFailed,
                        format!("invalid Windows adapter name: {name}"),
                    ));
                }
                name.to_string()
            }
            None => DEFAULT_WINTUN_NAME.to_string(),
        };
        let existing = self.host.list_interface_names()?;
        if !existing.contains(&base) {
            return Ok(base);
        }
        for suffix in 2..100u32 {
            let candidate = format!("{base} {suffix}");
            if !existing.contains(&candidate) {
                tracing::warn!(
                    base,
                    candidate,
                    "adapter name already in use; probing a numbered variant"
                );
                return Ok(candidate);
            }
        }
        Err(TunError::new(
            TunErrorCode::ApplyFailed,
            format!("no free adapter name derived from {base}"),
        ))
    }

    /// Whether the route table resolves `destination` to this adapter:
    /// IPv4 routes identify their interface by IP, IPv6 by interface index.
    fn route_owned_by_adapter(
        &self,
        destination: &str,
        applied: &AppliedTun,
    ) -> Result<bool, TunError> {
        let Some(identity) = self.host.route_interface(destination)? else {
            return Ok(false);
        };
        if destination.contains(':') {
            return Ok(applied.interface_id.as_deref() == Some(identity.as_str()));
        }
        let expected_v4: Vec<&str> = applied
            .expected_addresses
            .iter()
            .filter(|address| !address.contains(':'))
            .map(|address| routes::address_key(address))
            .collect();
        Ok(expected_v4.contains(&identity.as_str()))
    }

    /// Owned resources observed after full convergence: the required
    /// addresses and the routes the route table resolves to the adapter.
    fn observe_owned_state(
        &self,
        config: &TunConfig,
        name: &str,
        interface_id: &str,
    ) -> Result<OwnedState, TunError> {
        let expected_addresses = &config.addresses;
        let mut last_diagnostic = String::from("interface not observed");
        for _ in 0..APPLY_CONVERGE_TRIES {
            let state = self.host.interface_state(name)?;
            let missing_addresses = state.as_ref().map_or_else(
                || expected_addresses.to_vec(),
                |interface| {
                    expected_addresses
                        .iter()
                        .filter(|address| {
                            !interface.addresses.iter().any(|actual| {
                                routes::address_key(actual) == routes::address_key(address)
                            })
                        })
                        .cloned()
                        .collect::<Vec<_>>()
                },
            );
            let addresses_ok = state.is_some() && missing_addresses.is_empty();
            let owned_routes = if addresses_ok {
                let expected_v4: Vec<String> = expected_addresses
                    .iter()
                    .filter(|address| !address.contains(':'))
                    .map(|address| routes::address_key(address).to_string())
                    .collect();
                self.observe_owned_routes(name, &expected_v4, interface_id)?
            } else {
                Vec::new()
            };
            if addresses_ok && !owned_routes.is_empty() {
                let owned_addresses = expected_addresses
                    .iter()
                    .map(|cidr| CidrRecord {
                        cidr: cidr.clone(),
                        owned: true,
                    })
                    .collect();
                return Ok(OwnedState {
                    addresses: owned_addresses,
                    routes: owned_routes,
                });
            }
            let interface_summary = state.as_ref().map_or_else(
                || "<missing>".to_string(),
                |interface| format!("up={} addresses={:?}", interface.up, interface.addresses),
            );
            last_diagnostic = format!(
                "{interface_summary}; missing_addresses={missing_addresses:?}; owned_routes={owned_routes:?}"
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

    /// Observe the routes the route table actually resolves to the adapter
    /// after convergence. Windows auto-route does not use the macOS sub-range
    /// trick, so the owned set is observed rather than derived from a locked
    /// constant; the T0 spike reconciles this with a deterministic set if the
    /// pinned sing-box shape is stable. Probes a conservative sample of the
    /// RFC1918 / CGNAT / ULA space; routes that resolve to this adapter's
    /// addresses (v4) or index (v6) are owned.
    fn observe_owned_routes(
        &self,
        _name: &str,
        expected_v4: &[String],
        interface_id: &str,
    ) -> Result<Vec<RouteRecord>, TunError> {
        let mut owned = Vec::new();
        for destination in [
            "10.0.0.0/8",
            "172.16.0.0/12",
            "192.168.0.0/16",
            "100.64.0.0/10",
            "fdfe:dcba:9876::/126",
        ] {
            let probe = routes::route_probe_address(destination);
            let Some(identity) = self.host.route_interface(&probe)? else {
                continue;
            };
            let is_ours = if probe.contains(':') {
                identity == interface_id
            } else {
                expected_v4.iter().any(|address| address == &identity)
            };
            if is_ours {
                owned.push(RouteRecord {
                    destination: destination.to_string(),
                    gateway: None,
                    metric: 0,
                    owned: true,
                });
            }
        }
        Ok(owned)
    }

    /// Whether any journaled owned route still resolves to the adapter.
    fn owned_routes_remain(&self, applied: &AppliedTun) -> Result<bool, TunError> {
        for route in applied.routes.iter().filter(|r| r.owned) {
            if self.route_owned_by_adapter(&route.destination, applied)? {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

impl TunBackend for WindowsTunBackend {
    fn capability(&self) -> TunCapability {
        TunCapability {
            supported: true,
            reason: None,
            ipv4: true,
            ipv6: true,
            // Locked by the host spike (design note §1.1): sing-box sets the
            // TUN adapter's DNS to the TUN peers, so the backend journals
            // dns_before / dns_after and verifies the adapter still owns DNS.
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
        // Dual-stack lock (§24.5 point 4): an IPv4-only tun silently leaks
        // IPv6; IPv4 itself is mandatory.
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
        let expected_addresses = config.addresses.clone();

        // DNS ownership (locked by the spike): snapshot the per-interface
        // IPv4 DNS before the core starts; after convergence the TUN adapter
        // carries the TUN peers. The journal stores both snapshots so restore
        // can compare-before-restore and verify can prove the adapter still
        // owns DNS. Read-only; the backend never mutates DNS itself.
        let dns_before = Some(DnsSnapshot {
            platform_snapshot: self.dns_snapshot_string()?,
        });

        // Mutation boundary: the elevated core starts and sing-box creates
        // the adapter, assigns addresses, and installs routes in one go.
        let core_pid = self.coordinator.start_with_config(&self.config_path)?;
        // Bounded wait for the adapter to appear (the wintun adapter shows up
        // in netsh after the driver session starts).
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
        let Some(interface) = state else {
            return Err(self.rollback_after_apply_failure(TunError::new(
                TunErrorCode::HealthcheckFailed,
                format!(
                    "core started but interface {name} is not present after {} ms",
                    INTERFACE_APPEAR_TRIES * INTERFACE_APPEAR_DELAY_MS as u32
                ),
            )));
        };
        let interface_id = interface.index.map(|index| index.to_string());

        // Journal INTERFACE_CREATED; a failed journal write rolls the
        // mutation back (stop the core).
        if let Err(err) = self.journal_record(steps::INTERFACE_CREATED, |journal| {
            journal.interface_name = Some(name.to_string());
            journal.interface_id = interface_id.clone();
            journal.expected_addresses = expected_addresses.clone();
        }) {
            return Err(self.rollback_after_apply_failure(err));
        }

        // Dual-stack + route locks at apply time.
        let observed =
            match self.observe_owned_state(config, name, interface_id.as_deref().unwrap_or("")) {
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
            journal.expected_routes = observed
                .routes
                .iter()
                .map(|r| r.destination.clone())
                .collect();
        }) {
            return Err(self.rollback_after_apply_failure(err));
        }

        // The adapter now owns DNS (sing-box set the peers); snapshot it and
        // journal both snapshots. A probe failure rolls the apply back.
        let dns_after = match self.dns_snapshot_string() {
            Ok(snapshot) => Some(DnsSnapshot {
                platform_snapshot: snapshot,
            }),
            Err(err) => return Err(self.rollback_after_apply_failure(err)),
        };
        if let Err(err) = self.journal_record(steps::DNS_APPLIED, |journal| {
            journal.dns_before = dns_before.clone();
            journal.dns_after = dns_after.clone();
        }) {
            return Err(self.rollback_after_apply_failure(err));
        }

        let owned_routes = observed.routes.clone();
        Ok(AppliedTun {
            interface_name: Some(name.to_string()),
            interface_id,
            addresses: observed.addresses,
            routes: owned_routes.clone(),
            expected_addresses,
            expected_routes: owned_routes.iter().map(|r| r.destination.clone()).collect(),
            dns_before,
            dns_after,
            core_pid: Some(core_pid),
        })
    }

    fn verify(&self, applied: &AppliedTun) -> Result<TunHealth, TunError> {
        let name = applied.interface_name.as_deref();
        let Some(name) = name else {
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
        // Identity lock: exact name AND interface index must match the journal.
        let id_matches = expected_id.is_some_and(|id| {
            state
                .as_ref()
                .and_then(|state| state.index.map(|index| index.to_string()))
                .as_deref()
                == Some(id)
        });
        let interface_up = state.as_ref().is_some_and(|state| state.up && id_matches);
        if !interface_up {
            tracing::warn!(
                name,
                expected_id = ?expected_id,
                observed = ?state,
                "tun verify: interface up/id lock failed"
            );
        }
        // Exact-address lock: every address the config *required* must still
        // be on the interface (compared by bare address; netsh IPv6 entries
        // carry no prefix).
        let addresses_present = state.as_ref().is_some_and(|state| {
            applied.expected_addresses.iter().all(|address| {
                state
                    .addresses
                    .iter()
                    .any(|actual| routes::address_key(actual) == routes::address_key(address))
            })
        });
        // Full-route lock: every required destination must still resolve to
        // the adapter.
        let mut routes_owned = true;
        for destination in &applied.expected_routes {
            if !self.route_owned_by_adapter(destination, applied)? {
                routes_owned = false;
                break;
            }
        }
        // Control path: loopback must NOT resolve to this adapter.
        let control_path = self.host.route_interface("127.0.0.1")?;
        let control_path_reachable = control_path.as_deref().is_some_and(|identity| {
            applied
                .expected_addresses
                .iter()
                .filter(|address| !address.contains(':'))
                .all(|address| routes::address_key(address) != identity)
        });
        let interface_gone = state.is_none();
        let owned_routes_remain = self.owned_routes_remain(applied)?;
        let nothing_owned = interface_gone && !owned_routes_remain;
        // DNS lock: nothing changed since apply AND the TUN adapter still
        // carries DNS servers (the peers). A probe failure fails closed.
        let dns_consistent = match &applied.dns_after {
            Some(after) => {
                let current = self.host.dns_v4_servers()?;
                let tun_has_dns = current
                    .iter()
                    .any(|entry| entry.name == name && !entry.servers.is_empty());
                let snapshot = serde_json::to_string(&current).unwrap_or_default();
                if snapshot != after.platform_snapshot {
                    tracing::debug!(
                        interface = name,
                        current = %snapshot,
                        after = %after.platform_snapshot,
                        "tun verify: dns snapshot drifted since apply"
                    );
                }
                snapshot == after.platform_snapshot && tun_has_dns
            }
            None => true,
        };
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
        self.journal_record(steps::RESTORE_STARTED, |_| {})?;

        // Release: stop the core; the native path's sing-box removes its
        // routes and the adapter. Windows termination is a hard kill
        // (accepted model, windows-plan §2) — whether the wintun adapter is
        // removed on process death is a T0 spike item; if it survives, this
        // backend fails closed into recovery_required (removal needs the
        // privileged helper path).
        self.coordinator.stop().map_err(|err| {
            TunError::new(
                err.code,
                format!("stop core during restore: {}", err.message),
            )
        })?;

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
            return Err(TunError::new(
                TunErrorCode::RecoveryRequired,
                format!(
                    "interface {} still present after core stop; removal needs the privileged helper path (T0 spike item)",
                    name.unwrap_or("<unknown>")
                ),
            ));
        }
        if self.owned_routes_remain(applied)? {
            return Err(TunError::new(
                TunErrorCode::RecoveryRequired,
                format!("owned routes still resolve to {name:?} after core stop"),
            ));
        }

        // DNS compare-before-restore: the TUN adapter (and its DNS) died with
        // the core above, so the remaining interfaces must match the
        // pre-start snapshot. A third-party DNS change during the session is
        // preserved — the backend never mutates DNS itself (no elevated
        // context on the app side; `set_dns` is unsupported).
        if let Some(before) = &applied.dns_before {
            let current = self.dns_snapshot_string()?;
            if current != before.platform_snapshot {
                tracing::warn!(
                    "system DNS no longer matches the pre-start snapshot; external change preserved (Windows backend never mutates DNS)"
                );
            }
        }
        self.journal_record(steps::DNS_RESTORED, |journal| {
            journal.dns_before = None;
            journal.dns_after = None;
        })?;

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
        Ok(())
    }

    fn recover(&mut self, journal: &TunJournal) -> Result<RecoveryOutcome, TunError> {
        if journal.state == JournalState::Clean {
            return Ok(RecoveryOutcome::NothingToDo);
        }
        let applied = AppliedTun::from_journal(journal);
        // Kill residue: the adapter may already be gone (driver cleanup on
        // process death); verification alone proves cleanup.
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
#[cfg(test)]
mod parsing_tests {
    use super::*;

    #[test]
    fn netsh_interfaces_parses_index_and_name() {
        let output = "\
Idx     Met         MTU          State          Name
---------------------------------------------------------------------------
  1          75        4294967295  connected     Loopback Pseudo-Interface 1
  5          25          1500  connected     Ethernet
 17          25          9000  connected     Wintun
";
        let parsed = parse_netsh_interfaces(output);
        assert_eq!(
            parsed,
            [
                (1, "Loopback Pseudo-Interface 1".to_string()),
                (5, "Ethernet".to_string()),
                (17, "Wintun".to_string()),
            ]
        );
    }

    #[test]
    fn interface_index_requires_the_raw_table_not_the_name_list() {
        // Regression: `interface_state` used to resolve the adapter index by
        // re-parsing the output of `list_interface_names` (bare names) as if
        // it were the raw listing table. No name line starts with a numeric
        // index, so the parse always yielded nothing and `interface_up`
        // failed its identity lock on every Windows host (up + addresses +
        // routes + DNS all verified; only the index was missing).
        let names_only = vec![
            "Loopback Pseudo-Interface 1".to_string(),
            "Wintun".to_string(),
        ];
        assert!(
            parse_netsh_interfaces_names(&names_only).is_empty(),
            "a bare name list must never parse as a listing table"
        );
        let table = "\
Idx     Met         MTU          State          Name
---------------------------------------------------------------------------
 17          25          9000  connected     Wintun
";
        assert_eq!(parse_netsh_interfaces(table), [(17, "Wintun".to_string())]);
    }

    #[test]
    fn netsh_interface_show_parses_connected_state() {
        let output = "\
Admin State    State          Type           Interface Name
-------------------------------------------------------------------------
Enabled        Connected      Dedicated      Wintun
Enabled        Disconnected   Dedicated      Ethernet
";
        let parsed = parse_netsh_interface_show(output);
        assert_eq!(
            parsed,
            [
                ("Wintun".to_string(), true),
                ("Ethernet".to_string(), false)
            ]
        );
    }

    #[test]
    fn netsh_ipv4_addresses_parses_ip_and_subnet_prefix() {
        let output = "\
Configuration for interface \"Wintun\"
    DHCP enabled:                         No
    IP Address:                           10.0.0.1
    Subnet Prefix:                        10.0.0.0/30 (mask 255.255.255.252)
    Default Gateway:                      .
";
        assert_eq!(
            parse_netsh_ipv4_addresses(output, "Wintun"),
            ["10.0.0.1/30"]
        );
        assert!(
            parse_netsh_ipv4_addresses(output, "Other").is_empty(),
            "addresses of another interface are not claimed"
        );
    }

    #[test]
    fn netsh_ipv6_addresses_parses_bare_addresses() {
        let output = "\
Interface 17: Wintun
---------------------------------------------------------------
Address fdfe:dcba:9876::1
Parameters for interface 17:
-------------------------------------------------------------
Interface Luid     : 16883443044148183040
Address Type       : Manual
Valid Lifetime     : infinite
Preferred Lifetime : infinite
Dup Address Detection : 5
Prefix Origin      : Well Known
Suffix Origin      : Well Known
Address State      : Preferred
Scope              : Global
";
        assert_eq!(
            parse_netsh_ipv6_addresses(output, "Wintun"),
            ["fdfe:dcba:9876::1"]
        );
        // The probe is interface-scoped (`interface=<name>`), so the parse is
        // name-independent; the interface name never appears before the
        // addresses in the zh-CN output either.
        assert_eq!(
            parse_netsh_ipv6_addresses(output, "Other"),
            ["fdfe:dcba:9876::1"]
        );
    }

    #[test]
    fn zh_cn_netsh_outputs_parse_locale_proof() {
        // Captured live on the zh-CN host (2026-09-03).
        let show = "\
以太网
    种类:     专用
    管理状态: 已启用
    连接状态: 已连接
";
        assert_eq!(
            parse_netsh_interface_show(show),
            [("以太网".to_string(), true)]
        );
        let show_down = "\
以太网
    管理状态: 已启用
    连接状态: 已断开
";
        assert_eq!(
            parse_netsh_interface_show(show_down),
            [("以太网".to_string(), false)]
        );

        let v4 = "\
接口 \"以太网\" 的配置
    DHCP 已启用:                          是
    IP 地址:                           10.28.10.67
    子网前缀:                        10.28.10.0/24 (掩码 255.255.255.0)
    默认网关:                         10.28.10.1
";
        assert_eq!(parse_netsh_ipv4_addresses(v4, "以太网"), ["10.28.10.67/24"]);

        let dns = "\
接口 \"以太网\" 的配置
    通过 DHCP 配置的 DNS 服务器:      223.6.6.6
                                          61.130.254.34
    用哪个前缀注册:                   只是主要

接口 \"Loopback Pseudo-Interface 1\" 的配置
    静态配置的 DNS 服务器:            无
    用哪个前缀注册:                   只是主要
";
        let parsed = parse_netsh_dnsservers(dns);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].name, "以太网");
        assert_eq!(parsed[0].source, DnsSource::Dhcp);
        assert_eq!(parsed[0].servers, ["223.6.6.6", "61.130.254.34"]);
        assert_eq!(parsed[1].name, "Loopback Pseudo-Interface 1");
        assert_eq!(parsed[1].source, DnsSource::Static);
        assert!(parsed[1].servers.is_empty());

        let v6 = "\
地址 fe80::bb78:1915:9425:d77%5 参数
---------------------------------------------------------
接口 Luid          : 以太网
作用域 ID          : 0.5
有效生存时间       : infinite
";
        assert_eq!(
            parse_netsh_ipv6_addresses(v6, "以太网"),
            ["fe80::bb78:1915:9425:d77"]
        );

        let routes = "\
===========================================================================
接口列表
  5...08 bf b8 00 e2 f3 ......Realtek PCIe GbE Family Controller
===========================================================================

IPv4 路由表
===========================================================================
活动路由:
网络目标        网络掩码          网关      接口   跃点数
          0.0.0.0          0.0.0.0       10.28.10.1      10.28.10.67     35
       10.28.10.0    255.255.255.0            在链路上       10.28.10.67    291
===========================================================================
永久路由:
  无
";
        assert_eq!(
            parse_route_print_v4(routes, "10.28.10.67").as_deref(),
            Some("10.28.10.67")
        );
        assert_eq!(
            parse_route_print_v4(routes, "8.8.8.8").as_deref(),
            Some("10.28.10.67"),
            "default route wins for non-local destinations"
        );

        let routes6 = "\
===========================================================================
接口列表
  5...08 bf b8 00 e2 f3 ......Realtek PCIe GbE Family Controller
===========================================================================

IPv6 路由表
===========================================================================
活动路由:
 接口跃点数    网络目标                 网关
  1    331 ::1/128                  在链路上
  5    291 fe80::/64                在链路上
===========================================================================
永久路由:
  无
";
        assert_eq!(parse_route_print_v6(routes6, "::1"), Some(1));
        assert_eq!(parse_route_print_v6(routes6, "fe80::1"), Some(5));
    }

    #[test]
    fn route_print_v4_picks_most_specific_interface_ip() {
        let output = "\
===========================================================================
Interface List
 17...00 ff ...... Wintun
===========================================================================

IPv4 Route Table
===========================================================================
Active Routes:
Network Destination        Netmask          Gateway       Interface  Metric
          0.0.0.0          0.0.0.0      192.168.5.1      192.168.5.99     25
          10.0.0.0    255.255.255.252         On-link          10.0.0.1    281
          10.0.0.1  255.255.255.255         On-link          10.0.0.1    281
          127.0.0.0        255.0.0.0         On-link         127.0.0.1    331
===========================================================================
Persistent Routes:
  None
";
        assert_eq!(
            parse_route_print_v4(output, "10.0.0.3").as_deref(),
            Some("10.0.0.1")
        );
        assert_eq!(
            parse_route_print_v4(output, "8.8.8.8").as_deref(),
            Some("192.168.5.99")
        );
        assert_eq!(
            parse_route_print_v4(output, "127.0.0.1").as_deref(),
            Some("127.0.0.1")
        );
    }

    #[test]
    fn route_print_v6_picks_most_specific_if_index() {
        let output = "\
IPv6 Route Table
===========================================================================
Active Routes:
 If Metric Network Destination      Gateway
 17    281 ::/0                     On-link
 17    281 fdfe:dcba:9876::/126     On-link
 17    281 fdfe:dcba:9876::1/128    On-link
  1    331 ::1/128                  On-link
===========================================================================
Persistent Routes:
  None
";
        assert_eq!(parse_route_print_v6(output, "fdfe:dcba:9876::1"), Some(17));
        assert_eq!(parse_route_print_v6(output, "fdfe:dcba:9876::3"), Some(17));
        assert_eq!(parse_route_print_v6(output, "::1"), Some(1));
        assert_eq!(
            parse_route_print_v6(output, "2001:db8::1"),
            Some(17),
            "default via 17"
        );
    }

    #[test]
    fn split_prefix_handles_default_and_host_routes() {
        assert_eq!(split_prefix("default"), ("::".to_string(), 0));
        assert_eq!(split_prefix("::/0"), ("::".to_string(), 0));
        assert_eq!(
            split_prefix("fdfe:dcba:9876::/126"),
            ("fdfe:dcba:9876::".to_string(), 126)
        );
        assert_eq!(split_prefix("::1/128"), ("::1".to_string(), 128));
        assert_eq!(
            split_prefix("2001:db8::1"),
            ("2001:db8::1".to_string(), 128)
        );
    }

    #[test]
    fn netsh_interface_missing_classifies_gone_vs_probe_error() {
        // Missing-interface netsh stderr (English, the module's assumption).
        assert!(netsh_interface_missing(
            "An interface with this name is not enabled on this system.",
            ""
        ));
        assert!(netsh_interface_missing(
            "The interface \"Wintun\" does not exist.",
            ""
        ));
        assert!(netsh_interface_missing(
            "",
            "The interface cannot be found."
        ));
        // Any other failure is a probe error, never a verified "gone".
        assert!(!netsh_interface_missing("", ""));
        assert!(!netsh_interface_missing(
            "Access is denied.",
            "The RPC server is unavailable."
        ));
    }

    #[test]
    fn probe_means_interface_gone_is_locale_proof_via_the_listing() {
        // zh-CN netsh: the localized error matches no English marker — the
        // interface-listing cross-check is what proves the interface is gone.
        let zh_cn = "此名称的接口未与路由器一起注册";
        assert!(!netsh_interface_missing(zh_cn, ""));
        assert!(probe_means_interface_gone(
            zh_cn,
            "",
            Some(vec!["以太网".into(), "WLAN".into()]),
            "Wintun"
        ));
        // The interface present in the listing → the probe failure is a real
        // probe error, not a verified "gone" (fail closed).
        assert!(!probe_means_interface_gone(
            zh_cn,
            "",
            Some(vec!["Wintun".into()]),
            "Wintun"
        ));
        // Listing probe failed → fail closed (not verified gone).
        assert!(!probe_means_interface_gone(zh_cn, "", None, "Wintun"));
        // English markers still fast-path without the listing.
        assert!(probe_means_interface_gone(
            "The interface does not exist.",
            "",
            None,
            "Wintun"
        ));
    }

    #[test]
    fn adapter_name_validation() {
        assert!(valid_adapter_name("Wintun"));
        assert!(valid_adapter_name("My VPN Tunnel 2"));
        assert!(!valid_adapter_name(""));
        assert!(!valid_adapter_name(&"x".repeat(129)));
        assert!(!valid_adapter_name("bad/name"));
        assert!(!valid_adapter_name("bad:name"));
        assert!(!valid_adapter_name("bad*name"));
        assert!(!valid_adapter_name("bad\nname"));
    }
}
