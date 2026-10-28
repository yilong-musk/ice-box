// SPDX-License-Identifier: GPL-3.0-or-later

import { useLayoutEffect, useState } from "react";
import { Dialog as DialogPrimitive } from "radix-ui";

import type { RuleRow } from "../api/client";
import { Button } from "@/components/ui/button";
import { Field, FieldLabel } from "@/components/ui/field";
import { t, useLanguagePreference } from "../lib/i18n";

type RuleJsonDialogProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  busy: boolean;
  /** Custom rule being edited; null while the dialog is closed. */
  editing: RuleRow | null;
  /** Throws with a user-facing message when the update fails. */
  onSave: (
    fingerprint: string,
    rule: Record<string, unknown>,
  ) => Promise<void>;
};

/**
 * Raw JSON editor for custom rules the interactive form cannot represent.
 * Used for MCP-created / hand-edited rules whose matcher is not in the form.
 */
function RuleJsonDialog({
  open,
  onOpenChange,
  busy,
  editing,
  onSave,
}: RuleJsonDialogProps) {
  useLanguagePreference();
  const [text, setText] = useState("");
  const [error, setError] = useState<string | null>(null);

  useLayoutEffect(() => {
    if (!open || !editing) return;
    setText(JSON.stringify(editing.rule, null, 2));
    setError(null);
  }, [open, editing]);

  async function handleSave() {
    if (!editing) return;
    setError(null);
    let parsed: unknown;
    try {
      parsed = JSON.parse(text);
    } catch (e) {
      setError(
        t("ruleJson.invalid", {
          detail: e instanceof Error ? e.message : String(e),
        }),
      );
      return;
    }
    if (parsed === null || typeof parsed !== "object" || Array.isArray(parsed)) {
      setError(t("ruleJson.object"));
      return;
    }
    try {
      await onSave(editing.fingerprint, parsed as Record<string, unknown>);
      onOpenChange(false);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }

  return (
    <DialogPrimitive.Root open={open} onOpenChange={onOpenChange}>
      <DialogPrimitive.Portal>
        <DialogPrimitive.Overlay
          data-slot="dialog-overlay"
          className="fixed inset-0 z-50 bg-black/40 data-open:animate-in data-open:fade-in-0 data-closed:animate-out data-closed:fade-out-0"
        />
        <DialogPrimitive.Content
          data-slot="dialog-content"
          className="fixed left-1/2 top-1/2 z-50 grid w-full max-w-lg -translate-x-1/2 -translate-y-1/2 gap-4 rounded-xl border border-border bg-background p-5 shadow-lg outline-none data-open:animate-in data-open:fade-in-0 data-open:zoom-in-95 data-closed:animate-out data-closed:fade-out-0 data-closed:zoom-out-95"
        >
          <div className="flex flex-col gap-1.5">
            <DialogPrimitive.Title
              data-slot="dialog-title"
              className="font-heading text-sm font-medium"
            >
              {t("ruleJson.title")}
            </DialogPrimitive.Title>
            <DialogPrimitive.Description
              data-slot="dialog-description"
              className="text-sm text-muted-foreground"
            >
              {t("ruleJson.desc")}
            </DialogPrimitive.Description>
          </div>
          <Field>
            <FieldLabel htmlFor="rule-json">{t("ruleJson.label")}</FieldLabel>
            <textarea
              id="rule-json"
              aria-label={t("ruleJson.label")}
              value={text}
              onChange={(e) => {
                setText(e.target.value);
                setError(null);
              }}
              spellCheck={false}
              className="min-h-64 w-full resize-y rounded-md border border-border bg-background p-3 font-mono text-xs leading-relaxed outline-none focus-visible:border-ring focus-visible:ring-1 focus-visible:ring-ring/50"
            />
          </Field>
          {error ? (
            <p className="text-xs text-destructive" role="alert">
              {error}
            </p>
          ) : null}
          <div className="flex flex-col-reverse justify-end gap-2 sm:flex-row sm:justify-end">
            <Button
              type="button"
              size="sm"
              variant="outline"
              disabled={busy}
              onClick={() => onOpenChange(false)}
            >
              {t("common.cancel")}
            </Button>
            <Button
              type="button"
              size="sm"
              disabled={busy || !text.trim()}
              onClick={() => void handleSave()}
            >
              {t("ruleForm.save")}
            </Button>
          </div>
        </DialogPrimitive.Content>
      </DialogPrimitive.Portal>
    </DialogPrimitive.Root>
  );
}

export { RuleJsonDialog };
