import { useCallback, useRef } from "react";
import { t } from "./i18n";

/** Invalidates in-flight async work when generation changes (mutations / tab switches). */
export function useGenerationGuard() {
  const genRef = useRef(0);

  const nextGeneration = useCallback(() => {
    genRef.current += 1;
    return genRef.current;
  }, []);

  const isStale = useCallback((gen: number) => gen !== genRef.current, []);

  return { nextGeneration, isStale };
}

/** Parse port input; returns undefined when empty/invalid for client-side validation. */
export function parsePortInput(raw: string): number | undefined {
  if (raw.trim() === "") return undefined;
  const n = Number(raw);
  if (!Number.isFinite(n) || !Number.isInteger(n)) return undefined;
  if (n < 1024 || n > 65535) return undefined;
  return n;
}

/** Normalize listen host input (strip brackets). */
export function normalizeListenHost(raw: string): string {
  return raw.trim().replace(/^\[|\]$/g, "");
}

/** Matches backend loopback listen validation (`ice-config` settings). */
export function isLoopbackListenHost(raw: string): boolean {
  const host = normalizeListenHost(raw);
  if (host === "0.0.0.0" || host === "::") return false;
  const lower = host.toLowerCase();
  return lower === "127.0.0.1" || lower === "localhost" || lower === "::1";
}

export function formatListenValidationError(field: string): string {
  return `${field} ${t("validation.listen")}`;
}

export function formatPortValidationError(field: string): string {
  return `${field} ${t("validation.port")}`;
}

/** True when mixed and Clash API ports would conflict (matches backend validation). */
export function portsConflict(mixedPort: number, clashApiPort: number): boolean {
  return mixedPort === clashApiPort;
}

export function formatPortsConflictError(): string {
  return t("validation.portsConflict");
}
