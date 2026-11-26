// SPDX-License-Identifier: GPL-3.0-or-later

//! Linux TUN backend tests (host-free on every CI platform).
//!
//! Covers prepare validation + `icebox<N>` collision fallback, journaled apply
//! with `ifindex` identity lock and `route.default_interface` pinning, both
//! `systemd-resolved` and static `/etc/resolv.conf` DNS modes (modeled as raw
//! bytes with marker + backup/restore), rollback on failure, `kill -9`
//! recovery, external DNS protection, DNS drift self-healing, `ppp*` uplinks,
//! and IPv6-disabled diagnostics.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use ice_tun_sys::backend::RecoveryOutcome;
use ice_tun_sys::coordinator::CoreCoordinator;
use ice_tun_sys::journal::{steps, JournalState, TunJournal};
use ice_tun_sys::linux::{
    classify_dns_mode, parse_resolv_conf_servers, LinuxDnsMode, LinuxHost, LinuxInterfaceState,
    LinuxTunBackend, LINUX_RESOLV_CONF_SERVICE,
};
#[cfg(target_os = "linux")]
use ice_tun_sys::ProcessLinuxHost;
use ice_tun_sys::{create_backend, routes, TunBackend, TunConfig, TunStack};
use ice_types::{ErrorCode, TunError};

const OWNER: &str = "ice-box:test-linux-1";
const MANAGED_MARKER: &str = "# Managed by ice-box TUN";

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "ice-tun-linux-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_tun_config(dir: &Path, addresses: &[&str]) -> PathBuf {
    let path = dir.join("config.json");
    fs::write(
        &path,
        serde_json::json!({
            "inbounds": [
                { "type": "mixed", "tag": "mixed-in" },
                {
                    "type": "tun",
                    "tag": "tun-in",
                    "address": addresses,
                    "auto_route": true,
                    "strict_route": true
                }
            ],
            "route": { "final": "direct" }
        })
        .to_string(),
    )
    .unwrap();
    path
}

fn linux_config(dns_hijack: bool) -> TunConfig {
    TunConfig {
        interface_name: Some("icebox0".into()),
        addresses: vec!["10.0.0.1/30".into(), "fdfe:dcba:9876::1/126".into()],
        mtu: 9000,
        stack: TunStack::Gvisor,
        auto_route: true,
        strict_route: true,
        dns_hijack,
    }
}

struct HostState {
    interfaces: Vec<(String, LinuxInterfaceState)>,
    routes: HashMap<String, String>,
    default_iface: Option<String>,
    dns_mode: LinuxDnsMode,
    /// Per-link `systemd-resolved` DNS servers keyed by interface name.
    link_dns: HashMap<String, Vec<String>>,
    /// Raw bytes of `/etc/resolv.conf` (static mode).
    resolv_conf_bytes: Vec<u8>,
    /// Helper's raw-byte backup of pre-TUN `/etc/resolv.conf`.
    resolv_conf_backup: Option<Vec<u8>>,
    next_ifindex: u32,
    fail_interface_state_probe: bool,
    fail_dns_servers_probe: bool,
}

impl Default for HostState {
    fn default() -> Self {
        let mut routes = HashMap::new();
        routes.insert("127.0.0.1".into(), "lo".into());
        Self {
            interfaces: vec![(
                "enp0s3".into(),
                LinuxInterfaceState {
                    ifindex: 2,
                    up: true,
                    addresses: vec!["192.168.1.50/24".into()],
                },
            )],
            routes,
            default_iface: Some("enp0s3".into()),
            dns_mode: LinuxDnsMode::SystemdResolved,
            link_dns: HashMap::new(),
            resolv_conf_bytes: Vec::new(),
            resolv_conf_backup: None,
            next_ifindex: 10,
            fail_interface_state_probe: false,
            fail_dns_servers_probe: false,
        }
    }
}

#[derive(Clone, Default)]
struct FakeLinuxHost {
    state: Arc<Mutex<HostState>>,
}

impl FakeLinuxHost {
    fn with_dns_mode(mode: LinuxDnsMode) -> Self {
        let host = Self::default();
        host.state.lock().unwrap().dns_mode = mode;
        host
    }

    fn set_default_iface(&self, iface: Option<&str>) {
        self.state.lock().unwrap().default_iface = iface.map(str::to_string);
    }

    fn set_link_dns(&self, link: &str, servers: Vec<String>) {
        self.state
            .lock()
            .unwrap()
            .link_dns
            .insert(link.to_string(), servers);
    }

    fn link_dns(&self, link: &str) -> Vec<String> {
        self.state
            .lock()
            .unwrap()
            .link_dns
            .get(link)
            .cloned()
            .unwrap_or_default()
    }

    fn set_resolv_conf_raw(&self, bytes: impl Into<Vec<u8>>) {
        self.state.lock().unwrap().resolv_conf_bytes = bytes.into();
    }

    fn resolv_conf_raw(&self) -> Vec<u8> {
        self.state.lock().unwrap().resolv_conf_bytes.clone()
    }

