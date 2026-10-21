// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * README screenshot fixtures: the Live Demo browser mock plus the phone-only
 * state (VPN tunnel, Android app list, per-app routing). `vite build --mode
 * capture` aliases `@platform/api` here; the regular mobile build keeps using
 * the Tauri transport in `mobile-api.ts`.
 */
import type {
  ApiContract,
  AppProxyMode,
  AppSettings,
  InstalledApp,
  StatusResponse,
} from "../../desktop/src/api/contracts";
import { api as browserApi } from "../../website/src/browser-api";
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

const browserGetStatus = browserApi.getStatus.bind(browserApi);
const browserGetSettings = browserApi.getSettings.bind(browserApi);

export const api: ApiContract = {
  ...browserApi,
  async getStatus(): Promise<StatusResponse> {
    const status = await browserGetStatus();
    return {
      ...status,
      // The phone power button follows the tunnel, not the desktop core state.
      vpn_permission: "granted",
      tunnel_status: status.core.status === "running" ? "connected" : "stopped",
    };
  },
  async getSettings(): Promise<AppSettings> {
    // Keep the language the capture context stored; the browser mock pins "en".
    return {
      ...(await browserGetSettings()),
      language: readLanguagePreference(),
      app_proxies: appProxies,
    };
  },
  async listApps(): Promise<InstalledApp[]> {
    return apps;
  },
};
