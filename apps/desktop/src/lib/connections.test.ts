// SPDX-License-Identifier: GPL-3.0-or-later

import { afterEach, describe, expect, it } from "vitest";
import type { ConnectionRow } from "../api/contracts";
import {
  CONNECTION_SEARCH_FIELDS,
  CONNECTION_SORT_STORAGE_KEY,
  connectionChain,
  connectionRule,
  connectionRuleDraft,
  connectionTarget,
  formatDuration,
  matchesKeyword,
  processName,
  readConnectionSort,
  sortConnections,
  writeConnectionSort,
} from "./connections";

function row(patch: Partial<ConnectionRow> = {}): ConnectionRow {
  return {
    id: "a",
    network: "tcp",
    inbound: "mixed/mixed-in",
    source: "127.0.0.1:51234",
    host: "api.github.com",
    destination_ip: "",
    destination_port: "443",
    process_path: "/usr/bin/curl",
    upload: 100,
    download: 1000,
    start_ms: 1_000,
    chains: ["node-a", "proxy"],
    rule: "domain_suffix=github.com => route(proxy)",
    ...patch,
  };
}

describe("connection labels", () => {
  it("names the destination by host, then by IP", () => {
    expect(connectionTarget(row())).toBe("api.github.com:443");
    expect(
      connectionTarget(row({ host: "", destination_ip: "1.1.1.1", destination_port: "53" })),
    ).toBe("1.1.1.1:53");
    expect(
      connectionTarget(row({ host: "", destination_ip: "2001:db8::1", destination_port: "53" })),
    ).toBe("[2001:db8::1]:53");
    expect(connectionTarget(row({ destination_port: "" }))).toBe("api.github.com");
    expect(connectionTarget(row({ host: "", destination_port: "" }))).toBe("");
  });

  it("shows the chain from route target to exit node", () => {
    expect(connectionChain(row())).toBe("proxy → node-a");
    expect(connectionChain(row({ chains: ["direct"] }))).toBe("direct");
    expect(connectionChain(row({ chains: [] }))).toBe("");
  });

  it("prefills a custom rule from the destination address type", () => {
    expect(connectionRuleDraft(row())).toEqual({
      matcherKey: "domain_suffix",
      text: "api.github.com",
    });
    expect(
      connectionRuleDraft(row({ host: "API.GitHub.COM." })),
    ).toEqual({ matcherKey: "domain_suffix", text: "api.github.com" });
    // A hostname wins over a resolved IP; the port is not part of the address.
    expect(
      connectionRuleDraft(row({ destination_ip: "1.2.3.4", destination_port: "443" })),
    ).toEqual({ matcherKey: "domain_suffix", text: "api.github.com" });
    expect(
      connectionRuleDraft(row({ host: "1.1.1.1", destination_port: "53" })),
    ).toEqual({ matcherKey: "ip_cidr", text: "1.1.1.1/32" });
    expect(
      connectionRuleDraft(
        row({ host: "", destination_ip: "1.1.1.1", destination_port: "53" }),
      ),
    ).toEqual({ matcherKey: "ip_cidr", text: "1.1.1.1/32" });
    expect(
      connectionRuleDraft(
        row({ host: "", destination_ip: "2001:DB8::1", destination_port: "53" }),
      ),
    ).toEqual({ matcherKey: "ip_cidr", text: "2001:db8::1/128" });
    expect(
      connectionRuleDraft(row({ host: "", destination_ip: "fe80::1%en0" })),
    ).toEqual({ matcherKey: "ip_cidr", text: "fe80::1/128" });
    expect(
      connectionRuleDraft(row({ host: "[::1]", destination_ip: "" })),
    ).toEqual({ matcherKey: "ip_cidr", text: "::1/128" });
    expect(connectionRuleDraft(row({ host: "", destination_ip: "1.1.1.1/24" }))).toEqual({
      matcherKey: "ip_cidr",
      text: "1.1.1.1/24",
    });
    expect(connectionRuleDraft(row({ host: "", destination_ip: "", destination_port: "443" }))).toBe(
      null,
    );
  });

  it("drops the action from the rule", () => {
    expect(connectionRule(row())).toBe("domain_suffix=github.com");
    expect(connectionRule(row({ rule: "final" }))).toBe("final");
  });

  it("reduces a process path to its executable", () => {
    expect(processName("/usr/bin/curl")).toBe("curl");
    expect(processName("C:\\Program Files\\App\\app.exe")).toBe("app.exe");
    expect(processName("/Applications/Safari.app/Contents/MacOS/Safari (alice)")).toBe("Safari");
    expect(processName("curl")).toBe("curl");
    expect(processName("")).toBe("");
  });
});