    fn resolv_conf_servers(&self) -> Vec<String> {
        let raw = self.resolv_conf_raw();
        parse_resolv_conf_servers(&String::from_utf8_lossy(&raw))
    }

    fn set_fail_interface_state_probe(&self, fail: bool) {
        self.state.lock().unwrap().fail_interface_state_probe = fail;
    }

    fn set_fail_dns_servers_probe(&self, fail: bool) {
        self.state.lock().unwrap().fail_dns_servers_probe = fail;
    }

    /// Simulates the helper's static `/etc/resolv.conf` marker + raw-byte
    /// backup/restore semantics.
    fn apply_static_resolv_conf(&self, servers: &[String]) {
        let mut st = self.state.lock().unwrap();
        if servers.is_empty() {
            if let Some(backup) = st.resolv_conf_backup.take() {
                st.resolv_conf_bytes = backup;
            }
            return;
        }
        if !st.resolv_conf_bytes.starts_with(MANAGED_MARKER.as_bytes()) {
            st.resolv_conf_backup = Some(st.resolv_conf_bytes.clone());
        }
        let mut out = format!("{MANAGED_MARKER}\n");
        for server in servers {
            out.push_str("nameserver ");
            out.push_str(server);
            out.push('\n');
        }
        st.resolv_conf_bytes = out.into_bytes();
    }

    fn add_tun(&self, name: &str, addresses: &[String]) -> u32 {
        let mut st = self.state.lock().unwrap();
        let ifindex = st.next_ifindex;
        st.next_ifindex += 1;
        st.interfaces.push((
            name.to_string(),
            LinuxInterfaceState {
                ifindex,
                up: true,
                addresses: addresses.to_vec(),
            },
        ));
        let cfg = TunConfig {
            interface_name: Some(name.to_string()),
            addresses: addresses.to_vec(),
            mtu: 9000,
            stack: TunStack::Gvisor,
            auto_route: true,
            strict_route: true,
            dns_hijack: false,
        };
        for dest in routes::auto_route_destinations(&cfg) {
            for probe in routes::route_probe_addresses(&dest) {
                st.routes.insert(probe, name.to_string());
            }
        }
        ifindex
    }

    fn add_interface_with_ifindex(&self, name: &str, ifindex: u32, addresses: &[String]) {
        let mut st = self.state.lock().unwrap();
        st.interfaces.push((
            name.to_string(),
            LinuxInterfaceState {
                ifindex,
                up: true,
                addresses: addresses.to_vec(),
            },
        ));
        let cfg = TunConfig {
            interface_name: Some(name.to_string()),
            addresses: addresses.to_vec(),
            mtu: 9000,
            stack: TunStack::Gvisor,
            auto_route: true,
            strict_route: true,
            dns_hijack: false,
        };
        for dest in routes::auto_route_destinations(&cfg) {
            for probe in routes::route_probe_addresses(&dest) {
                st.routes.insert(probe, name.to_string());
            }
        }
    }

    fn remove_tun(&self, name: &str) {
        let mut st = self.state.lock().unwrap();
        st.interfaces.retain(|(existing, _)| existing != name);
        st.routes.retain(|_, iface| iface != name);
        // Kernel + systemd-resolved automatically drop per-link DNS on destroy.
        st.link_dns.remove(name);
    }

    /// Removes the TUN interface only if its current `ifindex` matches the one
    /// owned by the stopping core (mirroring Linux closing its own TUN fd).
    fn remove_tun_matching(&self, name: &str, ifindex: u32) {
        let mut st = self.state.lock().unwrap();
        let matches = st
            .interfaces
            .iter()
            .any(|(existing, state)| existing == name && state.ifindex == ifindex);
        if matches {
            st.interfaces.retain(|(existing, _)| existing != name);
            st.routes.retain(|_, iface| iface != name);
            st.link_dns.remove(name);
        }
    }
}

impl LinuxHost for FakeLinuxHost {
    fn list_interface_names(&self) -> Result<Vec<String>, TunError> {
        Ok(self
            .state
            .lock()
            .unwrap()
            .interfaces
            .iter()
            .map(|(n, _)| n.clone())
            .collect())
    }

    fn interface_state(&self, name: &str) -> Result<Option<LinuxInterfaceState>, TunError> {
        let st = self.state.lock().unwrap();
        if st.fail_interface_state_probe {
            return Err(TunError::new(
                ErrorCode::TunHealthcheckFailed,
                format!("mock ip link show {name} failed"),
            ));
        }
        Ok(st
            .interfaces
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, s)| s.clone()))
    }

    fn route_interface(&self, destination: &str) -> Result<Option<String>, TunError> {
        let probe = routes::route_probe_address(destination);
        Ok(self.state.lock().unwrap().routes.get(&probe).cloned())
    }

    fn default_ipv4_interface(&self) -> Result<Option<String>, TunError> {
        Ok(self.state.lock().unwrap().default_iface.clone())
    }

    fn dns_mode(&self) -> LinuxDnsMode {
        self.state.lock().unwrap().dns_mode
    }

    fn dns_servers(&self, service: &str) -> Result<Vec<String>, TunError> {
        let st = self.state.lock().unwrap();
        if st.fail_dns_servers_probe {
            return Err(TunError::new(
                ErrorCode::TunHealthcheckFailed,
                format!("mock dns_servers({service}) failed"),
            ));
        }
        if service == LINUX_RESOLV_CONF_SERVICE {
            Ok(parse_resolv_conf_servers(&String::from_utf8_lossy(
                &st.resolv_conf_bytes,
            )))
        } else {
            Ok(st.link_dns.get(service).cloned().unwrap_or_default())
        }
    }
}

