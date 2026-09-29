// SPDX-License-Identifier: GPL-3.0-or-later

/** The build composition root selects the only platform-specific dependency. */
import { api as platformApi } from "@platform/api";
import type { ApiContract } from "./contracts";

export type * from "./contracts";
export { formatDiagnostic, formatInvokeError, formatUiMessage } from "./format";
export const api: ApiContract = platformApi;
