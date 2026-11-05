// SPDX-License-Identifier: GPL-3.0-or-later

import { useCallback, useEffect, useRef, useState } from "react";
import { Plus, RefreshCw, Share, Trash2 } from "lucide-react";
import {
  api,
  formatInvokeError,
  formatInvokeErrorTitle,
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
import { CapsuleToast, useCapsuleToast } from "../components/CapsuleToast";
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
import { cn } from "@/lib/utils";
import { formatLocale, t, useLanguagePreference } from "../lib/i18n";
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

/** Anything interactive: native controls, labels, and any element that is
 * focusable or carries an ARIA role (custom widgets, Radix triggers, fields). */
const INTERACTIVE_SELECTOR =
  "button, a[href], input, select, textarea, label, summary, [role], [tabindex], [contenteditable]";

/** True when a click inside `row` landed on an interactive descendant rather
 * than on the row's bare surface. Checks by kind, not by a list of known
 * row controls, so controls added to the row later are covered too. */
function isInsideRowControl(target: EventTarget, row: Element): boolean {
  for (
    let el = target instanceof Element ? target : null;
    el && el !== row;
    el = el.parentElement
  ) {
    if (el.matches(INTERACTIVE_SELECTOR)) return true;
  }
  return false;
}

function activeSubscriptionId(items: SubscriptionMeta[]): string | null {
  return items.find((item) => item.active)?.id ?? null;
}

type Props = {
  /** When false the page stays mounted but its title-bar action is hidden. */
  active?: boolean;
};

export function Subscriptions({ active = true }: Props) {
  const { resolved: lang } = useLanguagePreference();
  const dateLocale = formatLocale(lang);
  const phone = isPhoneShell();
  const { nextGeneration, isStale } = useGenerationGuard();
  const [items, setItems] = useState<SubscriptionMeta[]>([]);
  const [importOpen, setImportOpen] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [warning, setWarning] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [updatingId, setUpdatingId] = useState<string | null>(null);
  const [pendingDelete, setPendingDelete] = useState<SubscriptionMeta | null>(
    null,
  );
  const [shareTarget, setShareTarget] = useState<SubscriptionMeta | null>(null);
  const activeIdRef = useRef<string | null>(null);
  const {
    toast,
    show: showToast,
    dismiss: dismissToast,
  } = useCapsuleToast();

  const refresh = useCallback(async (notifyError = true): Promise<boolean> => {
    const gen = nextGeneration();
    try {
      const next = await api.listSubscriptions();
      if (isStale(gen)) return true;
      const nextActive = activeSubscriptionId(next);
      // Drop the node snapshot only when the live profile moved. A mode or
      // service toggle also lands on `app://state-changed`.
      if (activeIdRef.current !== null && activeIdRef.current !== nextActive) {
        clearNodesSnapshot();
      }
      activeIdRef.current = nextActive;
      setItems(next);
      setLoadError(null);
      return true;
    } catch (e) {
      if (!isStale(gen)) {
        const message = formatInvokeError(e);
        setLoadError(message);
        if (notifyError) showToast(formatInvokeErrorTitle(e));
      }
      return false;
    }
  }, [isStale, nextGeneration, showToast]);

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

  async function run(
    action: () => Promise<unknown>,
    options: {
      updatingId?: string;
      errorSurface?: "toast" | "dialog";
      successToast?: string;
    } = {},
  ): Promise<boolean> {
    const {
      updatingId: rowUpdatingId,
      errorSurface = "toast",
      successToast,
    } = options;
    // Subscription changes can replace the node list while the Nodes tab stays mounted.
    clearNodesSnapshot();
    nextGeneration();
    setBusy(true);
    if (rowUpdatingId) setUpdatingId(rowUpdatingId);
    setError(null);
    setWarning(null);
    try {
      const result = await action();
      const applyWarning = extractApplyWarning(result);
      if (applyWarning) {
        setWarning(formatApplyWarning(applyWarning));
      }
      // Only confirm once the reloaded list has landed; a failed refresh
      // shows its own error and suppresses the success notice.
      const refreshed = await refresh();
      if (successToast && refreshed && !applyWarning) {
        showToast(successToast, "success");
      }
      return true;
    } catch (e) {
      if (errorSurface === "dialog") setError(formatInvokeError(e));
      else showToast(formatInvokeErrorTitle(e));
      // Keep the action failure visible. If the refresh also fails, its
      // persistent inline error is enough; replacing the toast would hide the
      // error the user just triggered.
      await refresh(false);
      return false;
    } finally {
      setUpdatingId(null);
      setBusy(false);
    }
  }

  function activateSubscription(s: SubscriptionMeta) {
    if (busy || s.active) return;
    void run(() => api.setSubscriptionActive(s.id, true));
  }

  /** `dateOnly` drops the time of day so the phone summary fits one line. */
  function subscriptionSummary(s: SubscriptionMeta, dateOnly = false): string {
    const parts = [
      s.format,
      t("subs.summaryNodes", { n: s.node_count }),
      t("subs.summaryGroups", { n: s.group_count ?? 0 }),
      t("subs.summaryRules", { n: s.rule_count ?? 0 }),
    ];
    if (s.has_dns) parts.push(t("subs.hasDns"));
    if (s.last_updated) {
      const updated = new Date(s.last_updated);
      parts.push(
        dateOnly
          ? updated.toLocaleDateString(dateLocale)
          : updated.toLocaleString(dateLocale),
      );
    }
    return parts.join(" · ");
  }

  return (
    <div
      className="subs-panel relative flex min-h-0 flex-1 flex-col gap-3"
      data-testid="subs-panel"
    >
      {active ? (
        <TitlebarActions>
          <Button
            type="button"
            size="sm"
            variant="outline"
            disabled={busy}
            aria-label={t("subs.importTitle")}
            title={phone ? t("subs.importTitle") : undefined}
            onClick={() => {
              setError(null);
              setImportOpen(true);
            }}
          >
            {phone ? <Plus /> : null}
            {phone ? null : t("subs.importTitle")}
          </Button>
        </TitlebarActions>
      ) : null}
      {loadError && items.length > 0 ? (
        <div className="flex items-center gap-2">
          <ErrorAlert className="flex-1">{loadError}</ErrorAlert>
          <Button
            type="button"
            size="sm"
            variant="outline"
            onClick={() => void refresh()}
          >
            {t("common.retry")}
          </Button>
        </div>
      ) : null}
      {warning && <WarnAlert className="shrink-0">{warning}</WarnAlert>}

      <Card size="sm" className="flex min-h-0 flex-1 flex-col overflow-hidden">
        <CardContent className="relative flex min-h-0 flex-1 flex-col overflow-hidden">
          {items.length === 0 ? (
            <div className="my-auto flex flex-col items-start gap-2">
              <CardTitle>
                {loadError ? t("subs.loadFailedTitle") : t("subs.emptyTitle")}
              </CardTitle>
              <CardDescription
                className={loadError ? "text-destructive" : undefined}
              >
                {loadError ?? t("subs.emptyDesc")}
              </CardDescription>
              {loadError ? (
                <Button
                  type="button"
                  size="sm"
                  variant="outline"
                  onClick={() => void refresh()}
                >
                  {t("common.retry")}
                </Button>
              ) : null}
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
                      className={cn(
                        "relative bg-muted",
                        s.active
                          ? "border-ok"
                          : "cursor-pointer hover:border-foreground/30 has-[[data-row-activate]:focus-visible]:border-ring has-[[data-row-activate]:focus-visible]:ring-[3px] has-[[data-row-activate]:focus-visible]:ring-ring/50",
                      )}
                      aria-current={s.active ? "true" : undefined}
                      onClick={(event) => {
                        // Controls inside the row keep their own behavior;
                        // only clicks on the row's bare surface activate it.
                        if (isInsideRowControl(event.target, event.currentTarget)) {
                          return;
                        }
                        // A drag that selects text (e.g. to copy a warning)
                        // also ends in a click; it must not switch profiles.
                        if (window.getSelection()?.isCollapsed === false) {
                          return;
                        }
                        activateSubscription(s);
                      }}
                    >
                      {s.active ? null : (
                        // Keyboard and screen-reader path: a real, named
                        // button; the row shows its focus ring instead.
                        <button
                          type="button"
                          data-row-activate
                          className="sr-only"
                          disabled={busy}
                          aria-label={t("subs.activateRow", { name: s.name })}
                          onClick={() => activateSubscription(s)}
                        />
                      )}
                      <ItemHeader className="flex-nowrap">
                        <ItemTitle title={s.name} className="min-w-0">
                          <span className="truncate">{s.name}</span>
                          {s.source === "file" ? (
                            <Label className="shrink-0 text-muted-foreground">
                              {t("subs.fileBadge")}
                            </Label>
                          ) : null}
                        </ItemTitle>
                        {/* The phone row is too narrow for labelled buttons:
                            icons keep the actions on the title line, and the
                            labels stay as accessible names. */}
                        <ItemActions className="shrink-0 flex-nowrap">
                          <Button
                            type="button"
                            size={phone ? "icon-sm" : "sm"}
                            variant="outline"
                            aria-label={t("subs.share")}
                            title={t("subs.share")}
                            onClick={() => setShareTarget(s)}
                          >
                            <Share />
                            {phone ? null : t("subs.share")}
                          </Button>
                          {s.source === "file" ? null : (
                            <Button
                              type="button"
                              size={phone ? "icon-sm" : "sm"}
                              disabled={busy}
                              aria-label={
                                updatingId === s.id
                                  ? t("common.updating")
                                  : t("common.update")
                              }
                              title={phone ? t("common.update") : undefined}
                              onClick={() =>
                                void run(() => api.updateSubscription(s.id), {
                                  updatingId: s.id,
                                  successToast: t("subs.updated"),
                                })
                              }
                            >
                              {phone ? (
                                <RefreshCw
                                  className={cn(
                                    updatingId === s.id && "animate-spin",
                                  )}
                                />
                              ) : updatingId === s.id ? (
                                t("common.updating")
                              ) : (
                                t("common.update")
                              )}
                            </Button>
                          )}
                          <Button
                            type="button"
                            size={phone ? "icon-sm" : "sm"}
                            variant="destructive"
                            disabled={busy}
                            aria-label={t("common.delete")}
                            title={phone ? t("common.delete") : undefined}
                            onClick={() => setPendingDelete(s)}
                          >
                            {phone ? <Trash2 /> : t("common.delete")}
                          </Button>
                        </ItemActions>
                      </ItemHeader>
                      <ItemContent className="min-w-0">
                        <ItemDescription
                          className={phone ? "truncate" : undefined}
                          title={phone ? subscriptionSummary(s) : undefined}
                        >
                          {subscriptionSummary(s, phone)}
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
                        {warnings.length > 0 ? (
                          <ItemDescription className="text-warn">
                            {warnings.map(formatUiMessage).join("；")}
                          </ItemDescription>
                        ) : null}
                      </ItemContent>
                      {phone || s.source === "file" ? null : (
                        <ItemFooter>
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
                        </ItemFooter>
                      )}
                    </Item>
                  );
                })}
              </ItemGroup>
            </ScrollArea>
          )}
        </CardContent>
      </Card>
      <CapsuleToast toast={toast} onDismiss={dismissToast} />
      <ImportSubscriptionDialog
        open={importOpen}
        busy={busy}
        error={error}
        onOpenChange={setImportOpen}
        onImportUrl={async (url, name, autoUpdate, interval) => {
          const ok = await run(
            async () => {
              await api.addSubscription(
                url,
                name,
                phone ? false : autoUpdate,
                interval,
              );
            },
            { errorSurface: "dialog" },
          );
          if (ok) setImportOpen(false);
        }}
        onImportFile={async (content, name) => {
          const ok = await run(
            async () => {
              await api.importSubscriptionFile(content, name);
            },
            { errorSurface: "dialog" },
          );
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
