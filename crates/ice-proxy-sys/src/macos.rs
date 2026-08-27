//! macOS system proxy via `/usr/sbin/networksetup`.

use std::process::Command;

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::bypass::bypass_domains;
use crate::{ProxyBackup, ProxyEndpoints, ProxySysError, SystemProxy};

const NETWORKSETUP: &str = "/usr/sbin/networksetup";

/// Runs `networksetup` (injectable for tests).
pub trait NetworkSetupRunner: Send {
    fn run(&self, args: &[&str]) -> Result<String, ProxySysError>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct RealNetworkSetup;

impl NetworkSetupRunner for RealNetworkSetup {
    fn run(&self, args: &[&str]) -> Result<String, ProxySysError> {
        let output = Command::new(NETWORKSETUP)
            .args(args)
            .output()
            .map_err(|e| ProxySysError::ApplyFailed(format!("spawn networksetup: {e}")))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        if !output.status.success() {
            let msg = format!(
                "networksetup {:?} failed ({}): {}{}",
                args,
                output.status,
                stdout.trim(),
                if stderr.trim().is_empty() {
                    String::new()
                } else {
                    format!(" / {}", stderr.trim())
                }
            );
            return Err(ProxySysError::ApplyFailed(msg));
        }
        Ok(stdout)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ServiceProxyState {
    pub enabled: bool,
    pub server: String,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServiceBackup {
    pub name: String,
    pub web: ServiceProxyState,
    pub secure_web: ServiceProxyState,
    pub socks: ServiceProxyState,
    pub bypass: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct MacosSystemProxy<R: NetworkSetupRunner = RealNetworkSetup> {
    runner: R,
}

impl MacosSystemProxy<RealNetworkSetup> {
    pub fn new() -> Self {
        Self {
            runner: RealNetworkSetup,
        }
    }
}

impl Default for MacosSystemProxy<RealNetworkSetup> {
    fn default() -> Self {
        Self::new()
    }
}

impl<R: NetworkSetupRunner> MacosSystemProxy<R> {
    pub fn with_runner(runner: R) -> Self {
        Self { runner }
    }

    fn list_enabled_services(&self) -> Result<Vec<String>, ProxySysError> {
        let out = self.runner.run(&["-listallnetworkservices"])?;
        let mut names = Vec::new();
        for line in out.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            // Header line from networksetup
            if line.starts_with("An asterisk") {
                continue;
            }
            // Disabled services are prefixed with "* "
            if let Some(rest) = line.strip_prefix("*") {
                let _ = rest.trim();
                continue;
            }
            names.push(line.to_string());
        }
        if names.is_empty() {
            return Err(ProxySysError::ApplyFailed(
                "no enabled network services found".into(),
            ));
        }
        Ok(names)
    }

    fn get_proxy(&self, flag: &str, service: &str) -> Result<ServiceProxyState, ProxySysError> {
        let out = self.runner.run(&[flag, service])?;
        Ok(parse_proxy_get_output(&out))
    }

    fn get_bypass(&self, service: &str) -> Result<Vec<String>, ProxySysError> {
        let out = self.runner.run(&["-getproxybypassdomains", service])?;
        Ok(parse_bypass_output(&out))
    }

    fn set_proxy(
        &self,
        set_cmd: &str,
        state_cmd: &str,
        service: &str,
        host: &str,
        port: u16,
    ) -> Result<(), ProxySysError> {
        self.runner
            .run(&[set_cmd, service, host, &port.to_string()])?;
        self.runner.run(&[state_cmd, service, "on"])?;
        Ok(())
    }

    fn set_proxy_state(
        &self,
        state_cmd: &str,
        service: &str,
        on: bool,
    ) -> Result<(), ProxySysError> {
        self.runner
            .run(&[state_cmd, service, if on { "on" } else { "off" }])?;
        Ok(())
    }

    fn restore_one_proxy(
        &self,
        set_cmd: &str,
        state_cmd: &str,
        service: &str,
        state: &ServiceProxyState,
    ) -> Result<(), ProxySysError> {
        if state.enabled && !state.server.is_empty() && state.port > 0 {
            self.set_proxy(set_cmd, state_cmd, service, &state.server, state.port)?;
        } else {
            // Clear host/port then disable — avoids leaving stale server when off.
            if !state.server.is_empty() && state.port > 0 {
                let _ =
                    self.runner
                        .run(&[set_cmd, service, &state.server, &state.port.to_string()]);
            }
            self.set_proxy_state(state_cmd, service, false)?;
        }
        Ok(())
    }

    fn restore_service(&self, svc: &ServiceBackup) -> Result<(), ProxySysError> {
        self.restore_one_proxy("-setwebproxy", "-setwebproxystate", &svc.name, &svc.web)
            .map_err(map_restore)?;
        self.restore_one_proxy(
            "-setsecurewebproxy",
            "-setsecurewebproxystate",
            &svc.name,
            &svc.secure_web,
        )
        .map_err(map_restore)?;
        self.restore_one_proxy(
            "-setsocksfirewallproxy",
            "-setsocksfirewallproxystate",
            &svc.name,
            &svc.socks,
        )
        .map_err(map_restore)?;

        if svc.bypass.is_empty() {
            self.runner
                .run(&["-setproxybypassdomains", &svc.name, "Empty"])
                .map_err(map_restore)?;
        } else {
            let mut args: Vec<&str> = vec!["-setproxybypassdomains", &svc.name];
            let owned: Vec<&str> = svc.bypass.iter().map(String::as_str).collect();
            args.extend(owned);
            self.runner.run(&args).map_err(map_restore)?;
        }
        Ok(())
    }

    fn restore_services_from_backup(
        &self,
        backup: &ProxyBackup,
        service_names: &[String],
    ) -> Result<(), ProxySysError> {
        let services: Vec<ServiceBackup> = match backup.extra.get("services") {
            Some(v) => serde_json::from_value(v.clone()).map_err(|e| {
                ProxySysError::RestoreFailed(format!("parse backup.extra.services: {e}"))
            })?,
            None => return Ok(()),
        };

        for name in service_names {
            let Some(svc) = services.iter().find(|s| &s.name == name) else {
                continue;
            };
            self.restore_service(svc)?;
        }
        Ok(())
    }

    fn backup_services(&self, services: &[String]) -> Result<ProxyBackup, ProxySysError> {
        let mut entries = Vec::new();
        let mut any_enabled = false;
        let mut first_http: Option<String> = None;
        let mut first_https: Option<String> = None;
        let mut first_socks: Option<String> = None;

        for name in services {
            let web = self.get_proxy("-getwebproxy", name)?;
            let secure_web = self.get_proxy("-getsecurewebproxy", name)?;
            let socks = self.get_proxy("-getsocksfirewallproxy", name)?;
            let bypass = self.get_bypass(name)?;

            if web.enabled {
                any_enabled = true;
                if first_http.is_none() {
                    first_http = Some(format!("{}:{}", web.server, web.port));
                }
            }
            if secure_web.enabled {
                any_enabled = true;
                if first_https.is_none() {
                    first_https = Some(format!("{}:{}", secure_web.server, secure_web.port));
                }
            }
            if socks.enabled {
                any_enabled = true;
                if first_socks.is_none() {
                    first_socks = Some(format!("{}:{}", socks.server, socks.port));
                }
            }

            entries.push(ServiceBackup {
                name: name.clone(),
                web,
                secure_web,
                socks,
                bypass,
            });
        }

        Ok(ProxyBackup {
            enabled: any_enabled,
            http: first_http,
            https: first_https,
            socks: first_socks,
            extra: json!({ "services": entries }),
        })
    }

    fn apply_one_service(
        &self,
        name: &str,
        endpoints: &ProxyEndpoints,
        bypass: &[&str],
    ) -> Result<(), ProxySysError> {
        let host = &endpoints.http_host;
        let port = endpoints.http_port;
        let socks_host = endpoints.socks_host.as_deref().unwrap_or(host);
        let socks_port = endpoints.socks_port.unwrap_or(port);

        self.set_proxy("-setwebproxy", "-setwebproxystate", name, host, port)
            .map_err(map_apply)?;
        self.set_proxy(
            "-setsecurewebproxy",
            "-setsecurewebproxystate",
            name,
            host,
            port,
        )
        .map_err(map_apply)?;
        self.set_proxy(
            "-setsocksfirewallproxy",
            "-setsocksfirewallproxystate",
            name,
            socks_host,
            socks_port,
        )
        .map_err(map_apply)?;

        let mut args: Vec<&str> = vec!["-setproxybypassdomains", name];
        args.extend(bypass.iter().copied());
        self.runner.run(&args).map_err(map_apply)?;
        Ok(())
    }
}

impl<R: NetworkSetupRunner> SystemProxy for MacosSystemProxy<R> {
    fn backup(&self) -> Result<ProxyBackup, ProxySysError> {
        let services = self.list_enabled_services()?;
        self.backup_services(&services)
    }

    fn apply(&self, endpoints: &ProxyEndpoints) -> Result<(), ProxySysError> {
        let services = self.list_enabled_services()?;
        // Reuse the listed services so apply does not spawn a second
        // `networksetup -listallnetworkservices` via `backup()`.
        let pre_backup = self.backup_services(&services)?;
        let bypass = bypass_domains();
        let mut applied = Vec::new();

        for name in &services {
            if let Err(err) = self.apply_one_service(name, endpoints, &bypass) {
                let mut rollback = applied.clone();
                rollback.push(name.clone());
                if let Err(restore_err) = self.restore_services_from_backup(&pre_backup, &rollback)
                {
                    tracing::error!(error = %restore_err, "rollback partial proxy apply");
                }
                return Err(err);
            }
            applied.push(name.clone());
        }
        Ok(())
    }

    fn restore(&self, backup: &ProxyBackup) -> Result<(), ProxySysError> {
        let services: Vec<ServiceBackup> = match backup.extra.get("services") {
            Some(v) => serde_json::from_value(v.clone()).map_err(|e| {
                ProxySysError::RestoreFailed(format!("parse backup.extra.services: {e}"))
            })?,
            None => {
                // Legacy / empty backup: turn proxies off on all current services.
                let names = self.list_enabled_services().map_err(map_restore)?;
                for name in names {
                    self.set_proxy_state("-setwebproxystate", &name, false)
                        .map_err(map_restore)?;
                    self.set_proxy_state("-setsecurewebproxystate", &name, false)
                        .map_err(map_restore)?;
                    self.set_proxy_state("-setsocksfirewallproxystate", &name, false)
                        .map_err(map_restore)?;
                }
                return Ok(());
            }
        };

        // A service may have been deleted since apply; skipping it keeps the restore
        // from aborting on the first missing service and leaving the rest unrestored.
        // When listing fails (e.g. no enabled services at all), fall back to restoring
        // every backed-up service as before.
        let current_names = match self.list_enabled_services() {
            Ok(names) => names,
            Err(err) => {
                tracing::warn!(error = %err, "list services during restore failed; restoring all backed-up services");
                Vec::new()
            }
        };

        for svc in services {
            if !current_names.iter().any(|n| n == &svc.name) {
                tracing::warn!(
                    service = %svc.name,
                    "skipping restore of network service that no longer exists"
                );
                continue;
            }
            self.restore_service(&svc).map_err(map_restore)?;
        }
        Ok(())
    }
}

fn map_apply(err: ProxySysError) -> ProxySysError {
    match err {
        ProxySysError::ApplyFailed(m) => ProxySysError::ApplyFailed(m),
        other => ProxySysError::ApplyFailed(other.to_string()),
    }
}

fn map_restore(err: ProxySysError) -> ProxySysError {
    match err {
        ProxySysError::RestoreFailed(m) => ProxySysError::RestoreFailed(m),
        ProxySysError::ApplyFailed(m) => ProxySysError::RestoreFailed(m),
        other => ProxySysError::RestoreFailed(other.to_string()),
    }
}

pub fn parse_proxy_get_output(out: &str) -> ServiceProxyState {
    let mut enabled = false;
    let mut server = String::new();
    let mut port = 0u16;
    for line in out.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("Enabled:") {
            let v = rest.trim().to_ascii_lowercase();
            enabled = matches!(v.as_str(), "yes" | "on" | "true" | "1");
        } else if let Some(rest) = line.strip_prefix("Server:") {
            server = rest.trim().to_string();
        } else if let Some(rest) = line.strip_prefix("Port:") {
            port = rest.trim().parse().unwrap_or(0);
        }
    }
    ServiceProxyState {
        enabled,
        server,
        port,
    }
}

pub fn parse_bypass_output(out: &str) -> Vec<String> {
    let trimmed = out.trim();
    if trimmed.is_empty()
        || trimmed.eq_ignore_ascii_case("empty")
        || trimmed.contains("There aren't any")
    {
        return Vec::new();
    }
    trimmed
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    #[derive(Default, Clone)]
    struct MockRunner {
        /// Keyed by joined args; value is stdout or error message (if starts with "ERR:").
        responses: Arc<Mutex<HashMap<String, String>>>,
        calls: Arc<Mutex<Vec<String>>>,
    }

    impl MockRunner {
        fn key(args: &[&str]) -> String {
            args.join(" ")
        }

        fn set(&self, args: &[&str], stdout: &str) {
            self.responses
                .lock()
                .unwrap()
                .insert(Self::key(args), stdout.to_string());
        }

        fn set_err(&self, args: &[&str], msg: &str) {
            self.responses
                .lock()
                .unwrap()
                .insert(Self::key(args), format!("ERR:{msg}"));
        }
    }

    impl NetworkSetupRunner for MockRunner {
        fn run(&self, args: &[&str]) -> Result<String, ProxySysError> {
            let key = Self::key(args);
            self.calls.lock().unwrap().push(key.clone());
            let map = self.responses.lock().unwrap();
            // Prefer exact match; for set* with variable hosts, match by prefix of first tokens.
            if let Some(v) = map.get(&key) {
                if let Some(msg) = v.strip_prefix("ERR:") {
                    return Err(ProxySysError::ApplyFailed(msg.to_string()));
                }
                return Ok(v.clone());
            }
            // Prefix match for setwebproxy SERVICE host port etc.
            for (k, v) in map.iter() {
                if key.starts_with(k.trim_end_matches('*'))
                    || k.ends_with('*') && {
                        let p = k.trim_end_matches('*');
                        key.starts_with(p)
                    }
                {
                    if let Some(msg) = v.strip_prefix("ERR:") {
                        return Err(ProxySysError::ApplyFailed(msg.to_string()));
                    }
                    return Ok(v.clone());
                }
            }
            // Default success for unknown set commands in restore/apply happy paths.
            if key.contains("-set") {
                return Ok(String::new());
            }
            Err(ProxySysError::ApplyFailed(format!("unexpected: {key}")))
        }
    }

    fn seed_wifi_disabled_proxy(runner: &MockRunner) {
        runner.set(
            &["-getwebproxy", "Wi-Fi"],
            "Enabled: No\nServer:\nPort: 0\n",
        );
        runner.set(
            &["-getsecurewebproxy", "Wi-Fi"],
            "Enabled: No\nServer:\nPort: 0\n",
        );
        runner.set(
            &["-getsocksfirewallproxy", "Wi-Fi"],
            "Enabled: No\nServer:\nPort: 0\n",
        );
        runner.set(&["-getproxybypassdomains", "Wi-Fi"], "Empty\n");
    }

    fn seed_wifi_backup(runner: &MockRunner) {
        runner.set(
            &["-listallnetworkservices"],
            "An asterisk (*) denotes that a network service is disabled.\nWi-Fi\n",
        );
        runner.set(
            &["-getwebproxy", "Wi-Fi"],
            "Enabled: Yes\nServer: 10.0.0.1\nPort: 8080\nAuthenticated Proxy Enabled: 0\n",
        );
        runner.set(
            &["-getsecurewebproxy", "Wi-Fi"],
            "Enabled: Yes\nServer: 10.0.0.1\nPort: 8080\nAuthenticated Proxy Enabled: 0\n",
        );
        runner.set(
            &["-getsocksfirewallproxy", "Wi-Fi"],
            "Enabled: No\nServer:\nPort: 0\nAuthenticated Proxy Enabled: 0\n",
        );
        runner.set(&["-getproxybypassdomains", "Wi-Fi"], "localhost\n*.local\n");
    }

    #[test]
    fn parse_proxy_get_output_reads_fields() {
        let s = parse_proxy_get_output(
            "Enabled: Yes\nServer: 127.0.0.1\nPort: 17890\nAuthenticated Proxy Enabled: 0\n",
        );
        assert!(s.enabled);
        assert_eq!(s.server, "127.0.0.1");
        assert_eq!(s.port, 17890);
    }

    #[test]
    fn backup_captures_services_in_extra() {
        let runner = MockRunner::default();
        seed_wifi_backup(&runner);
        let proxy = MacosSystemProxy::with_runner(runner);
        let backup = proxy.backup().expect("backup");
        assert!(backup.enabled);
        assert_eq!(backup.http.as_deref(), Some("10.0.0.1:8080"));
        let services = backup.extra.get("services").unwrap().as_array().unwrap();
        assert_eq!(services.len(), 1);
        assert_eq!(services[0]["name"], "Wi-Fi");
        assert_eq!(services[0]["web"]["port"], 8080);
        assert_eq!(services[0]["bypass"][0], "localhost");
    }

    #[test]
    fn g4_5_apply_failure_returns_apply_failed() {
        let runner = MockRunner::default();
        runner.set(
            &["-listallnetworkservices"],
            "An asterisk (*) denotes that a network service is disabled.\nWi-Fi\n",
        );
        seed_wifi_disabled_proxy(&runner);
        runner.set_err(
            &["-setwebproxy", "Wi-Fi", "127.0.0.1", "17890"],
            "permission denied",
        );
        let proxy = MacosSystemProxy::with_runner(runner);
        let err = proxy
            .apply(&ProxyEndpoints {
                http_host: "127.0.0.1".into(),
                http_port: 17890,
                socks_host: None,
                socks_port: None,
            })
            .expect_err("apply");
        assert!(matches!(err, ProxySysError::ApplyFailed(_)), "got {err:?}");
        assert!(err.to_string().contains("permission denied"));
    }

    #[test]
    fn apply_partial_failure_rolls_back_prior_services() {
        let runner = MockRunner::default();
        runner.set(&["-listallnetworkservices"], "Wi-Fi\nEthernet\n");
        runner.set(
            &["-getwebproxy", "Wi-Fi"],
            "Enabled: Yes\nServer: 10.0.0.1\nPort: 8080\n",
        );
        runner.set(
            &["-getsecurewebproxy", "Wi-Fi"],
            "Enabled: No\nServer:\nPort: 0\n",
        );
        runner.set(
            &["-getsocksfirewallproxy", "Wi-Fi"],
            "Enabled: No\nServer:\nPort: 0\n",
        );
        runner.set(&["-getproxybypassdomains", "Wi-Fi"], "localhost\n");
        runner.set(
            &["-getwebproxy", "Ethernet"],
            "Enabled: No\nServer:\nPort: 0\n",
        );
        runner.set(
            &["-getsecurewebproxy", "Ethernet"],
            "Enabled: No\nServer:\nPort: 0\n",
        );
        runner.set(
            &["-getsocksfirewallproxy", "Ethernet"],
            "Enabled: No\nServer:\nPort: 0\n",
        );
        runner.set(&["-getproxybypassdomains", "Ethernet"], "Empty\n");
        runner.set_err(
            &["-setwebproxy", "Ethernet", "127.0.0.1", "17890"],
            "permission denied",
        );

        let calls = runner.calls.clone();
        let proxy = MacosSystemProxy::with_runner(runner);
        let err = proxy
            .apply(&ProxyEndpoints {
                http_host: "127.0.0.1".into(),
                http_port: 17890,
                socks_host: None,
                socks_port: None,
            })
            .expect_err("apply");
        assert!(err.to_string().contains("permission denied"));

        let calls = calls.lock().unwrap();
        assert!(
            calls.iter().any(|c| {
                c.starts_with("-setwebproxy Wi-Fi 10.0.0.1 8080")
                    || c.contains("-setwebproxy Wi-Fi 10.0.0.1 8080")
            }),
            "expected Wi-Fi restore after partial apply, calls: {calls:?}"
        );
    }

    #[test]
    fn apply_bypass_failure_rolls_back_current_service() {
        let runner = MockRunner::default();
        runner.set(&["-listallnetworkservices"], "Wi-Fi\n");
        seed_wifi_disabled_proxy(&runner);
        runner.set_err(&["-setproxybypassdomains", "Wi-Fi"], "permission denied");

        let calls = runner.calls.clone();
        let proxy = MacosSystemProxy::with_runner(runner);
        let err = proxy
            .apply(&ProxyEndpoints {
                http_host: "127.0.0.1".into(),
                http_port: 17890,
                socks_host: None,
                socks_port: None,
            })
            .expect_err("apply");
        assert!(err.to_string().contains("permission denied"));

        let calls = calls.lock().unwrap();
        assert!(
            calls
                .iter()
                .any(|c| c.contains("-setwebproxystate Wi-Fi off")),
            "expected current service rollback after bypass failure, calls: {calls:?}"
        );
    }

    #[test]
    fn restore_skips_services_that_no_longer_exist() {
        let runner = MockRunner::default();
        runner.set(&["-listallnetworkservices"], "Wi-Fi\n");
        let calls = runner.calls.clone();
        let proxy = MacosSystemProxy::with_runner(runner);

        let backup = ProxyBackup {
            enabled: true,
            http: Some("10.0.0.1:8080".into()),
            https: None,
            socks: None,
            extra: serde_json::json!({
                "services": [
                    {
                        "name": "Wi-Fi",
                        "web": {"enabled": true, "server": "10.0.0.1", "port": 8080},
                        "secure_web": {"enabled": true, "server": "10.0.0.1", "port": 8080},
                        "socks": {"enabled": false, "server": "", "port": 0},
                        "bypass": ["localhost"]
                    },
                    {
                        "name": "Ethernet",
                        "web": {"enabled": false, "server": "", "port": 0},
                        "secure_web": {"enabled": false, "server": "", "port": 0},
                        "socks": {"enabled": false, "server": "", "port": 0},
                        "bypass": []
                    }
                ]
            }),
        };

        proxy
            .restore(&backup)
            .expect("restore skips missing service");
        let calls = calls.lock().unwrap();
        assert!(
            calls
                .iter()
                .any(|c| c.starts_with("-setwebproxy Wi-Fi 10.0.0.1 8080")),
            "existing service must be restored, calls: {calls:?}"
        );
        assert!(
            !calls.iter().any(|c| c.contains("Ethernet")),
            "deleted service must be skipped, calls: {calls:?}"
        );
    }

    #[test]
    fn apply_sets_web_secure_socks_and_bypass() {
        let runner = MockRunner::default();
        runner.set(&["-listallnetworkservices"], "Wi-Fi\n");
        seed_wifi_disabled_proxy(&runner);
        let calls = runner.calls.clone();
        let proxy = MacosSystemProxy::with_runner(runner);
        proxy
            .apply(&ProxyEndpoints {
                http_host: "127.0.0.1".into(),
                http_port: 17890,
                socks_host: Some("127.0.0.1".into()),
                socks_port: Some(17890),
            })
            .expect("apply");
        let calls = calls.lock().unwrap();
        assert!(calls.iter().any(|c| c.contains("-setwebproxy")));
        assert!(calls.iter().any(|c| c.contains("-setsecurewebproxy")));
        assert!(calls.iter().any(|c| c.contains("-setsocksfirewallproxy")));
        assert!(calls.iter().any(|c| c.contains("-setproxybypassdomains")
            && c.contains("localhost")
            && c.contains("127.0.0.1")
            && c.contains("::1")));
        let list_calls = calls
            .iter()
            .filter(|c| *c == "-listallnetworkservices")
            .count();
        assert_eq!(
            list_calls, 1,
            "apply must not re-list services via backup(), calls: {calls:?}"
        );
    }
}
