// SPDX-License-Identifier: GPL-3.0-or-later

import { useEffect, useState } from "react";
import { Dialog as DialogPrimitive } from "radix-ui";

import { api, formatInvokeError, type RuleSetStatus } from "../api/client";
import { ErrorAlert } from "./StatusAlert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Item,
  ItemContent,
  ItemDescription,
  ItemGroup,
  ItemSeparator,
  ItemTitle,
} from "@/components/ui/item";
import { ScrollArea } from "@/components/ui/scroll-area";
import { formatLocale, t, useLanguagePreference } from "../lib/i18n";

type RuleSetStatusDialogProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Status already loaded by the page; refreshed in place by "Update now". */
  statuses: RuleSetStatus[];
  onStatusesChange: (statuses: RuleSetStatus[]) => void;
};

const STATE_LABEL = {
  downloaded: "ruleSets.state.downloaded",
  bundled: "ruleSets.state.bundled",
  pending: "ruleSets.state.pending",
  local: "ruleSets.state.local",
} as const;

const STATE_VARIANT = {
  downloaded: "secondary",
  bundled: "outline",
  pending: "destructive",
  local: "outline",
} as const;

/** Lists where each rule-set the active profile needs comes from. */
function RuleSetStatusDialog({
  open,
  onOpenChange,
  statuses,
  onStatusesChange,
}: RuleSetStatusDialogProps) {
  const { resolved: lang } = useLanguagePreference();
  const locale = formatLocale(lang);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (open) setError(null);
  }, [open]);

  async function refresh() {
    setBusy(true);
    setError(null);
    try {
      onStatusesChange(await api.refreshRuleSets());
    } catch (e) {
      setError(formatInvokeError(e));
    } finally {
      setBusy(false);
    }
  }

  function details(set: RuleSetStatus): string[] {
    const parts: string[] = [];
    if (set.state === "pending") parts.push(t("ruleSets.pendingHint"));
    if (set.state === "bundled" && set.source === "download") {
      parts.push(t("ruleSets.bundledHint"));
    }
    if (set.updated_at) {
      parts.push(
        t("ruleSets.updatedAt", {
          time: new Date(set.updated_at * 1000).toLocaleString(locale),
        }),
      );
    }
    if (set.items !== undefined) {
      parts.push(t("ruleSets.items", { count: set.items.toLocaleString(locale) }));
    }
    return parts;
  }

  const downloadable = statuses.some((s) => s.url);

  return (
    <DialogPrimitive.Root open={open} onOpenChange={onOpenChange}>
      <DialogPrimitive.Portal>
        <DialogPrimitive.Overlay
          data-slot="dialog-overlay"
          className="fixed inset-0 z-50 bg-black/40 data-open:animate-in data-open:fade-in-0 data-closed:animate-out data-closed:fade-out-0"
        />
        <DialogPrimitive.Content
          data-slot="dialog-content"
          data-testid="rule-set-status-dialog"
          className="fixed left-1/2 top-1/2 z-50 grid max-h-[85vh] w-[calc(100%-2rem)] max-w-lg -translate-x-1/2 -translate-y-1/2 grid-rows-[auto_auto_minmax(0,1fr)_auto] gap-3 rounded-xl border border-border bg-background p-5 shadow-lg outline-none data-open:animate-in data-open:fade-in-0 data-open:zoom-in-95 data-closed:animate-out data-closed:fade-out-0 data-closed:zoom-out-95"
        >
          <div className="grid gap-1">
            <DialogPrimitive.Title className="text-base font-semibold">
              {t("ruleSets.title")}
            </DialogPrimitive.Title>
            <DialogPrimitive.Description className="text-sm text-muted-foreground">
              {t("ruleSets.desc")}
            </DialogPrimitive.Description>
          </div>
          {error ? <ErrorAlert>{error}</ErrorAlert> : <div />}
          {statuses.length === 0 ? (
            <p className="py-6 text-center text-sm text-muted-foreground">
              {t("ruleSets.empty")}
            </p>
          ) : (
            <ScrollArea type="scroll" className="min-h-0 overflow-hidden">
              <ItemGroup className="gap-0 pr-3">
                {statuses.map((set, index) => (
                  <div key={set.tag}>
                    {index > 0 ? <ItemSeparator className="my-0" /> : null}
                    <Item size="sm" className="px-0">
                      <ItemContent className="min-w-0">
                        <ItemTitle className="w-full min-w-0">
                          <span className="truncate font-mono text-xs" title={set.url ?? set.tag}>
                            {set.tag}
                          </span>
                          <Badge variant={STATE_VARIANT[set.state]} className="ml-auto shrink-0">
                            {t(STATE_LABEL[set.state])}
                          </Badge>
                        </ItemTitle>
                        {details(set).length > 0 ? (
                          <ItemDescription>{details(set).join(" · ")}</ItemDescription>
                        ) : null}
                        {set.last_error ? (
                          <ItemDescription className="text-destructive">
                            {t("ruleSets.lastError", { detail: set.last_error })}
                          </ItemDescription>
                        ) : null}
                      </ItemContent>
                    </Item>
                  </div>
                ))}
              </ItemGroup>
            </ScrollArea>
          )}
          <div className="flex justify-end gap-2">
            <DialogPrimitive.Close asChild>
              <Button type="button" variant="outline" size="sm">
                {t("ruleSets.close")}
              </Button>
            </DialogPrimitive.Close>
            {downloadable ? (
              <Button type="button" size="sm" onClick={() => void refresh()} disabled={busy}>
                {busy ? t("ruleSets.refreshing") : t("ruleSets.refresh")}
              </Button>
            ) : null}
          </div>
        </DialogPrimitive.Content>
      </DialogPrimitive.Portal>
    </DialogPrimitive.Root>
  );
}

export { RuleSetStatusDialog };