type SetDnsCalls = Arc<Mutex<Vec<(String, Vec<String>)>>>;
type SetDnsHook = Arc<dyn Fn(&str, &[String]) + Send + Sync>;

struct FakeCoreCoordinator {
    host: FakeLinuxHost,
    active_iface: Option<(String, u32)>,
    fail_stop: bool,
    fail_set_dns: bool,
    skip_ipv6_on_start: bool,
    set_dns_calls: SetDnsCalls,
    on_set_dns: Option<SetDnsHook>,
}

impl FakeCoreCoordinator {
    fn new(host: FakeLinuxHost) -> Self {
        Self {
            host,
            active_iface: None,
            fail_stop: false,
            fail_set_dns: false,
            skip_ipv6_on_start: false,
            set_dns_calls: Arc::new(Mutex::new(Vec::new())),
            on_set_dns: None,
        }
    }
}

impl CoreCoordinator for FakeCoreCoordinator {
    fn start_with_config(&mut self, config_path: &Path) -> Result<u32, TunError> {
        let raw = fs::read_to_string(config_path).unwrap();
        let cfg: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let tun = cfg["inbounds"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["type"] == "tun")
            .unwrap();
        let mut addresses: Vec<String> = tun["address"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        if self.skip_ipv6_on_start {
            addresses.retain(|addr| !addr.contains(':'));
        }
        let name = self
            .active_iface
            .as_ref()
            .map(|(n, _)| n.clone())
            .unwrap_or_else(|| "icebox0".into());
        let ifindex = self.host.add_tun(&name, &addresses);
        self.active_iface = Some((name, ifindex));
        Ok(4321)
    }

    fn stop(&mut self) -> Result<(), TunError> {
        if self.fail_stop {
            return Err(TunError::new(
                ErrorCode::TunRestoreFailed,
                "mock stop failed",
            ));
        }
        if let Some((name, ifindex)) = self.active_iface.take() {
            self.host.remove_tun_matching(&name, ifindex);
        }
        Ok(())
    }

    fn set_dns(&mut self, service: &str, servers: &[String]) -> Result<(), TunError> {
        // Mirror the real helper protocol validation (including MAX_DNS_SERVERS=4).
        ice_tun_sys::helper_protocol::validate_set_dns(service, servers)?;
        self.set_dns_calls
            .lock()
            .unwrap()
            .push((service.to_string(), servers.to_vec()));
        if let Some(hook) = &self.on_set_dns {
            hook(service, servers);
        }
        if self.fail_set_dns {
            return Err(TunError::new(
                ErrorCode::TunApplyFailed,
                format!("mock set_dns({service}) failed"),
            ));
        }
        if service == LINUX_RESOLV_CONF_SERVICE {
            self.host.apply_static_resolv_conf(servers);
        } else {
            self.host.set_link_dns(service, servers.to_vec());
        }
        Ok(())
    }
}

#[test]
fn prepare_validates_dual_stack_name_and_handles_collision() {
    let dir = temp_dir("prepare");
    let cfg_path = write_tun_config(&dir, &["10.0.0.1/30", "fdfe:dcba:9876::1/126"]);
    let host = FakeLinuxHost::default();
    host.add_tun("icebox0", &["10.0.0.1/30".into()]);
    let backend = LinuxTunBackend::new(
        OWNER,
        Box::new(host.clone()),
        Box::new(FakeCoreCoordinator::new(host)),
        cfg_path,
    );

    let mut cfg = linux_config(true);
    cfg.interface_name = None;
    let prepared = backend.prepare(&cfg).expect("auto pick free icebox1");
    assert_eq!(prepared.config.interface_name.as_deref(), Some("icebox1"));

    cfg.interface_name = Some("icebox0".into());
    let fallback = backend.prepare(&cfg).expect("collision fallback");
    assert_eq!(fallback.config.interface_name.as_deref(), Some("icebox1"));

    cfg.interface_name = Some("name-longer-than-15-chars".into());
    assert!(backend.prepare(&cfg).is_err());

    cfg.interface_name = Some("icebox2".into());
    cfg.addresses = vec!["10.0.0.1/30".into()];
    assert!(backend.prepare(&cfg).is_err(), "dual-stack required");

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn systemd_resolved_lifecycle_pins_interface_verifies_and_recovers_kill9() {
    let dir = temp_dir("resolved-lifecycle");
    let cfg_path = write_tun_config(&dir, &["10.0.0.1/30", "fdfe:dcba:9876::1/126"]);
    let journal_path = dir.join("tun-state.json");
    let host = FakeLinuxHost::with_dns_mode(LinuxDnsMode::SystemdResolved);
    let mut backend = LinuxTunBackend::new(
        OWNER,
        Box::new(host.clone()),
        Box::new(FakeCoreCoordinator::new(host.clone())),
        cfg_path.clone(),
    )
    .with_journal(journal_path.clone());

    let prepared = backend.prepare(&linux_config(true)).unwrap();
    let applied = backend.apply(&prepared).expect("apply");
    assert_eq!(applied.interface_name.as_deref(), Some("icebox0"));
    assert_eq!(applied.interface_id.as_deref(), Some("10"));
    assert_eq!(applied.core_pid, Some(4321));
    assert_eq!(host.link_dns("icebox0"), vec!["223.5.5.5", "119.29.29.29"]);

    // Outbound physical NIC was pinned in config.json.
    let pinned: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&cfg_path).unwrap()).unwrap();
    assert_eq!(pinned["route"]["default_interface"], "enp0s3");
    assert_eq!(pinned["route"]["auto_detect_interface"], false);

