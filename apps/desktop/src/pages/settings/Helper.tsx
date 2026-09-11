// SPDX-License-Identifier: GPL-3.0-or-later

import { Button } from "@/components/ui/button";
import { t } from "../../lib/i18n";
import type { StatusResponse } from "../../api/tauri";

export function HelperActions({
  status,
  busy,
  onInstall,
  onUninstall,
}: {
  status: StatusResponse | null;
  busy: boolean;
  onInstall: () => void;
  onUninstall: () => void;
}) {
  if (status?.helper_supported !== true) return null;
  return (
    <div className="flex flex-wrap gap-2">
      <Button
        type="button"
        size="sm"
        onClick={onInstall}
        disabled={
          busy ||
          (status?.helper_installed === true && status?.helper_stale !== true) ||
          status?.tun_status === "preparing" ||
          status?.tun_status === "stopping" ||
          status?.traffic_capture === "tun"
        }
      >
        {status?.helper_stale === true
          ? t("settings.updateHelper")
          : t("settings.installHelper")}
      </Button>
      <Button
        type="button"
        size="sm"
        variant="outline"
        onClick={onUninstall}
        disabled={
          busy ||
          status?.helper_installed !== true ||
          status?.tun_status === "preparing" ||
          status?.tun_status === "stopping" ||
          status?.traffic_capture === "tun"
        }
      >
        {t("settings.uninstallHelper")}
      </Button>
    </div>
  );
}
