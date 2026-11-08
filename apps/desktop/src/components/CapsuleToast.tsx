// SPDX-License-Identifier: GPL-3.0-or-later

import { useCallback, useRef } from "react";
import type { KeyboardEvent } from "react";
import { Check, CircleAlert } from "lucide-react";

import { t } from "../lib/i18n";
import { useTransient } from "../lib/transient";
import { cn } from "@/lib/utils";

const ERROR_DURATION_MS = 5000;
const SUCCESS_DURATION_MS = 2400;

type CapsuleToastTone = "error" | "success";

export type CapsuleToastState = {
  id: number;
  message: string;
  tone: CapsuleToastTone;
} | null;

/** Single transient page-level notice, replaced by the next one. Its time keeps
 * running while the page is hidden (see `useTransient`). */
export function useCapsuleToast() {
  const [toast, showToast, dismiss] = useTransient<CapsuleToastState>(null);
  const nextId = useRef(0);

  const show = useCallback(
    (message: string, tone: CapsuleToastTone = "error") => {
      const text = message.trim();
      if (!text) return;
      nextId.current += 1;
      showToast(
        { id: nextId.current, message: text, tone },
        tone === "success" ? SUCCESS_DURATION_MS : ERROR_DURATION_MS,
      );
    },
    [showToast],
  );

  return { toast, show, dismiss };
}

type Props = {
  toast: CapsuleToastState;
  onDismiss: () => void;
  className?: string;
};

/** Floating capsule for transient errors and confirmations that have no row of their own. */
export function CapsuleToast({ toast, onDismiss, className }: Props) {
  const Icon = toast?.tone === "error" ? CircleAlert : Check;

  function handleKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key !== "Enter" && event.key !== " " && event.key !== "Escape") {
      return;
    }
    event.preventDefault();
    onDismiss();
  }

  return (
    <>
      {/* The success live region stays mounted so screen readers announce text
          that arrives after the first render; the capsule itself is purely
          visual. Errors use a fresh `role="alert"`, which is announced on
          insertion. */}
      <div
        role="status"
        aria-live="polite"
        aria-atomic="true"
        data-testid={toast?.tone === "success" ? "success-toast" : undefined}
        className="sr-only"
      >
        {toast?.tone === "success" ? toast.message : ""}
      </div>
      {toast?.tone === "error" ? (
        <div
          role="alert"
          aria-live="assertive"
          aria-atomic="true"
          data-testid="error-toast"
          className="sr-only"
        >
          {toast.message}
        </div>
      ) : null}
      {toast ? (
        <div
          key={toast.id}
          data-testid="capsule-toast"
          role="button"
          aria-label={t("common.dismiss")}
          title={toast.message}
          tabIndex={0}
          onClick={onDismiss}
          onKeyDown={handleKeyDown}
          className={cn(
            "absolute bottom-4 left-1/2 z-20 flex max-w-[min(90%,36rem)] -translate-x-1/2 cursor-pointer animate-in fade-in-0 slide-in-from-bottom-2 items-center gap-1.5 rounded-full px-4 py-1.5 text-xs font-medium text-background shadow-lg duration-200",
            toast.tone === "error" ? "bg-destructive" : "bg-ok",
            className,
          )}
        >
          <Icon className="size-3 shrink-0" aria-hidden="true" />
          <span className="truncate">{toast.message}</span>
        </div>
      ) : null}
    </>
  );
}