    let health = backend.verify(&applied).unwrap();
    assert!(health.all_ok(), "health must be all_ok: {health:?}");
    assert!(!health.nothing_owned);

    // DNS drift self-heal.
    host.set_link_dns("icebox0", vec![]);
    assert!(backend.reapply_dns_if_stale(&applied).unwrap());
    assert_eq!(host.link_dns("icebox0"), vec!["223.5.5.5", "119.29.29.29"]);

    // Simulate kill -9: kernel removes TUN + routes + systemd-resolved per-link DNS.
    let journal = TunJournal::load(&journal_path).unwrap().unwrap();
    assert_eq!(journal.last_completed_step, steps::DNS_APPLIED);
    host.remove_tun("icebox0");
    let outcome = backend.recover(&journal).unwrap();
    assert_eq!(outcome, RecoveryOutcome::Cleaned);

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn resolved_mode_restore_handles_normal_missing_link_and_nm_cleared_dns() {
    // 1. Normal stop in systemd-resolved mode.
    let dir = temp_dir("resolved-restore");
    let cfg_path = write_tun_config(&dir, &["10.0.0.1/30", "fdfe:dcba:9876::1/126"]);
    let journal_path = dir.join("tun-state.json");
    let host = FakeLinuxHost::with_dns_mode(LinuxDnsMode::SystemdResolved);
    host.set_resolv_conf_raw("nameserver 127.0.0.53\n");
    let mut backend = LinuxTunBackend::new(
        OWNER,
        Box::new(host.clone()),
        Box::new(FakeCoreCoordinator::new(host.clone())),
        cfg_path.clone(),
    )
    .with_journal(journal_path.clone());

    let prepared = backend.prepare(&linux_config(true)).unwrap();
    let applied = backend.apply(&prepared).unwrap();
    backend
        .restore(&applied)
        .expect("normal resolved restore succeeds");
    assert!(backend.verify(&applied).unwrap().nothing_owned);
    let journal = TunJournal::load(&journal_path).unwrap().unwrap();
    assert_eq!(journal.last_completed_step, steps::DNS_RESTORED);
    assert!(journal.dns_after.is_none());
    assert_eq!(
        host.resolv_conf_raw(),
        b"nameserver 127.0.0.53\n",
        "resolved mode must never touch static resolv.conf"
    );

    // 2. Link already gone before restore().
    let applied2 = backend.apply(&prepared).unwrap();
    host.remove_tun("icebox0");
    backend
        .restore(&applied2)
        .expect("missing link restore must be clean");
    assert!(backend.verify(&applied2).unwrap().nothing_owned);

    // 3. NetworkManager cleared per-link DNS on TUN link AND resolvectl revert
    // fails — once the TUN interface is destroyed by core stop, restore MUST
    // still succeed cleanly (no bogus RecoveryRequired, never falls through to
    // static resolv.conf).
    let applied3 = backend.apply(&prepared).unwrap();
    let applied3_ifindex: u32 = applied3.interface_id.as_deref().unwrap().parse().unwrap();
    host.set_link_dns("icebox0", vec![]);
    let mut failing_dns_coord = FakeCoreCoordinator::new(host.clone());
    failing_dns_coord.active_iface = Some(("icebox0".into(), applied3_ifindex));
    failing_dns_coord.fail_set_dns = true;
    let mut backend3 = LinuxTunBackend::new(
        OWNER,
        Box::new(host.clone()),
        Box::new(failing_dns_coord),
        cfg_path,
    )
    .with_journal(journal_path.clone());
    backend3
        .restore(&applied3)
        .expect("NM cleared link DNS + failed revert must still succeed once link is gone");
    assert!(backend3.verify(&applied3).unwrap().nothing_owned);
    assert_eq!(host.resolv_conf_raw(), b"nameserver 127.0.0.53\n");

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn static_resolv_conf_preserves_directives_scope_and_more_than_four_servers() {
    let dir = temp_dir("resolv-verbatim");
    let cfg_path = write_tun_config(&dir, &["10.0.0.1/30", "fdfe:dcba:9876::1/126"]);
    let journal_path = dir.join("tun-state.json");
    let host = FakeLinuxHost::with_dns_mode(LinuxDnsMode::StaticResolvConf);
    let original = b"# Generated by dhclient\n\
        domain corp.local\n\
        search corp.local lab.example\n\
        options edns0 rotate ndots:2\n\
        nameserver 192.168.1.1\n\
        nameserver fe80::1%eth0\n\
        nameserver 10.0.0.1\n\
        nameserver 10.0.0.2\n\
        nameserver 10.0.0.3\n";
    host.set_resolv_conf_raw(original.to_vec());

    // Verify Finding 6: journal ALREADY contains dns_before + dns_after BEFORE
    // coordinator.set_dns executes during apply().
    let journal_seen_before_set_dns = Arc::new(Mutex::new(false));
    let seen_flag = Arc::clone(&journal_seen_before_set_dns);
    let journal_path_for_hook = journal_path.clone();
    let mut coord = FakeCoreCoordinator::new(host.clone());
    coord.on_set_dns = Some(Arc::new(move |service, servers| {
        if service == LINUX_RESOLV_CONF_SERVICE && !servers.is_empty() {
            let j = TunJournal::load(&journal_path_for_hook).unwrap().unwrap();
            assert!(
                j.dns_before.is_some() && j.dns_after.is_some(),
                "journal must persist DNS intent before set_dns mutates OS DNS"
            );
            *seen_flag.lock().unwrap() = true;
        }
    }));

    let mut backend =
        LinuxTunBackend::new(OWNER, Box::new(host.clone()), Box::new(coord), cfg_path)
            .with_journal(journal_path.clone());

    let prepared = backend.prepare(&linux_config(true)).unwrap();
    let applied = backend.apply(&prepared).unwrap();
    assert!(*journal_seen_before_set_dns.lock().unwrap());
    assert_eq!(
        host.resolv_conf_servers(),
        vec!["223.5.5.5", "119.29.29.29"]
    );
    assert!(backend.verify(&applied).unwrap().all_ok());

    // Restore sends empty server list (never hits MAX_DNS_SERVERS=4) and
    // restores the exact raw bytes including search/domain/options/%scope.
    backend.restore(&applied).unwrap();
    assert_eq!(host.resolv_conf_raw(), original);
    assert!(backend.verify(&applied).unwrap().nothing_owned);

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn static_resolv_conf_compare_before_restore_and_external_edit_protection() {
    let dir = temp_dir("resolv-conf");
    let cfg_path = write_tun_config(&dir, &["10.0.0.1/30", "fdfe:dcba:9876::1/126"]);
    let journal_path = dir.join("tun-state.json");
    let host = FakeLinuxHost::with_dns_mode(LinuxDnsMode::StaticResolvConf);
    host.set_resolv_conf_raw(b"nameserver 192.168.1.1\n".to_vec());

    let mut backend = LinuxTunBackend::new(
        OWNER,
        Box::new(host.clone()),
        Box::new(FakeCoreCoordinator::new(host.clone())),
        cfg_path,
    )
    .with_journal(journal_path.clone());

    let prepared = backend.prepare(&linux_config(true)).unwrap();
    let applied = backend.apply(&prepared).unwrap();
    assert_eq!(
        host.resolv_conf_servers(),
        vec!["223.5.5.5", "119.29.29.29"]
    );
    assert!(backend.verify(&applied).unwrap().all_ok());

    // Normal restore returns original resolv.conf.
    backend.restore(&applied).unwrap();
    assert_eq!(host.resolv_conf_raw(), b"nameserver 192.168.1.1\n");

    // Second run: simulate an external user edit to /etc/resolv.conf before restore.
    let applied2 = backend.apply(&prepared).unwrap();
    host.set_resolv_conf_raw(b"nameserver 1.1.1.1\n".to_vec());
    let err = backend
        .restore(&applied2)
        .expect_err("external DNS preserved");
    assert_eq!(err.code, ErrorCode::TunRecoveryRequired);
    assert_eq!(
        host.resolv_conf_raw(),
        b"nameserver 1.1.1.1\n",
        "foreign edit must never be overwritten"
    );

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn static_resolv_conf_self_heal_updates_journal_baseline_and_backup() {
    let dir = temp_dir("resolv-self-heal");
    let cfg_path = write_tun_config(&dir, &["10.0.0.1/30", "fdfe:dcba:9876::1/126"]);
    let journal_path = dir.join("tun-state.json");
    let host = FakeLinuxHost::with_dns_mode(LinuxDnsMode::StaticResolvConf);
    let net_a = b"search home.lan\nnameserver 192.168.1.1\n";
    let net_b = b"search office.corp\noptions edns0\nnameserver 10.20.0.1\n";
    host.set_resolv_conf_raw(net_a.to_vec());

    let mut backend = LinuxTunBackend::new(
        OWNER,
        Box::new(host.clone()),
        Box::new(FakeCoreCoordinator::new(host.clone())),
        cfg_path,
    )
    .with_journal(journal_path.clone());

    let prepared = backend.prepare(&linux_config(true)).unwrap();
    let applied = backend.apply(&prepared).unwrap();

    // NetworkManager / dhclient rewrites /etc/resolv.conf on network roam.
    host.set_resolv_conf_raw(net_b.to_vec());
    assert!(backend.reapply_dns_if_stale(&applied).unwrap());
    assert_eq!(
        host.resolv_conf_servers(),
        vec!["223.5.5.5", "119.29.29.29"]
    );

    // Journal dns_before must reflect the new network baseline.
    let journal = TunJournal::load(&journal_path).unwrap().unwrap();
    assert_eq!(
        journal
            .dns_before
            .as_ref()
            .map(|s| s.platform_snapshot.as_str()),
        Some("resolv.conf\n10.20.0.1")
    );

    // Stopping TUN restores network B's exact raw bytes (never stale network A).
    backend.restore(&applied).unwrap();
    assert_eq!(host.resolv_conf_raw(), net_b);

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn static_resolv_conf_still_restored_when_core_stop_or_teardown_probe_fails() {
    // 1. Core stop fails (`fail_stop = true`): restore returns RecoveryRequired,
    // yet static /etc/resolv.conf is still restored cleanly.
    let dir = temp_dir("resolv-fail-stop");
    let cfg_path = write_tun_config(&dir, &["10.0.0.1/30", "fdfe:dcba:9876::1/126"]);
    let journal_path = dir.join("tun-state.json");
    let host = FakeLinuxHost::with_dns_mode(LinuxDnsMode::StaticResolvConf);
    let original = b"search corp.lan\nnameserver 192.168.1.1\n";
    host.set_resolv_conf_raw(original.to_vec());

    let mut coord = FakeCoreCoordinator::new(host.clone());
    coord.fail_stop = true;
    let mut backend = LinuxTunBackend::new(
        OWNER,
        Box::new(host.clone()),
        Box::new(coord),
        cfg_path.clone(),
    )
    .with_journal(journal_path.clone());

    let prepared = backend.prepare(&linux_config(true)).unwrap();
    let applied = backend.apply(&prepared).unwrap();
    assert_eq!(
        host.resolv_conf_servers(),
        vec!["223.5.5.5", "119.29.29.29"]
    );

    let err = backend
        .restore(&applied)
        .expect_err("failed core stop must surface RecoveryRequired");
    assert_eq!(err.code, ErrorCode::TunRecoveryRequired);
    assert_eq!(
        host.resolv_conf_raw(),
        original,
        "static /etc/resolv.conf must still be restored when core stop fails"
    );

    // 2. Host probe error during teardown (`interface_state` fails) + DNS
    // probe failure: mapped to RecoveryRequired without early `?` panic/skip.
    host.remove_tun("icebox0");
    let mut backend2 = LinuxTunBackend::new(
        OWNER,
        Box::new(host.clone()),
        Box::new(FakeCoreCoordinator::new(host.clone())),
        cfg_path,
    )
    .with_journal(journal_path);
    let applied2 = backend2.apply(&prepared).unwrap();
    host.set_fail_interface_state_probe(true);
    let err = backend2
        .restore(&applied2)
        .expect_err("teardown probe error must map to RecoveryRequired");
    assert_eq!(err.code, ErrorCode::TunRecoveryRequired);
    assert_eq!(
        host.resolv_conf_raw(),
        original,
        "static /etc/resolv.conf must still be restored when interface probe fails"
    );

    // 3. DNS probe error during restore maps to RecoveryRequired.
    host.set_fail_interface_state_probe(false);
    let applied3 = backend2.apply(&prepared).unwrap();
    host.set_fail_dns_servers_probe(true);
    let err = backend2
        .restore(&applied3)
        .expect_err("dns_servers probe error must map to RecoveryRequired");
    assert_eq!(err.code, ErrorCode::TunRecoveryRequired);

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn static_resolv_conf_kill9_recovery_restores_dns() {
    let dir = temp_dir("resolv-kill9");
    let cfg_path = write_tun_config(&dir, &["10.0.0.1/30", "fdfe:dcba:9876::1/126"]);
    let journal_path = dir.join("tun-state.json");
    let host = FakeLinuxHost::with_dns_mode(LinuxDnsMode::StaticResolvConf);
    let original = b"options edns0\nnameserver 192.168.1.1\n";
    host.set_resolv_conf_raw(original.to_vec());

    let mut backend = LinuxTunBackend::new(
        OWNER,
        Box::new(host.clone()),
        Box::new(FakeCoreCoordinator::new(host.clone())),
        cfg_path,
    )
    .with_journal(journal_path.clone());

    let prepared = backend.prepare(&linux_config(true)).unwrap();
    let _applied = backend.apply(&prepared).unwrap();
    let mut journal = TunJournal::load(&journal_path).unwrap().unwrap();
    journal.state = JournalState::Applied;
    journal.save(&journal_path).unwrap();

    // Kernel tears down TUN on kill -9, but static /etc/resolv.conf remains mutated.
    host.remove_tun("icebox0");
    let outcome = backend.recover(&journal).unwrap();
    assert_eq!(outcome, RecoveryOutcome::Cleaned);
    assert_eq!(host.resolv_conf_raw(), original);

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn ifindex_mismatch_rejects_foreign_interface_in_verify_heal_restore_and_apply() {
    let dir = temp_dir("ifindex-lock");
    let cfg_path = write_tun_config(&dir, &["10.0.0.1/30", "fdfe:dcba:9876::1/126"]);
    let journal_path = dir.join("tun-state.json");
    let host = FakeLinuxHost::with_dns_mode(LinuxDnsMode::SystemdResolved);
    let coord = FakeCoreCoordinator::new(host.clone());
    let dns_calls = Arc::clone(&coord.set_dns_calls);

    let mut backend =
        LinuxTunBackend::new(OWNER, Box::new(host.clone()), Box::new(coord), cfg_path)
            .with_journal(journal_path.clone());

    let prepared = backend.prepare(&linux_config(true)).unwrap();
    let applied = backend.apply(&prepared).unwrap();
    assert_eq!(applied.interface_id.as_deref(), Some("10"));

    // Our interface disappears and an unrelated process creates `icebox0` with
    // a different ifindex (99) and its own DNS.
    host.remove_tun("icebox0");
    host.add_interface_with_ifindex(
        "icebox0",
        99,
        &["10.0.0.1/30".into(), "fdfe:dcba:9876::1/126".into()],
    );
    host.set_link_dns("icebox0", vec!["9.9.9.9".into()]);

    // verify(): unrelated interface must NOT be attributed to us.
    let health = backend.verify(&applied).unwrap();
    assert!(!health.interface_up);
    assert!(!health.addresses_present);
    assert!(!health.routes_owned);
    assert!(
        health.nothing_owned,
        "our interface is gone; unrelated ifindex=99 must not count as owned: {health:?}"
    );

    // reapply_dns_if_stale(): must NOT mutate the foreign interface's DNS.
    dns_calls.lock().unwrap().clear();
    assert!(!backend.reapply_dns_if_stale(&applied).unwrap());
    assert!(dns_calls.lock().unwrap().is_empty());
    assert_eq!(host.link_dns("icebox0"), vec!["9.9.9.9"]);

    // restore(): must NOT revert DNS on the foreign interface and must NOT hang
    // waiting for it to disappear.
    backend
        .restore(&applied)
        .expect("restore ignores foreign ifindex");
    assert!(
        dns_calls.lock().unwrap().is_empty(),
        "must not invoke set_dns on foreign ifindex=99"
    );
    assert_eq!(host.link_dns("icebox0"), vec!["9.9.9.9"]);

    // apply(): must refuse to adopt a pre-existing interface with the same name.
    let err = backend
        .apply(&prepared)
        .expect_err("apply must reject pre-existing interface");
    assert_eq!(err.code, ErrorCode::TunApplyFailed);
    assert!(
        err.message.contains("already exists") && err.message.contains("99"),
        "unexpected error: {}",
        err.message
    );

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn apply_rollback_with_fail_stop_returns_recovery_required() {
    let dir = temp_dir("apply-rollback-fail-stop");
    let cfg_path = write_tun_config(&dir, &["10.0.0.1/30", "fdfe:dcba:9876::1/126"]);
    let journal_path = dir.join("tun-state.json");
    let host = FakeLinuxHost::with_dns_mode(LinuxDnsMode::StaticResolvConf);
    host.set_resolv_conf_raw(b"nameserver 192.168.1.1\n".to_vec());

    let mut coord = FakeCoreCoordinator::new(host.clone());
    coord.fail_set_dns = true;
    coord.fail_stop = true;
    let mut backend = LinuxTunBackend::new(OWNER, Box::new(host), Box::new(coord), cfg_path)
        .with_journal(journal_path);

    let prepared = backend.prepare(&linux_config(true)).unwrap();
    let err = backend
        .apply(&prepared)
        .expect_err("rollback with fail_stop must fail");
    assert_eq!(
        err.code,
        ErrorCode::TunRecoveryRequired,
        "failed rollback stop must escalate to TunRecoveryRequired, got: {err}"
    );

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn ppp0_is_allowed_uplink_and_ipv6_disabled_host_gives_actionable_error() {
    let dir = temp_dir("ppp-and-ipv6");
    let cfg_path = write_tun_config(&dir, &["10.0.0.1/30", "fdfe:dcba:9876::1/126"]);
    let journal_path = dir.join("tun-state.json");
    let host = FakeLinuxHost::with_dns_mode(LinuxDnsMode::SystemdResolved);
    host.set_default_iface(Some("ppp0"));

    // 1. PPPoE (`ppp0`) is accepted and pinned as default_interface.
    let mut backend = LinuxTunBackend::new(
        OWNER,
        Box::new(host.clone()),
        Box::new(FakeCoreCoordinator::new(host.clone())),
        cfg_path.clone(),
    )
    .with_journal(journal_path.clone());
    let prepared = backend.prepare(&linux_config(true)).unwrap();
    let applied = backend.apply(&prepared).expect("ppp0 uplink must succeed");
    let pinned: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&cfg_path).unwrap()).unwrap();
    assert_eq!(pinned["route"]["default_interface"], "ppp0");
    backend.restore(&applied).unwrap();

    // 2. Host where IPv4 converges on icebox0 but IPv6 fails (IPv6 disabled).
    let mut coord_no_v6 = FakeCoreCoordinator::new(host.clone());
    coord_no_v6.skip_ipv6_on_start = true;
    let mut backend_no_v6 =
        LinuxTunBackend::new(OWNER, Box::new(host), Box::new(coord_no_v6), cfg_path)
            .with_journal(journal_path);
    let err = backend_no_v6
        .apply(&prepared)
        .expect_err("IPv6-disabled host must fail convergence");
    assert_eq!(err.code, ErrorCode::TunHealthcheckFailed);
    assert!(
        err.message.contains("IPv6") && err.message.contains("disable_ipv6"),
        "error should clearly explain host IPv6 may be disabled: {}",
        err.message
    );

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn classify_dns_mode_checks_stub_symlink_uplink_symlink_regular_file_and_loopback() {
    // Symlink to systemd-resolved stub (absolute and relative).
    assert_eq!(
        classify_dns_mode(
            true,
            false,
            Some(Path::new("/run/systemd/resolve/stub-resolv.conf")),
            Some("nameserver 127.0.0.53\n"),
        ),
        LinuxDnsMode::SystemdResolved
    );
    assert_eq!(
        classify_dns_mode(
            true,
            false,
            Some(Path::new("../run/systemd/resolve/stub-resolv.conf")),
            None,
        ),
        LinuxDnsMode::SystemdResolved
    );

    // Symlink to systemd-resolved uplink resolv.conf (NOT stub) -> Unmanaged,
    // even when resolvectl status succeeds (prevents silent LAN DNS leak).
    assert_eq!(
        classify_dns_mode(
            true,
            false,
            Some(Path::new("/run/systemd/resolve/resolv.conf")),
            Some("nameserver 192.168.1.1\n"),
        ),
        LinuxDnsMode::Unmanaged
    );

    // Regular file with 127.0.0.53 / 127.0.0.54 and resolvectl ok -> SystemdResolved.
    assert_eq!(
        classify_dns_mode(true, true, None, Some("nameserver 127.0.0.53\n")),
        LinuxDnsMode::SystemdResolved
    );
    assert_eq!(
        classify_dns_mode(
            true,
            true,
            None,
            Some("nameserver 127.0.0.53\nnameserver 127.0.0.54\n"),
        ),
        LinuxDnsMode::SystemdResolved
    );

    // Regular file mixing stub + LAN server -> must NOT trust resolved alone,
    // fall back to StaticResolvConf.
    assert_eq!(
        classify_dns_mode(
            true,
            true,
            None,
            Some("nameserver 127.0.0.53\nnameserver 192.168.1.1\n"),
        ),
        LinuxDnsMode::StaticResolvConf
    );

    // Regular file with LAN DNS (e.g. NM rc-manager=file) while resolved runs ->
    // fall back to StaticResolvConf.
    assert_eq!(
        classify_dns_mode(true, true, None, Some("nameserver 192.168.1.1\n")),
        LinuxDnsMode::StaticResolvConf
    );

    // Regular file without systemd-resolved -> StaticResolvConf.
    assert_eq!(
        classify_dns_mode(false, true, None, Some("nameserver 127.0.0.53\n")),
        LinuxDnsMode::StaticResolvConf
    );

    // Foreign symlink without systemd-resolved -> Unmanaged.
    assert_eq!(
        classify_dns_mode(
            false,
            false,
            Some(Path::new("/run/NetworkManager/resolv.conf")),
            Some("nameserver 192.168.1.1\n"),
        ),
        LinuxDnsMode::Unmanaged
    );

    // Missing /etc/resolv.conf -> Unmanaged.
    assert_eq!(
        classify_dns_mode(true, false, None, None),
        LinuxDnsMode::Unmanaged
    );
}

#[test]
fn create_backend_reports_supported_on_linux() {
    let backend = create_backend(
        OWNER,
        PathBuf::from("/tmp/ice-box-linux-test-config.json"),
        None,
        PathBuf::from("/tmp/ice-box-linux-test.log"),
    );
    let cap = backend.capability();
    #[cfg(target_os = "linux")]
    {
        assert!(cap.supported);
        assert!(cap.ipv4 && cap.ipv6 && cap.dns_hijack);
        assert!(cap.reason.is_none());
    }
    let _ = cap;
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "live host reads; run manually on a real Linux host"]
fn live_process_linux_host_reads_match_real_host() {
    let host = ProcessLinuxHost;
    let names = host.list_interface_names().expect("ip -o link show");
    assert!(names.iter().any(|n| n == "lo"));
    let lo = host.interface_state("lo").expect("ip link lo");
    assert!(lo.is_some());
    assert_eq!(
        host.route_interface("127.0.0.1")
            .expect("route get lo")
            .as_deref(),
        Some("lo")
    );
}
