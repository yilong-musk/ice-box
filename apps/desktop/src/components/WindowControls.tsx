import { Minus, Square, X } from "lucide-react";
import { detectWindowChrome, runWindowCommand } from "@/lib/windowChrome";
import { t, useLanguagePreference } from "@/lib/i18n";
import { cn } from "@/lib/utils";

const BUTTON_CLASS =
  "flex h-12 w-11 items-center justify-center text-muted-foreground transition-colors hover:bg-muted hover:text-foreground";

/** Windows caption buttons; macOS keeps native traffic lights in the overlay bar. */
export function WindowControls() {
  useLanguagePreference();
  if (detectWindowChrome() !== "windows-custom") return null;

  return (
    <div className="flex h-full shrink-0">
      <button
        type="button"
        className={BUTTON_CLASS}
        aria-label={t("window.minimize")}
        onClick={() => void runWindowCommand("minimize")}
      >
        <Minus className="size-3.5" />
      </button>
      <button
        type="button"
        className={BUTTON_CLASS}
        aria-label={t("window.maximize")}
        onClick={() => void runWindowCommand("toggleMaximize")}
      >
        <Square className="size-3" />
      </button>
      <button
        type="button"
        className={cn(
          BUTTON_CLASS,
          "hover:bg-destructive hover:text-white dark:hover:text-white",
        )}
        aria-label={t("window.close")}
        onClick={() => void runWindowCommand("close")}
      >
        <X className="size-3.5" />
      </button>
    </div>
  );
}
