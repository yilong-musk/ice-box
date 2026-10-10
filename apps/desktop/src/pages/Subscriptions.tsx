// SPDX-License-Identifier: GPL-3.0-or-later

import { useCallback, useEffect, useRef, useState } from "react";
import { Share } from "lucide-react";
import {
  api,
  formatInvokeError,
  formatUiMessage,
  type SubscriptionAutoUpdateInterval,
  type SubscriptionMeta,
} from "../api/client";
import { useGenerationGuard } from "../lib/generationGuard";
import {
  extractApplyWarning,
  formatApplyWarning,
  subscriptionTrafficView,
} from "../lib/subscriptions";
import { clearNodesSnapshot } from "../lib/nodes";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { TitlebarActions } from "../components/TitlebarActions";
import { ImportSubscriptionDialog } from "../components/ImportSubscriptionDialog";
import { ShareSubscriptionDialog } from "../components/ShareSubscriptionDialog";
import { ErrorAlert, WarnAlert } from "../components/StatusAlert";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardTitle,
} from "@/components/ui/card";
import { Field, FieldLabel } from "@/components/ui/field";
import { Label } from "@/components/ui/label";
import {
  Item,
  ItemActions,
  ItemContent,
  ItemDescription,
  ItemFooter,
  ItemGroup,
  ItemHeader,
  ItemTitle,
} from "@/components/ui/item";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  NativeSelect,
  NativeSelectOption,
} from "@/components/ui/native-select";
import { Switch } from "@/components/ui/switch";
import { t, useLanguagePreference } from "../lib/i18n";
import { isPhoneShell } from "@platform/shell";

const AUTO_UPDATE_INTERVALS: SubscriptionAutoUpdateInterval[] = [
  "one_hour",
  "three_hours",
  "six_hours",
  "twelve_hours",
  "twenty_four_hours",
];

function intervalLabel(interval: SubscriptionAutoUpdateInterval): string {
  return t(`subs.interval.${interval}`);
}

function activeSubscriptionId(items: SubscriptionMeta[]): string | null {
  return items.find((item) => item.active)?.id ?? null;
}

type Props = {
  /** When false the page stays mounted but its title-bar action is hidden. */
  active?: boolean;
};

