// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * Phone-only fixtures shared by the website Live Demo (`mobile-demo.tsx`) and
 * the README capture mode (`apps/mobile/src/capture-api.ts`).
 *
 * The browser mock in `browser-api.ts` is desktop-shaped, so the phone shell
 * layers the Android state on top: an already granted VPN permission, a tunnel
 * that follows the mocked core, the installed-app list, per-app routing, and
 * the state-changed announcements a real backend emits after mutations.
 */
import type {
  ApiContract,
  AppProxyMode,
  AppSettings,
  InstalledApp,
  StatusResponse,
} from "../../desktop/src/api/contracts";
import { readLanguagePreference } from "../../desktop/src/lib/language";

/** One app per routing mode so the Apps page shows every badge. */
const apps: InstalledApp[] = [
  { package_name: "com.google.android.youtube", label: "YouTube", system: false, icon: null },
  { package_name: "com.android.chrome", label: "Chrome", system: true, icon: null },
  { package_name: "org.telegram.messenger", label: "Telegram", system: false, icon: null },
  { package_name: "com.spotify.music", label: "Spotify", system: false, icon: null },
  { package_name: "com.google.android.gm", label: "Gmail", system: true, icon: null },
  { package_name: "com.github.android", label: "GitHub", system: false, icon: null },
  { package_name: "com.netflix.mediaclient", label: "Netflix", system: false, icon: null },
  { package_name: "com.google.android.apps.maps", label: "Maps", system: true, icon: null },
];

const appProxies: Record<string, AppProxyMode> = {
  "com.google.android.youtube": "proxy",
  "com.android.chrome": "proxy",
  "org.telegram.messenger": "proxy",
  "com.google.android.gm": "direct",
  "com.github.android": "block",
};

/**
 * Apply the phone fixtures to an API object in place. The website demo patches
 * the shared `browser-api` singleton, which `App` imports through the
 * `@platform/api` alias, so the overrides must land on that same object.
 */
export function applyPhoneFixtures(api: ApiContract): void {
  const proxies = { ...appProxies };
  const stateChangedHandlers = new Set<() => void>();

  function announceStateChanged() {
    for (const handler of stateChangedHandlers) handler();
  }

  const browserGetStatus = api.getStatus.bind(api);
  const browserGetSettings = api.getSettings.bind(api);
  const browserStart = api.start.bind(api);
  const browserStop = api.stop.bind(api);

  api.getStatus = async (): Promise<StatusResponse> => {
    const status = await browserGetStatus();
    return {
      ...status,
      // The phone power button follows the tunnel, not the desktop core state.
      vpn_permission: "granted",
      tunnel_status: status.core.status === "running" ? "connected" : "stopped",
    };
  };

  api.getSettings = async (): Promise<AppSettings> => {
    // The mock pins the English language; follow the visitor's pick instead, the
    // same way the desktop demo does. Return a copy so callers cannot mutate the
    // fixture map behind `setAppProxy`.
    return {
      ...(await browserGetSettings()),
      language: readLanguagePreference(),
      app_proxies: { ...proxies },
    };
  };

  api.listApps = async (): Promise<InstalledApp[]> =>
    apps.map((app) => ({ ...app }));

  api.setAppProxy = async (
    packageName: string,
    mode: AppProxyMode,
  ): Promise<void> => {
    if (mode === "follow") delete proxies[packageName];
    else proxies[packageName] = mode;
  };

  // The browser mock has no backend events, so synthesize the announcements
  // after the actions that change what the node list and Home read.
  api.listenStateChanged = async (
    handler: () => void,
  ): Promise<() => void> => {
    stateChangedHandlers.add(handler);
    return () => {
      stateChangedHandlers.delete(handler);
    };
  };

  api.start = async (): Promise<void> => {
    await browserStart();
    announceStateChanged();
  };

  api.stop = async (): Promise<void> => {
    await browserStop();
    announceStateChanged();
  };
}

/** Phone fixtures on a copy of `base`, for the README capture API. */
export function createPhoneApi(base: ApiContract): ApiContract {
  const api: ApiContract = { ...base };
  applyPhoneFixtures(api);
  return api;
}
