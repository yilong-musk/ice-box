// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * Mobile Live Demo entry: the real phone-first UI against the same simulated
 * backend as the desktop demo. The phone shell is chosen by
 * `<html data-shell="phone">` in `mobile-demo.html`.
 *
 * The browser mock is desktop-shaped in a few places, so this entry adds the
 * phone-only pieces: an already granted VPN permission, a tunnel that follows
 * the mocked core, the installed-app fixtures, and the per-app routing modes.
 * Keep these fixtures in sync with `apps/mobile/src/capture-api.ts`, which
 * feeds the README phone screenshots.
 */
import React from "react";
import { createRoot } from "react-dom/client";
import App from "../../desktop/src/App";
import { api } from "./browser-api";
import type {
  AppProxyMode,
  AppSettings,
  InstalledApp,
  StatusResponse,
} from "../../desktop/src/api/contracts";
import { applyFlagEmojiPolyfill } from "../../desktop/src/lib/flagEmoji";
import {
  LANGUAGE_CHANGE_EVENT,
  applyStoredLanguage,
} from "../../desktop/src/lib/i18n";
import {
  LANGUAGE_STORAGE_KEY,
  isLanguagePreference,
  readLanguagePreference,
} from "../../desktop/src/lib/language";
import { applyStoredTheme } from "../../desktop/src/lib/theme";
import flagFontUrl from "country-flag-emoji-polyfill/dist/TwemojiCountryFlags.woff2?url";
import "../../mobile/src/app.css";

/**
 * The phone frame draws a gesture navigation bar over the bottom of the app,
 * so the tab bar lifts itself with the same inset Android reports.
 */
const NAV_INSET_PX = 24;

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

const browserGetStatus = api.getStatus.bind(api);
const browserGetSettings = api.getSettings.bind(api);

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
  // same way the desktop demo does.
  return {
    ...(await browserGetSettings()),
    language: readLanguagePreference(),
    app_proxies: appProxies,
  };
};

api.listApps = async (): Promise<InstalledApp[]> => apps;

/**
 * The embedded node list mounts before the mocked status arrives and then
 * shows the transient "proxy service not running" warning until its fallback
 * poll. Deliver the state-changed announcements a real backend emits after
 * boot so the list re-reads the now-loaded status right away.
 */
const stateChangedHandlers = new Set<() => void>();

api.listenStateChanged = async (handler: () => void) => {
  stateChangedHandlers.add(handler);
  return () => {
    stateChangedHandlers.delete(handler);
  };
};

function announceStateChanged() {
  for (const handler of stateChangedHandlers) handler();
}

window.setTimeout(announceStateChanged, 600);
window.setTimeout(announceStateChanged, 1500);

// The landing page draws a gesture bar over the bottom of the iframe; when the
// page is opened on its own there is no bar, so keep the app edge-to-edge.
if (window.self !== window.top) {
  document.documentElement.style.setProperty(
    "--ice-nav-inset",
    `${NAV_INSET_PX}px`,
  );
}

// The landing page and this document share `ice-box.language` (same origin).
// A toggle on the site only fires a `storage` event here; re-dispatch it as
// the app's language event so the running React tree re-renders in place.
window.addEventListener("storage", (event) => {
  if (event.key !== LANGUAGE_STORAGE_KEY) return;
  if (!isLanguagePreference(event.newValue)) return;
  window.dispatchEvent(
    new CustomEvent(LANGUAGE_CHANGE_EVENT, { detail: event.newValue }),
  );
});

applyStoredTheme();
applyStoredLanguage();
applyFlagEmojiPolyfill(flagFontUrl);

createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
