// SPDX-License-Identifier: GPL-3.0-or-later

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
    let rows = parse_netsh_interface_rows(output);
    assert_eq!(rows[2].index, 17);
    assert!(rows[2].up, "connected listing rows must parse as up");
    assert_eq!(rows[2].name, "Wintun");
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
