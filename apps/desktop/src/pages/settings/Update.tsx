// SPDX-License-Identifier: GPL-3.0-or-later

import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import {
  Field,
  FieldError,
  FieldGroup,
  FieldLabel,
} from "@/components/ui/field";
import { Switch } from "@/components/ui/switch";
import { formatProgress } from "../../components/UpdateAvailableDialog";
import { APP_VERSION } from "../../lib/appVersion";
import { t } from "../../lib/i18n";
import { isErrorCode } from "../../api/errorCodes";
import { formatInvokeError, type CheckAppUpdateResponse } from "../../api/tauri";

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
    return t("settings.updateCheckFailed");
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
}) {
  return (
    <Card
      size="sm"
      className="w-full shrink-0 data-[size=sm]:[--card-spacing:--spacing(2)]"
    >
      <CardHeader>
        <CardTitle>{t("settings.update")}</CardTitle>
      </CardHeader>
      <CardContent>
        <FieldGroup>
          <Field orientation="horizontal" className="w-auto gap-2">
            <Switch
              id="settings-check-app-updates"
              size="sm"
              checked={checkAppUpdates}
              disabled={busy || !loaded}
              aria-label={t("settings.updateAutoCheck")}
              onCheckedChange={(checked) => {
                onCheckAppUpdatesChange(checked === true);
              }}
            />
            <FieldLabel htmlFor="settings-check-app-updates">
              {t("settings.updateAutoCheck")}
            </FieldLabel>
          </Field>
          <p className="text-xs text-muted-foreground">
            {t("settings.updateCurrent", { version: APP_VERSION })}
          </p>
          {updateInfo?.available && updateInfo.version ? (
            <p className="text-xs">
              {t("settings.updateAvailable", {
                version: updateInfo.version,
              })}
            </p>
          ) : updateInfo && !updateInfo.available ? (
            <p className="text-xs text-muted-foreground">
              {t("settings.updateUpToDate")}
            </p>
          ) : null}
          {updateBusy && updateProgress ? (
            <p className="text-xs text-muted-foreground">
              {formatProgress(
                updateProgress.downloaded,
                updateProgress.contentLength,
              )}
            </p>
          ) : null}
          {updateError ? <FieldError>{updateError}</FieldError> : null}
          <div className="flex flex-wrap gap-2">
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
                onClick={onInstall}
              >
                {t("settings.updateInstall")}
              </Button>
            ) : null}
          </div>
        </FieldGroup>
      </CardContent>
    </Card>
  );
}
