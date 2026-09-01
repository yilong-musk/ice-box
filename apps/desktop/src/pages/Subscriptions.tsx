import { useCallback, useEffect, useState } from "react";
import {
  api,
  formatInvokeError,
  type SubscriptionMeta,
} from "../api/tauri";
import { useGenerationGuard } from "../lib/generationGuard";
import {
  isInsecureSubscriptionUrl,
  extractApplyWarning,
  formatApplyWarning,
  extractUpdateResults,
  formatUpdateFailures,
} from "../lib/subscriptions";
import { clearNodesSnapshot } from "../lib/nodes";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { ErrorAlert, WarnAlert } from "../components/StatusAlert";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardAction,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field";
import { Label } from "@/components/ui/label";
import { Input } from "@/components/ui/input";
import {
  Item,
  ItemActions,
  ItemContent,
  ItemDescription,
  ItemGroup,
  ItemSeparator,
  ItemTitle,
} from "@/components/ui/item";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Switch } from "@/components/ui/switch";
import { t, useLanguagePreference } from "../lib/i18n";

export function Subscriptions() {
  useLanguagePreference();
  const { nextGeneration, isStale } = useGenerationGuard();
  const [items, setItems] = useState<SubscriptionMeta[]>([]);
  const [url, setUrl] = useState("");
  const [name, setName] = useState("");
  const [autoUpdate, setAutoUpdate] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [warning, setWarning] = useState<string | null>(null);
  const [updateFailures, setUpdateFailures] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [updating, setUpdating] = useState(false);
  const [pendingDelete, setPendingDelete] = useState<SubscriptionMeta | null>(
    null,
  );

  const refresh = useCallback(async () => {
    const gen = nextGeneration();
    try {
      const next = await api.listSubscriptions();
      if (isStale(gen)) return;
      setItems(next);
      setError(null);
    } catch (e) {
      if (!isStale(gen)) setError(formatInvokeError(e));
    }
  }, [isStale, nextGeneration]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  async function run(action: () => Promise<unknown>, isUpdate = false) {
    // Subscription changes can replace the node list while the Nodes tab stays mounted.
    clearNodesSnapshot();
    nextGeneration();
    setBusy(true);
    if (isUpdate) setUpdating(true);
    setError(null);
    setWarning(null);
    setUpdateFailures(null);
    try {
      const result = await action();
      const applyWarning = extractApplyWarning(result);
      if (applyWarning) {
        setWarning(formatApplyWarning(applyWarning));
      }
      const results = extractUpdateResults(result);
      if (results) {
        const failures = formatUpdateFailures(results);
        if (failures) setUpdateFailures(failures);
      }
      await refresh();
    } catch (e) {
      setError(formatInvokeError(e));
      await refresh();
    } finally {
      setUpdating(false);
      setBusy(false);
    }
  }

  const httpWarn = isInsecureSubscriptionUrl(url);

  function subscriptionSummary(s: SubscriptionMeta): string {
    const parts = [
      s.format,
      t("subs.summaryNodes", { n: s.node_count }),
      t("subs.summaryGroups", { n: s.group_count ?? 0 }),
      t("subs.summaryRules", { n: s.rule_count ?? 0 }),
    ];
    if (s.has_dns) parts.push("DNS");
    if (s.last_updated) {
      parts.push(new Date(s.last_updated).toLocaleString());
    }
    return parts.join(" · ");
  }

  return (
    <div className="subs-panel flex min-h-0 flex-1 flex-col gap-3">
      {error && <ErrorAlert className="shrink-0">{error}</ErrorAlert>}
      {warning && <WarnAlert className="shrink-0">{warning}</WarnAlert>}
      {updateFailures && (
        <ErrorAlert className="shrink-0">
          {t("subs.partialUpdateFailed", { details: updateFailures })}
        </ErrorAlert>
      )}

      <Card size="sm" className="shrink-0">
        <CardHeader>
          <CardTitle>{t("subs.import")}</CardTitle>
          <CardDescription>{t("subs.importDesc")}</CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-3">
          <form
            className="flex flex-col gap-3"
            onSubmit={(e) => {
              e.preventDefault();
              const u = url.trim();
              if (!u) return;
              void run(async () => {
                await api.addSubscription(
                  u,
                  name.trim() || undefined,
                  autoUpdate,
                );
                setUrl("");
                setName("");
                setAutoUpdate(false);
              });
            }}
          >
            <FieldGroup>
              <Field>
                <FieldLabel htmlFor="sub-url">{t("subs.url")}</FieldLabel>
                <Input
                  id="sub-url"
                  type="url"
                  placeholder={t("subs.urlPlaceholder")}
                  value={url}
                  onChange={(e) => setUrl(e.target.value)}
                  disabled={busy}
                  required
                />
              </Field>
              <div className="flex flex-wrap items-end gap-2">
                <Field className="min-w-48 flex-1">
                  <FieldLabel htmlFor="sub-name">{t("subs.name")}</FieldLabel>
                  <Input
                    id="sub-name"
                    type="text"
                    placeholder={t("subs.namePlaceholder")}
                    value={name}
                    onChange={(e) => setName(e.target.value)}
                    disabled={busy}
                  />
                </Field>
                <Button
                  type="submit"
                  size="sm"
                  disabled={busy || !url.trim()}
                >
                  {t("subs.importAction")}
                </Button>
              </div>
            </FieldGroup>
            <Field orientation="horizontal" className="gap-1.5">
              <Switch
                id="sub-auto-update"
                size="sm"
                checked={autoUpdate}
                disabled={busy}
                aria-label={t("subs.autoUpdate")}
                onCheckedChange={setAutoUpdate}
              />
              <FieldLabel
                htmlFor="sub-auto-update"
                className="text-muted-foreground"
              >
                {t("subs.autoUpdate")}
              </FieldLabel>
            </Field>
          </form>
          {httpWarn ? (
            <WarnAlert>{t("subs.httpWarn")}</WarnAlert>
          ) : null}
        </CardContent>
      </Card>

      <Card size="sm" className="flex min-h-0 flex-1 flex-col overflow-hidden">
        <CardHeader className="shrink-0">
          <CardTitle>{t("subs.title")}</CardTitle>
          <CardDescription>
            {items.length === 0
              ? t("subs.emptyHint")
              : t("subs.count", { n: items.length })}
          </CardDescription>
          <CardAction className="flex flex-wrap items-center justify-end gap-2">
            <Button
              type="button"
              size="sm"
              variant="outline"
              disabled={busy || items.length === 0}
              onClick={() =>
                void run(() => api.updateAllSubscriptions(), true)
              }
            >
              {updating ? t("common.updating") : t("subs.updateAll")}
            </Button>
            <Button
              type="button"
              size="sm"
              variant="outline"
              disabled={busy || items.length === 0}
              onClick={() => void run(() => api.applySubscriptions())}
            >
              {t("subs.apply")}
            </Button>
          </CardAction>
        </CardHeader>
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
              <ItemGroup aria-label={t("subs.listAria")} className="gap-0">
                {items.map((s, index) => {
                  const warnings = s.parse_warnings ?? [];
                  return (
                    <div key={s.id}>
                      {index > 0 ? <ItemSeparator className="my-0" /> : null}
                      <Item
                        size="sm"
                        variant={s.active ? "muted" : "default"}
                      >
                        <ItemContent className="min-w-0">
                          <ItemTitle title={s.name}>
                            <span className="truncate">{s.name}</span>
                            {s.active ? <Label className="shrink-0 text-ok">{t("subs.activeBadge")}</Label> : null}
                          </ItemTitle>
                          <ItemDescription>
                            {subscriptionSummary(s)}
                          </ItemDescription>
                          {s.last_error ? (
                            <ItemDescription className="text-destructive">
                              {s.last_error}
                            </ItemDescription>
                          ) : null}
                          {warnings.length > 0 ? (
                            <ItemDescription className="text-warn">
                              {warnings.join("；")}
                            </ItemDescription>
                          ) : null}
                        </ItemContent>
                        <ItemActions className="flex-wrap">
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
                          <Field orientation="horizontal" className="w-auto gap-1.5">
                            <Switch
                              id={`sub-auto-${s.id}`}
                              size="sm"
                              checked={!!s.auto_update}
                              disabled={busy}
                              aria-label={t("subs.autoUpdate")}
                              onCheckedChange={(checked) =>
                                void run(() =>
                                  api.setSubscriptionAutoUpdate(s.id, checked),
                                )
                              }
                            />
                            <FieldLabel
                              htmlFor={`sub-auto-${s.id}`}
                              className="text-muted-foreground"
                            >
                              {t("subs.autoUpdate")}
                            </FieldLabel>
                          </Field>
                          <Button
                            type="button"
                            size="sm"
                            variant="outline"
                            disabled={busy}
                            onClick={() =>
                              void run(() => api.updateSubscription(s.id), true)
                            }
                          >
                            {updating ? t("common.updating") : t("common.update")}
                          </Button>
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
                      </Item>
                    </div>
                  );
                })}
              </ItemGroup>
            </ScrollArea>
          )}
        </CardContent>
      </Card>
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
