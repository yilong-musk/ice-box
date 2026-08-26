import { Power } from "lucide-react";

import { cn } from "@/lib/utils";

type Props = {
  proxyOn: boolean;
  busy: boolean;
  disabled: boolean;
  ariaLabel: string;
  title: string;
  subtitle: string;
  onClick: () => void;
};

/** Home proxy toggle: large shadcn-style power control. */
export function ProxyPowerButton({
  proxyOn,
  busy,
  disabled,
  ariaLabel,
  title,
  subtitle,
  onClick,
}: Props) {
  return (
    <button
      type="button"
      className={cn(
        "flex w-full max-w-md items-center gap-4 rounded-none border px-4 py-3.5 text-left transition-colors",
        "focus-visible:border-ring focus-visible:ring-1 focus-visible:ring-ring/50 focus-visible:outline-none",
        "disabled:pointer-events-none disabled:opacity-50",
        proxyOn
          ? "border-primary/30 bg-primary text-primary-foreground"
          : "border-border bg-card hover:bg-muted/60",
        busy && "opacity-80",
      )}
      disabled={disabled}
      aria-pressed={proxyOn}
      aria-label={ariaLabel}
      onClick={onClick}
    >
      <span
        className={cn(
          "flex size-11 shrink-0 items-center justify-center rounded-none",
          proxyOn ? "bg-primary-foreground/15" : "bg-muted",
        )}
        aria-hidden="true"
      >
        <Power className="size-5" />
      </span>
      <span className="min-w-0">
        <span className="block text-sm font-medium">{title}</span>
        <span
          className={cn(
            "block text-xs",
            proxyOn ? "text-primary-foreground/80" : "text-muted-foreground",
          )}
        >
          {subtitle}
        </span>
      </span>
    </button>
  );
}
