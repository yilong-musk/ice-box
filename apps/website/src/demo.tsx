// SPDX-License-Identifier: GPL-3.0-or-later

import React from "react";
import { createRoot } from "react-dom/client";
import App from "../../../apps/desktop/src/App";
import { api } from "./browser-api";
import { LANGUAGE_CHANGE_EVENT } from "../../../apps/desktop/src/lib/i18n";
import {
  LANGUAGE_STORAGE_KEY,
  isLanguagePreference,
  readLanguagePreference,
} from "../../../apps/desktop/src/lib/language";
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

// The landing page and this document share `ice-box.language` (same origin).
// A toggle on the site only fires a `storage` event here; re-dispatch it as
// the app's language event so the running React tree re-renders in place
// (no reload, and the demo session state survives).
window.addEventListener("storage", (event) => {
  if (event.key !== LANGUAGE_STORAGE_KEY) return;
  if (!isLanguagePreference(event.newValue)) return;
  window.dispatchEvent(
    new CustomEvent(LANGUAGE_CHANGE_EVENT, { detail: event.newValue }),
  );
});

createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
