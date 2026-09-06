import { t } from "@/lib/i18n";

function formatProgress(downloaded: number, contentLength: number | null): string {
  if (contentLength && contentLength > 0) {
    const pct = Math.min(100, Math.round((downloaded / contentLength) * 100));
    return t("update.progressPct", { pct });
  }
  if (downloaded > 0) {
    return t("update.progressBytes", { bytes: downloaded });
  }
  return t("update.installing");
}

export { formatProgress };