export function Subscriptions({ active = true }: Props) {
  useLanguagePreference();
  const phone = isPhoneShell();
  const { nextGeneration, isStale } = useGenerationGuard();
  const [items, setItems] = useState<SubscriptionMeta[]>([]);
  const [importOpen, setImportOpen] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [warning, setWarning] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [updating, setUpdating] = useState(false);
  const [pendingDelete, setPendingDelete] = useState<SubscriptionMeta | null>(
    null,
  );
  const [shareTarget, setShareTarget] = useState<SubscriptionMeta | null>(null);
  const activeIdRef = useRef<string | null>(null);

  const refresh = useCallback(async (keepError = false) => {
    const gen = nextGeneration();
    try {
      const next = await api.listSubscriptions();
      if (isStale(gen)) return;
      const nextActive = activeSubscriptionId(next);
      // Drop the node snapshot only when the live profile moved. A mode or
      // service toggle also lands on `app://state-changed`.
      if (activeIdRef.current !== null && activeIdRef.current !== nextActive) {
        clearNodesSnapshot();
      }
      activeIdRef.current = nextActive;
      setItems(next);
      if (!keepError) setError(null);
    } catch (e) {
      if (!isStale(gen)) setError(formatInvokeError(e));
    }
  }, [isStale, nextGeneration]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  // The tray「订阅」submenu switches the active subscription while this page may
  // be on screen. Re-read the index so the active badge and switch follow
  // without waiting for a manual refresh. `refresh` drops the node snapshot
  // only when the active id actually changed.
  useEffect(() => {
    if (typeof api.listenStateChanged !== "function") return;
    let cancelled = false;
    let unlisten = () => {};
    void api
      .listenStateChanged(() => {
        void refresh();
      })
      .then((off) => {
        if (cancelled) off();
        else unlisten = off;
      });
    return () => {
      cancelled = true;
      unlisten();
    };
  }, [refresh]);

  async function run(action: () => Promise<unknown>, isUpdate = false): Promise<boolean> {
    // Subscription changes can replace the node list while the Nodes tab stays mounted.
    clearNodesSnapshot();
    nextGeneration();
    setBusy(true);
    if (isUpdate) setUpdating(true);
    setError(null);
    setWarning(null);
    try {
      const result = await action();
      const applyWarning = extractApplyWarning(result);
      if (applyWarning) {
        setWarning(formatApplyWarning(applyWarning));
      }
      await refresh();
      return true;
    } catch (e) {
      setError(formatInvokeError(e));
      await refresh(true);
      return false;
    } finally {
      setUpdating(false);
      setBusy(false);
    }
  }

  function subscriptionSummary(s: SubscriptionMeta): string {
    const parts = [
      s.format,
      t("subs.summaryNodes", { n: s.node_count }),
      t("subs.summaryGroups", { n: s.group_count ?? 0 }),
      t("subs.summaryRules", { n: s.rule_count ?? 0 }),
    ];
    if (s.has_dns) parts.push(t("subs.hasDns"));
    if (s.last_updated) {
      parts.push(new Date(s.last_updated).toLocaleString());
    }
    return parts.join(" · ");
  }

  return (
    <div className="subs-panel flex min-h-0 flex-1 flex-col gap-3" data-testid="subs-panel">
      {active ? (
        <TitlebarActions>
          <Button
            type="button"
            size="sm"
            variant="outline"
            disabled={busy}
            onClick={() => {
              setError(null);
              setImportOpen(true);
            }}
          >
            {t("subs.importTitle")}
          </Button>
        </TitlebarActions>
      ) : null}
      {error && !importOpen && (
        <ErrorAlert className="shrink-0">{error}</ErrorAlert>
      )}
      {warning && <WarnAlert className="shrink-0">{warning}</WarnAlert>}

      <Card size="sm" className="flex min-h-0 flex-1 flex-col overflow-hidden">
        <CardContent className="relative flex min-h-0 flex-1 flex-col overflow-hidden">
          {items.length === 0 ? (
            <div className="my-auto flex flex-col items-start gap-1">
              <CardTitle>{t("subs.emptyTitle")}</CardTitle>
              <CardDescription>{t("subs.emptyDesc")}</CardDescription>
            </div>
          ) : (
            <ScrollArea
              type="scroll"
              scrollHideDelay={600}
              className="min-h-0 flex-1 overflow-hidden"
            >
              <ItemGroup aria-label={t("subs.listAria")} className="gap-3">
                {items.map((s) => {
                  const warnings = s.parse_warnings ?? [];
                  const traffic = subscriptionTrafficView(
                    s.userinfo,
                    s.provider_info,
                  );
                  return (
                    <Item
                      key={s.id}
                      size="sm"
                      variant="outline"
                      className={
                        s.active ? "border-foreground/30 bg-muted" : "bg-muted"
                      }
                    >
                      <ItemHeader>
                        <ItemTitle title={s.name}>
                          <span className="truncate">{s.name}</span>
                          {s.source === "file" ? (
                            <Label className="shrink-0 text-muted-foreground">
                              {t("subs.fileBadge")}
                            </Label>
                          ) : null}
                          {s.active ? <Label className="shrink-0 text-ok">{t("subs.activeBadge")}</Label> : null}
                        </ItemTitle>
                        <ItemActions className="flex-nowrap">
                          <Button
                            type="button"
                            size="sm"
                            variant="outline"
                            title={t("subs.share")}
                            onClick={() => setShareTarget(s)}
                          >
                            <Share />
                            {t("subs.share")}
                          </Button>
                          {s.source === "file" ? null : (
                            <Button
                              type="button"
                              size="sm"
                              disabled={busy}
                              onClick={() =>
                                void run(() => api.updateSubscription(s.id), true)
                              }
                            >
                              {updating ? t("common.updating") : t("common.update")}
                            </Button>
                          )}
                          <Button
                            type="button"
                            size="sm"
                            variant="destructive"
                            disabled={busy}
                            onClick={() => setPendingDelete(s)}
                          >
                            {t("common.delete")}
                          </Button>
                        </ItemActions>
                      </ItemHeader>
                      <ItemContent className="min-w-0">
                        <ItemDescription>
                          {subscriptionSummary(s)}
                        </ItemDescription>
                        {traffic ? (
                          <ItemDescription
                            className="tabular-nums"
                            data-testid={`sub-traffic-${s.id}`}
                          >
                            {traffic.usage}
                            {traffic.usage && traffic.expiry ? " · " : null}
                            {traffic.expiry ? (
                              <span
                                className={
                                  traffic.expired
                                    ? "text-destructive"
                                    : undefined
                                }
                              >
                                {traffic.expiry}
                              </span>
                            ) : null}
                          </ItemDescription>
                        ) : null}
                        {s.last_error ? (
                          <ItemDescription className="text-destructive">
                            {formatUiMessage(s.last_error)}
                          </ItemDescription>
                        ) : null}
                        {warnings.length > 0 ? (
                          <ItemDescription className="text-warn">
                            {warnings.map(formatUiMessage).join("；")}
                          </ItemDescription>
                        ) : null}
                      </ItemContent>
                      <ItemFooter>
                        <Field orientation="horizontal" className="w-auto gap-1.5">
                          <Switch
                            id={`sub-active-${s.id}`}
                            size="sm"
                            checked={!!s.active}
                            disabled={busy}
                            aria-label={t("common.activate")}
                            onCheckedChange={(checked) =>
                              void run(() =>
                                api.setSubscriptionActive(s.id, checked),
                              )
                            }
                          />
                          <FieldLabel
                            htmlFor={`sub-active-${s.id}`}
                            className="text-muted-foreground"
                          >
                            {t("common.activate")}
                          </FieldLabel>
                        </Field>
                        {phone || s.source === "file" ? null : (
                          <Field orientation="horizontal" className="w-auto gap-1.5">
                            <Switch
                              id={`sub-auto-${s.id}`}
                              size="sm"
                              checked={!!s.auto_update}
                              disabled={busy}
                              aria-label={t("subs.autoUpdate")}
                              onCheckedChange={(checked) =>
                                void run(() =>
                                  api.setSubscriptionAutoUpdate(
                                    s.id,
                                    checked,
                                    s.auto_update_interval ?? "one_hour",
                                  ),
                                )
                              }
                            />
                            <FieldLabel
                              htmlFor={`sub-auto-${s.id}`}
                              className="text-muted-foreground"
                            >
                              {t("subs.autoUpdate")}
                            </FieldLabel>
                            <NativeSelect
                              size="sm"
                              className="w-auto"
                              aria-label={t("subs.interval")}
                              value={s.auto_update_interval ?? "one_hour"}
                              disabled={busy || !s.auto_update}
                              onChange={(e) =>
                                void run(() =>
                                  api.setSubscriptionAutoUpdate(
                                    s.id,
                                    s.auto_update,
                                    e.target
                                      .value as SubscriptionAutoUpdateInterval,
                                  ),
                                )
                              }
                            >
                              {AUTO_UPDATE_INTERVALS.map((interval) => (
                                <NativeSelectOption
                                  key={interval}
                                  value={interval}
                                >
                                  {intervalLabel(interval)}
                                </NativeSelectOption>
                              ))}
                            </NativeSelect>
                          </Field>
                        )}
                      </ItemFooter>
                    </Item>
                  );
                })}
              </ItemGroup>
            </ScrollArea>
          )}
        </CardContent>
      </Card>
      <ImportSubscriptionDialog
        open={importOpen}
        busy={busy}
        error={error}
        onOpenChange={setImportOpen}
        onImportUrl={async (url, name, autoUpdate, interval) => {
          const ok = await run(async () => {
            await api.addSubscription(
              url,
              name,
              phone ? false : autoUpdate,
              interval,
            );
          });
          if (ok) setImportOpen(false);
        }}
        onImportFile={async (content, name) => {
          const ok = await run(async () => {
            await api.importSubscriptionFile(content, name);
          });
          if (ok) setImportOpen(false);
        }}
      />
      <ShareSubscriptionDialog
        subscription={shareTarget}
        onOpenChange={(open) => {
          if (!open) setShareTarget(null);
        }}
      />
      <ConfirmDialog
        open={pendingDelete !== null}
        title={t("subs.deleteTitle")}
        description={
          pendingDelete
            ? t("subs.deleteConfirm", { name: pendingDelete.name })
            : undefined
        }
        confirmLabel={t("common.delete")}
        busy={busy}
        onOpenChange={(open) => {
          if (!open) setPendingDelete(null);
        }}
        onConfirm={() => {
          if (!pendingDelete) return;
          const sub = pendingDelete;
          setPendingDelete(null);
          void run(() => api.removeSubscription(sub.id));
        }}
      />
    </div>
  );
}
