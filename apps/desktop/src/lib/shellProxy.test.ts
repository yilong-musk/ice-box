// SPDX-License-Identifier: GPL-3.0-or-later

import { describe, expect, it, vi } from "vitest";
import {
  copyText,
  detectShellProxyPlatform,
  formatShellProxyCommand,
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

describe("detectShellProxyPlatform", () => {
  it("selects PowerShell on Windows and POSIX elsewhere", () => {
    expect(
      detectShellProxyPlatform(
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64)",
        "Win32",
      ),
    ).toBe("windows");
    expect(
      detectShellProxyPlatform(
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)",
        "MacIntel",
      ),
    ).toBe("posix");
    expect(
      detectShellProxyPlatform("Mozilla/5.0 (X11; Linux x86_64)", "Linux x86_64"),
    ).toBe("posix");
  });
});

describe("formatShellProxyCommand", () => {
  it("exports session variables for POSIX shells", () => {
    const cmd = formatShellProxyCommand("posix", "127.0.0.1", 17890);
    expect(cmd).toBe(
      "export http_proxy=http://127.0.0.1:17890 https_proxy=http://127.0.0.1:17890 all_proxy=socks5://127.0.0.1:17890 no_proxy=localhost,127.0.0.1,::1",
    );
    // Must stay shell-session scoped (no profile / system env persistence).
    expect(cmd).not.toMatch(/bashrc|zshrc|profile|\/etc\/environment|setx/i);
  });

  it("sets PowerShell session variables on Windows", () => {
    const cmd = formatShellProxyCommand("windows", "127.0.0.1", 17890);
    expect(cmd).toBe(
      "$env:HTTP_PROXY='http://127.0.0.1:17890'; $env:HTTPS_PROXY='http://127.0.0.1:17890'; $env:ALL_PROXY='socks5://127.0.0.1:17890'; $env:NO_PROXY='localhost,127.0.0.1,::1'",
    );
    // Must stay process-scoped (no User/Machine registry persistence).
    expect(cmd).not.toMatch(
      /setx|SetEnvironmentVariable|\[Environment\]|Machine|User/i,
    );
  });

  it("brackets IPv6 hosts in the proxy URLs", () => {
    expect(formatShellProxyCommand("posix", "::1", 17890)).toContain(
      "http://[::1]:17890",
    );
    expect(formatShellProxyCommand("windows", "::1", 17890)).toContain(
      "socks5://[::1]:17890",
    );
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
});

describe("copyText", () => {
  it("writes through the clipboard API when available", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal("navigator", { clipboard: { writeText } });
    await expect(copyText("export http_proxy=x")).resolves.toBe(true);
    expect(writeText).toHaveBeenCalledWith("export http_proxy=x");
    vi.unstubAllGlobals();
  });
});
