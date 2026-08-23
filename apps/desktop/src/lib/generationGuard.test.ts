import { describe, expect, it } from "vitest";
import {
  formatListenValidationError,
  formatPortValidationError,
  formatPortsConflictError,
  isLoopbackListenHost,
  parsePortInput,
  portsConflict,
} from "./generationGuard";
import { formatUpdateFailures } from "./subscriptions";

describe("generationGuard", () => {
  it("parsePortInput accepts valid ports", () => {
    expect(parsePortInput("17890")).toBe(17890);
    expect(parsePortInput("1024")).toBe(1024);
    expect(parsePortInput("65535")).toBe(65535);
  });

  it("parsePortInput rejects out of range and empty", () => {
    expect(parsePortInput("")).toBeUndefined();
    expect(parsePortInput("0")).toBeUndefined();
    expect(parsePortInput("1023")).toBeUndefined();
    expect(parsePortInput("99999")).toBeUndefined();
    expect(parsePortInput("abc")).toBeUndefined();
  });

  it("formatPortValidationError is readable", () => {
    expect(formatPortValidationError("Mixed 端口")).toContain("1024");
  });

  it("isLoopbackListenHost accepts backend loopback hosts", () => {
    expect(isLoopbackListenHost("127.0.0.1")).toBe(true);
    expect(isLoopbackListenHost("localhost")).toBe(true);
    expect(isLoopbackListenHost("[::1]")).toBe(true);
  });

  it("isLoopbackListenHost rejects non-loopback listens", () => {
    expect(isLoopbackListenHost("0.0.0.0")).toBe(false);
    expect(isLoopbackListenHost("192.168.1.1")).toBe(false);
    expect(isLoopbackListenHost("::")).toBe(false);
  });

  it("formatListenValidationError is readable", () => {
    expect(formatListenValidationError("Mixed 监听")).toContain("loopback");
  });

  it("portsConflict matches backend port equality rule", () => {
    expect(portsConflict(17890, 17890)).toBe(true);
    expect(portsConflict(17890, 19090)).toBe(false);
    expect(formatPortsConflictError()).toContain("不能相同");
  });
});

describe("formatUpdateFailures", () => {
  it("returns null when all ok", () => {
    expect(
      formatUpdateFailures([
        { id: "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee", ok: true },
      ]),
    ).toBeNull();
  });

  it("lists failed subscription ids", () => {
    const msg = formatUpdateFailures([
      {
        id: "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
        ok: false,
        error: "fetch failed",
      },
    ]);
    expect(msg).toContain("fetch failed");
    expect(msg).toContain("aaaaaaaa");
  });
});
