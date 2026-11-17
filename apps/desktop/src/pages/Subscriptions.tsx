// SPDX-License-Identifier: GPL-3.0-or-later

import { useCallback, useEffect, useRef, useState } from "react";
import { Plus, RefreshCw, Share, Trash2 } from "lucide-react";
import {
  api,
  formatInvokeError,
  formatInvokeErrorTitle,
  formatUiMessage,
  type SubscriptionMeta,
} from "../api/client";
import { useGenerationGuard } from "../lib/generationGuard";
import {
  extractApplyWarning,
  formatApplyWarning,
  subscriptionTrafficView,
  trafficTone,
} from "../lib/subscriptions";
import {
  AUTO_UPDATE_INTERVALS,
  AUTO_UPDATE_NEVER,
  autoUpdateChoiceValue,
  autoUpdateNeverLabel,
  intervalLabel,
  parseAutoUpdateChoice,
  retainedAutoUpdateInterval,
} from "../lib/subscriptionAutoUpdate";
import { TONE_WASH } from "../lib/tone";
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
import { cn } from "@/lib/utils";
import { formatLocale, t, useLanguagePreference } from "../lib/i18n";
import { isPhoneShell } from "@platform/shell";

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

  /** Update time only. Format and profile stats are not listed on the row. */
  function subscriptionUpdatedAt(s: SubscriptionMeta): string | null {
    if (!s.last_updated) return null;
    return new Date(s.last_updated).toLocaleString(dateLocale);
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
                  const updatedAt = subscriptionUpdatedAt(s);
                  const traffic = subscriptionTrafficView(
                    s.userinfo,
                    s.provider_info,
                  );
                  const used = traffic?.ratio ?? null;
                  const remaining =
                    used != null ? Math.min(1, Math.max(0, 1 - used)) : null;
                  const wash =
                    used != null && remaining != null && remaining > 0
                      ? TONE_WASH[trafficTone(used)]
                      : null;
                  return (
                    <Item
                      key={s.id}
                      size="sm"
                      variant="outline"
                      className={cn(
                        // `isolate` belongs to the `-z-10` row wash below: it
                        // keeps the fill inside this row's stacking context
                        // instead of painting behind the card background.
                        "relative isolate bg-muted",
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
                      {wash != null && remaining != null ? (
                        // Same wash as a connection row: a left-aligned gradient
                        // whose width is the share. Here that share is what
                        // remains. Colour still follows the used-share bands.
                        <div
                          aria-hidden="true"
                          className={cn(
                            "pointer-events-none absolute inset-y-0 left-0 -z-10 rounded-lg bg-linear-to-r transition-[width] duration-500",
                            wash,
                          )}
                          style={{
                            width: `${Math.round(remaining * 10000) / 100}%`,
                          }}
                          data-testid={`sub-remaining-${s.id}`}
                        />
                      ) : null}
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
                        <ItemTitle className="min-w-0">
                          <span className="min-w-0 truncate" title={s.name}>
                            {s.name}
                          </span>
                          {s.source === "file" ? (
                            <Label className="shrink-0 text-muted-foreground">
                              {t("subs.fileBadge")}
                            </Label>
                          ) : null}
                          {/* Phone is too narrow for the time beside the title and the
                              actions, so it stays on its own line below. */}
                          {!phone && updatedAt ? (
                            <span className="shrink-0 text-xs font-normal whitespace-nowrap tabular-nums text-muted-foreground">
                              {updatedAt}
                            </span>
                          ) : null}
                        </ItemTitle>
                        {/* Row actions are icon buttons on every shell; the
                            labels stay as accessible names and hover titles. */}
                        <ItemActions className="shrink-0 flex-nowrap">
                          <Button
                            type="button"
                            size="icon-sm"
                            variant="outline"
                            aria-label={t("subs.share")}
                            title={t("subs.share")}
                            onClick={() => setShareTarget(s)}
                          >
                            <Share aria-hidden="true" />
                          </Button>
                          {s.source === "file" ? null : (
                            <Button
                              type="button"
                              size="icon-sm"
                              disabled={busy}
                              aria-label={
                                updatingId === s.id
                                  ? t("common.updating")
                                  : t("common.update")
                              }
                              title={
                                updatingId === s.id
                                  ? t("common.updating")
                                  : t("common.update")
                              }
                              onClick={() =>
                                void run(() => api.updateSubscription(s.id), {
                                  updatingId: s.id,
                                  successToast: t("subs.updated"),
                                })
                              }
                            >
                              <RefreshCw
                                aria-hidden="true"
                                className={cn(
                                  updatingId === s.id && "animate-spin",
                                )}
                              />
                            </Button>
                          )}
                          <Button
                            type="button"
                            size="icon-sm"
                            variant="destructive"
                            disabled={busy}
                            aria-label={t("common.delete")}
                            title={t("common.delete")}
                            onClick={() => setPendingDelete(s)}
                          >
                            <Trash2 aria-hidden="true" />
                          </Button>
                        </ItemActions>
                      </ItemHeader>
                      {(phone && updatedAt) || traffic || warnings.length > 0 ? (
                        <ItemContent className="min-w-0">
                          {phone && updatedAt ? (
                            <ItemDescription className="truncate tabular-nums" title={updatedAt}>
                              {updatedAt}
                            </ItemDescription>
                          ) : null}
                          {traffic ? (
                            <ItemDescription
                              className="min-w-0 tabular-nums"
                              data-testid={`sub-traffic-${s.id}`}
                            >
                              {traffic.usage}
                              {traffic.usage && traffic.expiry ? " · " : null}
                              {traffic.expiry}
                            </ItemDescription>
                          ) : null}
                          {warnings.length > 0 ? (
                            <ItemDescription className="text-warn">
                              {warnings.map(formatUiMessage).join("；")}
                            </ItemDescription>
                          ) : null}
                        </ItemContent>
                      ) : null}
                      {phone || s.source === "file" ? null : (
                        <ItemFooter>
                          {/* One dropdown: "Never" first, then the intervals.
                              It replaces a switch plus a second dropdown, so
                              the off state is a frequency choice too. */}
                          <Field orientation="horizontal" className="w-auto gap-1.5">
                            <FieldLabel
                              htmlFor={`sub-auto-${s.id}`}
                              className="leading-none text-muted-foreground"
                            >
                              {t("subs.autoUpdate")}
                            </FieldLabel>
                            <NativeSelect
                              id={`sub-auto-${s.id}`}
                              size="sm"
                              className="w-auto"
                              value={autoUpdateChoiceValue(s)}
                              disabled={busy}
                              onChange={(e) => {
                                const choice = parseAutoUpdateChoice(
                                  e.target.value,
                                );
                                if (!choice) return;
                                void run(() =>
                                  api.setSubscriptionAutoUpdate(
                                    s.id,
                                    choice.autoUpdate,
                                    choice.autoUpdate
                                      ? choice.interval
                                      : retainedAutoUpdateInterval(s),
                                  ),
                                );
                              }}
                            >
                              <NativeSelectOption value={AUTO_UPDATE_NEVER}>
                                {autoUpdateNeverLabel()}
                              </NativeSelectOption>
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
