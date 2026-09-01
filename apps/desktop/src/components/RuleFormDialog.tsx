import { useEffect, useState } from "react";
import { Dialog as DialogPrimitive } from "radix-ui";

import { api, type NodeInfo } from "../api/tauri";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Field,
  FieldDescription,
  FieldError,
  FieldGroup,
  FieldLabel,
} from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import {
  NativeSelect,
  NativeSelectOption,
} from "@/components/ui/native-select";
import {
  buildCustomRule,
  RULE_MATCHER_DEFS,
  STRATEGY_GROUP_TYPES,
} from "../lib/rules";
import { t, useLanguagePreference } from "../lib/i18n";

type RuleFormDialogProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  busy: boolean;
  /** Throws with a user-facing message when the add fails. */
  onAdd: (rule: Record<string, unknown>) => Promise<void>;
};

/** In-app modal for adding a custom rule (radix Dialog). */
function RuleFormDialog({
  open,
  onOpenChange,
  busy,
  onAdd,
}: RuleFormDialogProps) {
  useLanguagePreference();
  const [matcherKey, setMatcherKey] = useState("domain_suffix");
  const [matcherValue, setMatcherValue] = useState("");
  const [matcherBool, setMatcherBool] = useState(true);
  const [outbound, setOutbound] = useState("direct");
  const [nodeOptions, setNodeOptions] = useState<NodeInfo[]>([]);
  const [customError, setCustomError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    setCustomError(null);
    let cancelled = false;
    api
      .listNodes()
      .then((nodes) => {
        if (!cancelled) setNodeOptions(nodes);
      })
      .catch(() => {
        if (!cancelled) setNodeOptions([]);
      });
    return () => {
      cancelled = true;
    };
  }, [open]);

  const previewDef = RULE_MATCHER_DEFS.find((d) => d.key === matcherKey);
  const previewRule = buildCustomRule(
    matcherKey,
    previewDef?.kind === "boolean" ? matcherBool : matcherValue,
    outbound,
  );

  async function handleAdd() {
    if (!previewRule) return;
    setCustomError(null);
    try {
      await onAdd(previewRule);
      setMatcherValue("");
      setMatcherBool(true);
      onOpenChange(false);
    } catch (e) {
      setCustomError(e instanceof Error ? e.message : String(e));
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
          className="fixed left-1/2 top-1/2 z-50 grid w-full max-w-md -translate-x-1/2 -translate-y-1/2 gap-4 border border-border bg-background p-5 shadow-lg outline-none data-open:animate-in data-open:fade-in-0 data-open:zoom-in-95 data-closed:animate-out data-closed:fade-out-0 data-closed:zoom-out-95"
        >
          <div className="flex flex-col gap-1.5">
            <DialogPrimitive.Title
              data-slot="dialog-title"
              className="font-heading text-sm font-medium"
            >
              {t("ruleForm.title")}
            </DialogPrimitive.Title>
            <DialogPrimitive.Description
              data-slot="dialog-description"
              className="break-all text-sm text-muted-foreground"
            >
              {t("ruleForm.desc")}
            </DialogPrimitive.Description>
          </div>
          <FieldGroup className="flex flex-col gap-2.5">
            <Field>
              <FieldLabel htmlFor="rule-matcher-type">
                {t("ruleForm.matcherType")}
              </FieldLabel>
              <NativeSelect
                id="rule-matcher-type"
                aria-label={t("ruleForm.matcherType")}
                value={matcherKey}
                onChange={(e) => {
                  setMatcherKey(e.target.value);
                  setMatcherValue("");
                  setCustomError(null);
                }}
              >
                {RULE_MATCHER_DEFS.map((d) => (
                  <NativeSelectOption key={d.key} value={d.key}>
                    {t(d.label)}
                  </NativeSelectOption>
                ))}
              </NativeSelect>
            </Field>
            {previewDef?.kind === "boolean" ? (
              <Field>
                <FieldLabel htmlFor="rule-matcher-bool">
                  {t(previewDef.label)}
                </FieldLabel>
                <Field orientation="horizontal">
                  <Checkbox
                    id="rule-matcher-bool"
                    checked={matcherBool}
                    onCheckedChange={(checked) =>
                      setMatcherBool(checked === true)
                    }
                    aria-label={t(previewDef.label)}
                  />
                  <FieldDescription>
                    {t("ruleForm.matchBool", { label: t(previewDef.label) })}
                  </FieldDescription>
                </Field>
              </Field>
            ) : (
              <Field>
                <FieldLabel htmlFor="rule-match-value">
                  {t("ruleForm.matchValue")}
                </FieldLabel>
                <Input
                  id="rule-match-value"
                  type="text"
                  aria-label={t("ruleForm.matchValue")}
                  placeholder={previewDef?.placeholder}
                  value={matcherValue}
                  onChange={(e) => {
                    setMatcherValue(e.target.value);
                    setCustomError(null);
                  }}
                />
              </Field>
            )}
            <Field>
              <FieldLabel htmlFor="rule-outbound">
                {t("ruleForm.outbound")}
              </FieldLabel>
              <NativeSelect
                id="rule-outbound"
                aria-label={t("ruleForm.outbound")}
                value={outbound}
                onChange={(e) => setOutbound(e.target.value)}
              >
                <NativeSelectOption value="direct">
                  {t("ruleForm.directOption")}
                </NativeSelectOption>
                <NativeSelectOption value="block">
                  {t("ruleForm.blockOption")}
                </NativeSelectOption>
                {nodeOptions.map((n) => (
                  <NativeSelectOption key={n.tag} value={n.tag}>
                    {n.tag}
                    {STRATEGY_GROUP_TYPES.includes(n.outbound_type)
                      ? t("ruleForm.strategyGroupSuffix")
                      : ""}
                  </NativeSelectOption>
                ))}
              </NativeSelect>
            </Field>
          </FieldGroup>
          {customError ? <FieldError>{customError}</FieldError> : null}
          {previewRule ? (
            <FieldDescription className="break-all">
              {t("ruleForm.preview")}
              <code>{JSON.stringify(previewRule)}</code>
            </FieldDescription>
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
              disabled={busy || !previewRule}
              onClick={() => void handleAdd()}
            >
              {t("ruleForm.add")}
            </Button>
          </div>
        </DialogPrimitive.Content>
      </DialogPrimitive.Portal>
    </DialogPrimitive.Root>
  );
}

export { RuleFormDialog };