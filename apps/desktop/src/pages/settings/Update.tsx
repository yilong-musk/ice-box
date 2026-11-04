// SPDX-License-Identifier: GPL-3.0-or-later

import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { formatProgress } from "../../components/UpdateAvailableDialog";
import { APP_VERSION } from "../../lib/appVersion";
import { t } from "../../lib/i18n";
import { isErrorCode } from "../../api/errorCodes";
import { formatInvokeError, type CheckAppUpdateResponse } from "../../api/client";
import { isPhoneShell } from "@platform/shell";
import { SettingsRow, SettingsSection } from "./SettingsLayout";

function invokeErrorCode(err: unknown): string | null {
  if (err && typeof err === "object") {
    const code = (err as { code?: unknown }).code;
    if (typeof code === "string" && code.length > 0) {
      return code;
    }
  }
  if (typeof err === "string") {
    const sep = err.indexOf(":");
    const code = (sep === -1 ? err : err.slice(0, sep)).trim();
    // Only treat the prefix as a code when it is a known IPC code so a
    // mention of `update.feed_unavailable` in a free-form message cannot
    // steal the catalog copy.
    if (isErrorCode(code)) {
      return code;
    }
  }
  return null;
}

/** Map updater IPC failures to the Settings-specific copy. Looks at the
 * `{ code, message }` payload first; splitting a pre-formatted
 * `formatInvokeError` string on `:` would miss that path. */
export function formatUpdateError(err: unknown): string {
  const code = invokeErrorCode(err);
  if (code === "update.feed_unavailable") {
    return t("settings.updateFeedUnavailable");
  }
  if (code === "update.check_failed") {
    return t(
      isPhoneShell()
        ? "settings.updateCheckFailedPhone"
        : "settings.updateCheckFailed",
    );
  }
  return formatInvokeError(err);
}

export function UpdateCard({
  checkAppUpdates,
  busy,
  loaded,
  updateBusy,
  updateInfo,
  updateProgress,
  updateError,
  onCheckAppUpdatesChange,
  onCheck,
  onInstall,
  onDownload,
}: {
  checkAppUpdates: boolean;
  busy: boolean;
  loaded: boolean;
  updateBusy: boolean;
  updateInfo: CheckAppUpdateResponse | null;
  updateProgress: { downloaded: number; contentLength: number | null } | null;
  updateError: string | null;
  onCheckAppUpdatesChange: (enabled: boolean) => void;
  onCheck: () => void;
  onInstall: () => void;
  /** Optional browser download. The phone shell installs the APK itself. */
  onDownload?: () => void;
}) {
  const download = onDownload != null;
  const statusLine =
    !download && updateBusy && updateProgress ? (
      formatProgress(updateProgress.downloaded, updateProgress.contentLength)
    ) : updateInfo?.available && updateInfo.version ? (
      <span className="font-medium text-foreground">
        {t("settings.updateAvailable", { version: updateInfo.version })}
      </span>
    ) : updateInfo && !updateInfo.available ? (
      t("settings.updateUpToDate")
    ) : null;

  return (
    <SettingsSection title={t("settings.update")}>
      <SettingsRow
        label={t("settings.updateAutoCheck")}
        htmlFor="settings-check-app-updates"
      >
        <Switch
          id="settings-check-app-updates"
          checked={checkAppUpdates}
          disabled={busy || !loaded}
          aria-label={t("settings.updateAutoCheck")}
          onCheckedChange={(checked) => {
            onCheckAppUpdatesChange(checked === true);
          }}
        />
      </SettingsRow>
      <SettingsRow
        label={t("settings.updateCurrent", { version: APP_VERSION })}
        description={statusLine}
        error={updateError}
      >
        <Button
          type="button"
          size="sm"
          variant="outline"
          disabled={busy || updateBusy || !loaded}
          onClick={onCheck}
        >
          {t("settings.updateCheck")}
        </Button>
        {updateInfo?.available ? (
          <Button
            type="button"
            size="sm"
            disabled={busy || updateBusy || !loaded}
            onClick={download ? onDownload : onInstall}
          >
            {t(download ? "settings.updateDownload" : "settings.updateInstall")}
          </Button>
        ) : null}
      </SettingsRow>
    </SettingsSection>
  );
}
