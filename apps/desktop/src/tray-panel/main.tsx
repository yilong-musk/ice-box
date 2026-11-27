// SPDX-License-Identifier: GPL-3.0-or-later

import React from "react";
import ReactDOM from "react-dom/client";
import "../index.css";
import "./tray-panel.css";
import { applyFlagEmojiPolyfill } from "../lib/flagEmoji";
import { applyStoredTheme } from "../lib/theme";
import flagFontUrl from "country-flag-emoji-polyfill/dist/TwemojiCountryFlags.woff2?url";
import { TrayPanel } from "./TrayPanel";

applyStoredTheme();
applyFlagEmojiPolyfill(flagFontUrl);

window.addEventListener("contextmenu", (event) => {
  event.preventDefault();
});

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <TrayPanel />
  </React.StrictMode>,
);
