// SPDX-License-Identifier: GPL-3.0-or-later

import { describe, expect, it } from "vitest";
import {
  isSafeShellProxyHost,
  resolveShellProxyEndpoint,
  shellProxyHost,
} from "./shellProxy";

describe("shellProxyHost", () => {
  it("maps unspecified listens to loopback", () => {
    expect(shellProxyHost("0.0.0.0")).toBe("127.0.0.1");
    expect(shellProxyHost("::")).toBe("127.0.0.1");
    expect(shellProxyHost("")).toBe("127.0.0.1");
  });

  it("keeps loopback and unicast listens", () => {
    expect(shellProxyHost("127.0.0.1")).toBe("127.0.0.1");
    expect(shellProxyHost("::1")).toBe("::1");
    expect(shellProxyHost("192.168.1.8")).toBe("192.168.1.8");
  });
});

describe("isSafeShellProxyHost", () => {
  it("accepts IPv4, bracketed IPv6, and hostnames", () => {
    for (const host of [
      "127.0.0.1",
      "::1",
      "[::1]",
      "localhost",
      "my-host.local",
      "node_1",
    ]) {
      expect(isSafeShellProxyHost(host), host).toBe(true);
    }
  });

  it("rejects hosts that could inject shell syntax", () => {
    for (const host of [
      "",
      "127.0.0.1; rm -rf ~",
      "127.0.0.1 && touch /tmp/pwned",
      "host$(id)",
      "host`id`",
      'host"x',
      "host'x",
      "host|cat",
      "host\nx",
      "host x",
      "localhost:17890/share",
      "a".repeat(256),
    ]) {
      expect(isSafeShellProxyHost(host), host).toBe(false);
    }
    expect(isSafeShellProxyHost("a".repeat(255))).toBe(true);
  });
});

describe("resolveShellProxyEndpoint", () => {
  it("prefers the live inbound and rewrites unspecified hosts", () => {
    expect(
      resolveShellProxyEndpoint(
        { mixed_listen: "0.0.0.0", mixed_port: 18080 },
        { host: "0.0.0.0", port: 17890 },
      ),
    ).toEqual({ host: "127.0.0.1", port: 17890 });
  });

  it("falls back to settings when inbound is missing", () => {
    expect(
      resolveShellProxyEndpoint(
        { mixed_listen: "127.0.0.1", mixed_port: 17890 },
        { host: null, port: null },
      ),
    ).toEqual({ host: "127.0.0.1", port: 17890 });
  });

  it("returns null without a port", () => {
    expect(resolveShellProxyEndpoint(null, { host: null, port: null })).toBeNull();
  });

  it("returns null for a host that cannot be embedded in a shell command", () => {
    // Allow LAN skips `mixed_listen` validation, so an arbitrary string must
    // hide the controls rather than reach a generated command.
    expect(
      resolveShellProxyEndpoint(
        { mixed_listen: "127.0.0.1; touch /tmp/pwned", mixed_port: 17890 },
        { host: null, port: null },
      ),
    ).toBeNull();
    expect(
      resolveShellProxyEndpoint(
        { mixed_listen: "127.0.0.1", mixed_port: 17890 },
        { host: "127.0.0.1; touch /tmp/pwned", port: 17890 },
      ),
    ).toBeNull();
  });
});
