// SPDX-License-Identifier: GPL-3.0-or-later

import { useCallback, useEffect, useRef, useState } from "react";
import type { KeyboardEvent } from "react";
import { CircleAlert } from "lucide-react";

import { cn } from "@/lib/utils";

const TOAST_DURATION_MS = 5000;

export type ErrorToastState = { id: number; message: string } | null;

/** Single transient page-level error, replaced by the next one. */
export function useErrorToast() {
  const [toast, setToast] = useState<ErrorToastState>(null);
  const nextId = useRef(0);
  const timer = useRef<number | null>(null);

  const clearTimer = useCallback(() => {
    if (timer.current !== null) {
      window.clearTimeout(timer.current);
      timer.current = null;
    }
  }, []);

  const dismiss = useCallback(() => {
    clearTimer();
    setToast(null);
  }, [clearTimer]);

  const show = useCallback(
    (message: string) => {
      const text = message.trim();
      if (!text) return;
      clearTimer();
      nextId.current += 1;
      setToast({ id: nextId.current, message: text });
      timer.current = window.setTimeout(() => {
        timer.current = null;
        setToast(null);
      }, TOAST_DURATION_MS);
    },
    [clearTimer],
  );

  useEffect(() => {
    return () => clearTimer();
  }, [clearTimer]);

  return { toast, show, dismiss };
}

type Props = {
  toast: ErrorToastState;
  onDismiss: () => void;
  className?: string;
};

/** Floating capsule for transient errors that have no row of their own. */
export function ErrorToast({ toast, onDismiss, className }: Props) {
  if (!toast) return null;

  function handleKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key !== "Enter" && event.key !== " " && event.key !== "Escape") {
      return;
    }
    event.preventDefault();
    onDismiss();
  }

  return (
    <div
      key={toast.id}
      role="alert"
      aria-live="assertive"
      data-testid="error-toast"
      title={toast.message}
      tabIndex={0}
      onClick={onDismiss}
      onKeyDown={handleKeyDown}
      className={cn(
        "absolute bottom-4 left-1/2 z-20 flex max-w-[min(90%,36rem)] -translate-x-1/2 cursor-pointer animate-in fade-in-0 slide-in-from-bottom-2 items-center gap-1.5 rounded-full bg-destructive px-4 py-1.5 text-xs font-medium text-background shadow-lg duration-200",
        className,
      )}
    >
      <CircleAlert className="size-3 shrink-0" aria-hidden="true" />
      <span className="truncate">{toast.message}</span>
    </div>
  );
}
