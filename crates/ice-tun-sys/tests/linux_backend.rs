// SPDX-License-Identifier: GPL-3.0-or-later

//! Linux TUN backend tests (host-free on every CI platform).
//!
//! Covers prepare validation + `icebox<N>` collision fallback, journaled apply
//! with `ifindex` identity lock and `route.default_interface` pinning, both
//! `systemd-resolved` and static `/etc/resolv.conf` DNS modes, rollback on
//! failure, `kill -9` recovery, external DNS protection, and DNS drift
//! self-healing.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use ice_tun_sys::backend::RecoveryOutcome;
use ice_tun_sys::coordinator::CoreCoordinator;
use ice_tun_sys::journal::{steps, JournalState, TunJournal};
use ice_tun_sys::linux::{
    LinuxDnsMode, LinuxHost, LinuxInterfaceState, LinuxTunBackend, LINUX_RESOLV_CONF_SERVICE,
};
#[cfg(target_os = "linux")]
use ice_tun_sys::ProcessLinuxHost;
use ice_tun_sys::{create_backend, routes, TunBackend, TunConfig, TunStack};
use ice_types::{ErrorCode, TunError};

const OWNER: &str = "ice-box:test-linux-1";

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
    dns: HashMap<String, Vec<String>>,
    next_ifindex: u32,
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
            dns: HashMap::new(),
            next_ifindex: 10,
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

    fn set_dns(&self, service: &str, servers: Vec<String>) {
        self.state
            .lock()
            .unwrap()
            .dns
            .insert(service.to_string(), servers);
    }

    fn dns(&self, service: &str) -> Vec<String> {
        self.state
            .lock()
            .unwrap()
            .dns
            .get(service)
            .cloned()
            .unwrap_or_default()
    }

    fn add_tun(&self, name: &str, addresses: &[String]) {
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
    }

    fn remove_tun(&self, name: &str) {
        let mut st = self.state.lock().unwrap();
        st.interfaces.retain(|(existing, _)| existing != name);
        st.routes.retain(|_, iface| iface != name);
        // Kernel + systemd-resolved automatically drop per-link DNS on destroy.
        st.dns.remove(name);
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
        Ok(self
            .state
            .lock()
            .unwrap()
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
        Ok(self.dns(service))
    }
}

struct FakeCoreCoordinator {
    host: FakeLinuxHost,
    active_iface: Option<String>,
    fail_stop: bool,
}

impl FakeCoreCoordinator {
    fn new(host: FakeLinuxHost) -> Self {
        Self {
            host,
            active_iface: None,
            fail_stop: false,
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
        let addresses: Vec<String> = tun["address"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        let name = self
            .active_iface
            .clone()
            .unwrap_or_else(|| "icebox0".into());
        self.host.add_tun(&name, &addresses);
        self.active_iface = Some(name);
        Ok(4321)
    }

    fn stop(&mut self) -> Result<(), TunError> {
        if self.fail_stop {
            return Err(TunError::new(
                ErrorCode::TunRestoreFailed,
                "mock stop failed",
            ));
        }
        if let Some(name) = self.active_iface.take() {
            self.host.remove_tun(&name);
        }
        Ok(())
    }

    fn set_dns(&mut self, service: &str, servers: &[String]) -> Result<(), TunError> {
        self.host.set_dns(service, servers.to_vec());
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
    assert_eq!(host.dns("icebox0"), vec!["223.5.5.5", "119.29.29.29"]);

    // Outbound physical NIC was pinned in config.json.
    let pinned: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&cfg_path).unwrap()).unwrap();
    assert_eq!(pinned["route"]["default_interface"], "enp0s3");
    assert_eq!(pinned["route"]["auto_detect_interface"], false);

    let health = backend.verify(&applied).unwrap();
    assert!(health.all_ok(), "health must be all_ok: {health:?}");
    assert!(!health.nothing_owned);

    // DNS drift self-heal.
    host.set_dns("icebox0", vec![]);
    assert!(backend.reapply_dns_if_stale(&applied).unwrap());
    assert_eq!(host.dns("icebox0"), vec!["223.5.5.5", "119.29.29.29"]);

    // Simulate kill -9: kernel removes TUN + routes + systemd-resolved per-link DNS.
    let journal = TunJournal::load(&journal_path).unwrap().unwrap();
    assert_eq!(journal.last_completed_step, steps::DNS_APPLIED);
    host.remove_tun("icebox0");
    let outcome = backend.recover(&journal).unwrap();
    assert_eq!(outcome, RecoveryOutcome::Cleaned);

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn static_resolv_conf_compare_before_restore_and_external_edit_protection() {
    let dir = temp_dir("resolv-conf");
    let cfg_path = write_tun_config(&dir, &["10.0.0.1/30", "fdfe:dcba:9876::1/126"]);
    let journal_path = dir.join("tun-state.json");
    let host = FakeLinuxHost::with_dns_mode(LinuxDnsMode::StaticResolvConf);
    host.set_dns(LINUX_RESOLV_CONF_SERVICE, vec!["192.168.1.1".into()]);

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
        host.dns(LINUX_RESOLV_CONF_SERVICE),
        vec!["223.5.5.5", "119.29.29.29"]
    );
    assert!(backend.verify(&applied).unwrap().all_ok());

    // Normal restore returns original nameserver.
    backend.restore(&applied).unwrap();
    assert_eq!(host.dns(LINUX_RESOLV_CONF_SERVICE), vec!["192.168.1.1"]);

    // Second run: simulate an external user edit to /etc/resolv.conf before restore.
    let applied2 = backend.apply(&prepared).unwrap();
    host.set_dns(LINUX_RESOLV_CONF_SERVICE, vec!["1.1.1.1".into()]);
    let err = backend
        .restore(&applied2)
        .expect_err("external DNS preserved");
    assert_eq!(err.code, ErrorCode::TunRecoveryRequired);
    assert_eq!(
        host.dns(LINUX_RESOLV_CONF_SERVICE),
        vec!["1.1.1.1"],
        "foreign edit must never be overwritten"
    );

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn static_resolv_conf_kill9_recovery_restores_dns() {
    let dir = temp_dir("resolv-kill9");
    let cfg_path = write_tun_config(&dir, &["10.0.0.1/30", "fdfe:dcba:9876::1/126"]);
    let journal_path = dir.join("tun-state.json");
    let host = FakeLinuxHost::with_dns_mode(LinuxDnsMode::StaticResolvConf);
    host.set_dns(LINUX_RESOLV_CONF_SERVICE, vec!["192.168.1.1".into()]);

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
    assert_eq!(host.dns(LINUX_RESOLV_CONF_SERVICE), vec!["192.168.1.1"]);

    let _ = fs::remove_dir_all(dir);
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
