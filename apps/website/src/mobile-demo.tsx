// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * Mobile Live Demo entry: the real phone-first UI against the same simulated
 * backend as the desktop demo. The phone shell is chosen by
 * `<html data-shell="phone">` in `mobile-demo.html`.
 *
 * Phone-only fixtures (VPN permission, tunnel, installed apps, per-app routing)
 * live in `./phone-api`, shared with the README capture mode.
 */
import React from "react";
import { createRoot } from "react-dom/client";
import App from "../../desktop/src/App";
import { api } from "./browser-api";
import { bridgeStoredPreferences } from "./demo-preferences";
import { applyPhoneFixtures } from "./phone-api";
import { applyFlagEmojiPolyfill } from "../../desktop/src/lib/flagEmoji";
import { applyStoredLanguage } from "../../desktop/src/lib/i18n";
import { applyStoredTheme } from "../../desktop/src/lib/theme";
import flagFontUrl from "country-flag-emoji-polyfill/dist/TwemojiCountryFlags.woff2?url";
import "../../mobile/src/app.css";
import "./demo-logo.css";

/**
 * The phone frame draws a gesture navigation bar over the bottom of the app,
 * so the tab bar lifts itself with the same inset Android reports.
 */
const NAV_INSET_PX = 24;

/**
 * The phone frame also draws the status bar over the top of the app, as an
 * edge-to-edge Android window has it, so the frosted header extends under
 * it. Same height as `.phone-status-bar` in `phone-demo.css`.
 */
const STATUS_INSET_PX = 32;

applyPhoneFixtures(api);

// The landing page draws both system bars over the iframe; when the page is
// opened on its own there are no bars, so keep the app edge-to-edge.
if (window.self !== window.top) {
  const rootStyle = document.documentElement.style;
  rootStyle.setProperty("--ice-nav-inset", `${NAV_INSET_PX}px`);
  rootStyle.setProperty("--ice-status-inset", `${STATUS_INSET_PX}px`);
}

bridgeStoredPreferences();

applyStoredTheme();
applyStoredLanguage();
applyFlagEmojiPolyfill(flagFontUrl);

createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
