// SPDX-License-Identifier: GPL-3.0-or-later

/** Suggested download name for one subscription's sing-box document. */
export function singboxExportFilename(name: string): string {
  const cleaned = name
    .replace(/[\\/:*?"<>|]/g, "_")
    .replace(/[\u0000-\u001f\u007f]/g, "")
    .trim()
    .replace(/^\.+|\.+$/g, "");
  if (!cleaned) return "sing-box.json";
  return cleaned.toLowerCase().endsWith(".json") ? cleaned : `${cleaned}.json`;
}

/** Browser download. Desktop export uses the native save dialog instead. */
export function saveTextFile(filename: string, text: string): void {
  const blob = new Blob([text], { type: "application/json" });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = filename;
  anchor.rel = "noopener";
  document.body.appendChild(anchor);
  anchor.click();
  anchor.remove();
  URL.revokeObjectURL(url);
}