describe("formatters", () => {
  it("formats durations by magnitude", () => {
    expect(formatDuration(-5)).toBe("0s");
    expect(formatDuration(42_900)).toBe("42s");
    expect(formatDuration(185_000)).toBe("3m 05s");
    expect(formatDuration(3_720_000)).toBe("1h 02m");
    expect(formatDuration(183_600_000)).toBe("2d 03h");
  });
});

describe("matchesKeyword", () => {
  it("searches host, chain, rule and process without case", () => {
    expect(matchesKeyword(row(), "")).toBe(true);
    expect(matchesKeyword(row(), "  GitHub ")).toBe(true);
    expect(matchesKeyword(row(), "node-a")).toBe(true);
    expect(matchesKeyword(row(), "domain_suffix")).toBe(true);
    expect(matchesKeyword(row(), "curl")).toBe(true);
    expect(matchesKeyword(row(), "udp")).toBe(false);
    expect(matchesKeyword(row({ host: "", destination_ip: "1.1.1.1" }), "1.1.1")).toBe(true);
  });

  it("searches every listed field and nothing else", () => {
    for (const field of CONNECTION_SEARCH_FIELDS) {
      const value = field === "chains" ? ["zz-needle"] : "zz-needle";
      expect(matchesKeyword(row({ [field]: value }), "zz-needle"), field).toBe(true);
    }
    expect(matchesKeyword(row({ id: "zz-needle" }), "zz-needle")).toBe(false);
  });
});

describe("sortConnections", () => {
  const rows = [
    row({ id: "old", start_ms: 1_000, upload: 20, download: 9_000 }),
    row({ id: "new", start_ms: 3_000, upload: 700, download: 10 }),
    row({ id: "mid", start_ms: 2_000, upload: 0, download: 500 }),
  ];
  const ids = (sorted: ConnectionRow[]) => sorted.map((r) => r.id);

  it("puts the newest connection first by default", () => {
    expect(ids(sortConnections(rows, "newest"))).toEqual(["new", "mid", "old"]);
  });

  it("orders by upload plus download", () => {
    // Totals: old 9020, new 710, mid 500.
    expect(ids(sortConnections(rows, "total"))).toEqual(["old", "new", "mid"]);
  });

  it("orders by bytes downloaded", () => {
    expect(ids(sortConnections(rows, "download"))).toEqual(["old", "mid", "new"]);
  });

  it("orders by bytes uploaded, ignoring downloads", () => {
    expect(ids(sortConnections(rows, "upload"))).toEqual(["new", "old", "mid"]);
  });

  it("breaks equal start times by id so rows do not swap between polls", () => {
    const tied = [row({ id: "b", start_ms: 5 }), row({ id: "a", start_ms: 5 })];
    expect(ids(sortConnections(tied, "newest"))).toEqual(["a", "b"]);
    expect(ids(sortConnections([...tied].reverse(), "newest"))).toEqual(["a", "b"]);
  });
});

describe("remembered connection sort", () => {
  afterEach(() => {
    window.localStorage.removeItem(CONNECTION_SORT_STORAGE_KEY);
  });

  it("defaults to newest when nothing is stored", () => {
    expect(readConnectionSort()).toBe("newest");
  });

  it("reads back the last written sort", () => {
    writeConnectionSort("upload");
    expect(readConnectionSort()).toBe("upload");
  });

  it("ignores an unknown stored value", () => {
    window.localStorage.setItem(CONNECTION_SORT_STORAGE_KEY, "traffic");
    expect(readConnectionSort()).toBe("newest");
  });
});
