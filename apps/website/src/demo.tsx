// SPDX-License-Identifier: GPL-3.0-or-later

import React from "react";
import { createRoot } from "react-dom/client";
import App from "../../../apps/desktop/src/App";
import { api } from "./browser-api";
import { bridgeStoredPreferences } from "./demo-preferences";
import { readLanguagePreference } from "../../../apps/desktop/src/lib/language";
import { applyFlagEmojiPolyfill } from "../../../apps/desktop/src/lib/flagEmoji";
import flagFontUrl from "country-flag-emoji-polyfill/dist/TwemojiCountryFlags.woff2?url";
import "./demo.css";

// The mocked settings must not pin the UI language: the desktop app treats
// `settings.language` as authoritative and re-applies it right after mount,
// so the fixture's fixed "en" would override the language the site resolved
// (system locale or the visitor's pick) and write it back to localStorage.
// Mirroring the stored preference keeps the embedded app in sync with the
// landing page, exactly like the real settings file mirrors it.
const mockGetSettings = api.getSettings.bind(api);
api.getSettings = async () => ({
  ...(await mockGetSettings()),
  language: readLanguagePreference(),
});

// Language and theme changes on the landing page (or the phone demo iframe)
// reach this app through the shared storage bridge.
bridgeStoredPreferences();
applyFlagEmojiPolyfill(flagFontUrl);

createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
