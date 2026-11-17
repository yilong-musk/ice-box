// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * The subscription auto-update control is one dropdown: "Never" first, then the
 * refresh intervals. It replaces a switch plus a second dropdown, so the off
 * state reads as a frequency choice like the rest and the row stays one line.
 */

import type {
  SubscriptionAutoUpdateInterval,
  SubscriptionMeta,
} from "../api/client";
import { t } from "./i18n";

/** Interval options, in the order the dropdown lists them. */
export const AUTO_UPDATE_INTERVALS: SubscriptionAutoUpdateInterval[] = [
  "one_hour",
  "three_hours",
  "six_hours",
  "twelve_hours",
  "twenty_four_hours",
];

/** Option value for auto-update off. Not an interval: selecting it only clears
 * the flag, and the backend keeps the stored interval. */
export const AUTO_UPDATE_NEVER = "never";

/** Frequency assumed when a stored subscription has none: an entry that
 * predates interval selection or was written with auto-update off. It matches
 * `AutoUpdateInterval::default_duration` in the backend, so the UI and the
 * watchdog agree on when the next refresh is due. Do not fold this into
 * [`IMPORT_AUTO_UPDATE_INTERVAL`]: that one is a product choice for new
 * imports, not a fallback for data that already exists. */
export const DEFAULT_AUTO_UPDATE_INTERVAL: SubscriptionAutoUpdateInterval =
  "one_hour";

/** Frequency preselected in the import dialog, and stored with an import that
 * starts on "Never" so turning auto-update on later has a sensible cadence.
 * Unlike [`DEFAULT_AUTO_UPDATE_INTERVAL`] this never applies to stored
 * subscriptions, only to a new one. */
export const IMPORT_AUTO_UPDATE_INTERVAL: SubscriptionAutoUpdateInterval =
  "twelve_hours";

export type AutoUpdateChoice =
  | { autoUpdate: false }
  | { autoUpdate: true; interval: SubscriptionAutoUpdateInterval };

type AutoUpdateMeta = Pick<
  SubscriptionMeta,
  "auto_update" | "auto_update_interval"
>;

/** Dropdown value for a subscription or an import form. */
export function autoUpdateChoiceValue(meta: AutoUpdateMeta): string {
  if (!meta.auto_update) return AUTO_UPDATE_NEVER;
  return meta.auto_update_interval ?? DEFAULT_AUTO_UPDATE_INTERVAL;
}

/** Parse a dropdown value; `null` for a value the list does not offer. */
export function parseAutoUpdateChoice(value: string): AutoUpdateChoice | null {
  if (value === AUTO_UPDATE_NEVER) return { autoUpdate: false };
  const interval = AUTO_UPDATE_INTERVALS.find((item) => item === value);
  return interval ? { autoUpdate: true, interval } : null;
}

/** The interval to store while auto-update is off, so turning it back on keeps
 * the frequency the user picked before. */
export function retainedAutoUpdateInterval(
  meta: AutoUpdateMeta,
): SubscriptionAutoUpdateInterval {
  return meta.auto_update_interval ?? DEFAULT_AUTO_UPDATE_INTERVAL;
}

/** Label of one interval option. */
export function intervalLabel(
  interval: SubscriptionAutoUpdateInterval,
): string {
  return t(`subs.interval.${interval}`);
}

/** Label of the "off" option. */
export function autoUpdateNeverLabel(): string {
  return t("subs.autoUpdateNever");
}
