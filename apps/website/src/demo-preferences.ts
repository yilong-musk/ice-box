// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * Bridges same-origin `localStorage` changes into the events the embedded app
 * already listens to. A `storage` event never reaches the document that wrote
 * the value, so the landing page (and the other demo document) only reach an
 * iframe this way.
 */
import { LANGUAGE_CHANGE_EVENT } from "../../desktop/src/lib/i18n";
import {
  LANGUAGE_STORAGE_KEY,
  isLanguagePreference,
} from "../../desktop/src/lib/language";
import {
  THEME_CHANGE_EVENT,
  THEME_STORAGE_KEY,
  isThemePreference,
} from "../../desktop/src/lib/theme";

export function bridgeStoredPreferences(): void {
  window.addEventListener("storage", (event) => {
    if (event.key === LANGUAGE_STORAGE_KEY) {
      if (!isLanguagePreference(event.newValue)) return;
      window.dispatchEvent(
        new CustomEvent(LANGUAGE_CHANGE_EVENT, { detail: event.newValue }),
      );
      return;
    }
    if (event.key === THEME_STORAGE_KEY) {
      if (!isThemePreference(event.newValue)) return;
      window.dispatchEvent(
        new CustomEvent(THEME_CHANGE_EVENT, { detail: event.newValue }),
      );
    }
  });
}
