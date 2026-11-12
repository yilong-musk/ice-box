// SPDX-License-Identifier: GPL-3.0-or-later
import { drawLogo, makeMeta } from "./_switch-logo-small.js";
export const meta = makeMeta(32);
export const draw = (p, t, o) => drawLogo(p, t, o, 32);
