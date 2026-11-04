// SPDX-License-Identifier: GPL-3.0-or-later

import type { ReactNode } from "react";
import { Card, CardHeader, CardTitle } from "@/components/ui/card";
import { FieldError, FieldLabel } from "@/components/ui/field";
import { cn } from "@/lib/utils";

/** One settings group: a small caption above a bordered list of rows.
 *
 * It stays a `Card` (title in `CardTitle`) so tests and smoke scripts can still
 * scope a section by `[data-slot="card"]` / `[data-slot="card-title"]`; only the
 * outer chrome is dropped so the caption sits outside the bordered list. */
export function SettingsSection({
  title,
  children,
  className,
}: {
  title: ReactNode;
  children: ReactNode;
  className?: string;
}) {
  return (
    <Card
      size="sm"
      className={cn(
        "w-full shrink-0 gap-1.5 overflow-visible rounded-none border-0 bg-transparent py-0 shadow-none",
        className,
      )}
    >
      <CardHeader className="px-1">
        <CardTitle className="text-xs font-medium tracking-wide text-muted-foreground">
          {title}
        </CardTitle>
      </CardHeader>
      <div
        data-slot="settings-rows"
        className="flex flex-col divide-y divide-border overflow-hidden rounded-lg border border-border bg-card shadow-xs"
      >
        {children}
      </div>
    </Card>
  );
}

/** A single setting: label and optional hint on the left, the control on the
 * right. On a narrow window the control wraps under the label instead of
 * squeezing it. */
export function SettingsRow({
  label,
  htmlFor,
  description,
  error,
  children,
  className,
}: {
  /** Omit for a row that is only a hint plus an action. */
  label?: ReactNode;
  /** Ties the label to its control so clicking the text toggles it. */
  htmlFor?: string;
  description?: ReactNode;
  error?: ReactNode;
  children?: ReactNode;
  className?: string;
}) {
  return (
    <div
      data-slot="settings-row"
      className={cn(
        "flex min-h-12 flex-wrap items-center justify-between gap-x-4 gap-y-2 px-3.5 py-2.5",
        className,
      )}
    >
      {/* A small floor keeps short labels beside their control on a phone;
          only a label squeezed below it pushes the control onto its own line. */}
      <div className="flex min-w-24 flex-1 flex-col gap-0.5">
        {/* Only a row with a form control gets a <label>; a button-only row
            uses plain text so screen readers do not announce a dangling
            form label. */}
        {label != null && htmlFor ? (
          <FieldLabel htmlFor={htmlFor} className="text-sm font-normal">
            {label}
          </FieldLabel>
        ) : label != null ? (
          <div data-slot="settings-label" className="text-sm leading-snug">
            {label}
          </div>
        ) : null}
        {description != null ? (
          <div className="text-xs/relaxed text-muted-foreground">
            {description}
          </div>
        ) : null}
        {error ? <FieldError className="text-xs">{error}</FieldError> : null}
      </div>
      {children != null ? (
        <div className="ml-auto flex shrink-0 flex-wrap items-center justify-end gap-2">
          {children}
        </div>
      ) : null}
    </div>
  );
}
