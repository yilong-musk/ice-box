export type WindowChrome = "macos-overlay" | "windows-custom" | "plain";
export type WindowCommand = "minimize" | "toggleMaximize" | "close";

/** Classify the native chrome so the UI can inset traffic lights or draw caption buttons. */
export function detectWindowChrome(
  userAgent = typeof navigator === "undefined" ? "" : navigator.userAgent,
  platform = typeof navigator === "undefined" ? "" : navigator.platform,
): WindowChrome {
  const haystack = `${platform} ${userAgent}`;
  if (/Windows|Win32|Win64/i.test(haystack)) return "windows-custom";
  if (/Mac|iPhone|iPad|darwin/i.test(haystack)) return "macos-overlay";
  return "plain";
}

export async function runWindowCommand(command: WindowCommand): Promise<void> {
  try {
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    const current = getCurrentWindow();
    switch (command) {
      case "minimize":
        await current.minimize();
        return;
      case "toggleMaximize":
        await current.toggleMaximize();
        return;
      case "close":
        await current.close();
        return;
    }
  } catch {
    // Browser preview and unit tests are not inside Tauri.
  }
}
