// SPDX-License-Identifier: GPL-3.0-or-later

import React from "react";
import ReactDOM from "react-dom/client";
import App from "../../desktop/src/App";
import "../../desktop/src/index.css";
import { applyFlagEmojiPolyfill } from "../../desktop/src/lib/flagEmoji";
import { applyStoredTheme } from "../../desktop/src/lib/theme";
import { applyStoredLanguage } from "../../desktop/src/lib/i18n";
import flagFontUrl from "country-flag-emoji-polyfill/dist/TwemojiCountryFlags.woff2?url";

applyStoredTheme();
applyStoredLanguage();
applyFlagEmojiPolyfill(flagFontUrl);

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
