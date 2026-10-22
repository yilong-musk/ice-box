// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * README screenshot fixtures: the Live Demo browser mock with the phone-only
 * state (VPN tunnel, Android app list, per-app routing) applied by
 * `apps/website/src/phone-api.ts`. `vite build --mode capture` aliases
 * `@platform/api` here; the regular mobile build keeps using the Tauri
 * transport in `mobile-api.ts`.
 */
import type { ApiContract } from "../../desktop/src/api/contracts";
import { api as browserApi } from "../../website/src/browser-api";
import { createPhoneApi } from "../../website/src/phone-api";

export const api: ApiContract = createPhoneApi(browserApi);
