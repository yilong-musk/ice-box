// SPDX-License-Identifier: GPL-3.0-or-later

import { useCallback, useEffect, useMemo, useState } from "react";
import { Check, ChevronRight, Loader2, Search } from "lucide-react";
import {
  api,
  formatInvokeError,
  type AppProxyMode,
  type InstalledApp,
} from "../api/client";
import { ErrorAlert } from "../components/StatusAlert";
import { Badge } from "@/components/ui/badge";
import { Card } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { cn } from "@/lib/utils";
import { t, useLanguagePreference, type MessageKey } from "../lib/i18n";
import { useGenerationGuard } from "../lib/generationGuard";

type Filter = "all" | "user" | "system";

type Props = {
  /** When false the page stays mounted but reloads on reactivation. */
  active?: boolean;
};

const MODES: ReadonlyArray<{
  mode: AppProxyMode;
  labelKey: MessageKey;
  descKey: MessageKey;
}> = [
  { mode: "follow", labelKey: "apps.mode.follow", descKey: "apps.mode.followDesc" },
  { mode: "proxy", labelKey: "apps.mode.proxy", descKey: "apps.mode.proxyDesc" },
  { mode: "direct", labelKey: "apps.mode.direct", descKey: "apps.mode.directDesc" },
  { mode: "block", labelKey: "apps.mode.block", descKey: "apps.mode.blockDesc" },
];

function modeLabel(mode: AppProxyMode): string {
  return t(MODES.find((item) => item.mode === mode)?.labelKey ?? "apps.mode.follow");
}

function modeBadgeVariant(
  mode: AppProxyMode,
): "default" | "secondary" | "outline" | "destructive" {
  switch (mode) {
    case "proxy":
      return "default";
    case "direct":
      return "outline";
    case "block":
      return "destructive";
    default:
      return "secondary";
  }
}

export function Apps({ active = true }: Props) {
  useLanguagePreference();
  const { nextGeneration, isStale } = useGenerationGuard();
  const [apps, setApps] = useState<InstalledApp[]>([]);
  const [proxies, setProxies] = useState<Record<string, AppProxyMode>>({});
  const [loaded, setLoaded] = useState(false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [keyword, setKeyword] = useState("");
  const [filter, setFilter] = useState<Filter>("all");
  const [selected, setSelected] = useState<InstalledApp | null>(null);
  const [pending, setPending] = useState<{
    packageName: string;
    mode: AppProxyMode;
  } | null>(null);

  const load = useCallback(async () => {
    const gen = nextGeneration();
    setLoading(true);
    try {
      const [list, settings] = await Promise.all([
        api.listApps(),
        api.getSettings(),
      ]);
      if (isStale(gen)) return;
      setApps(list);
      setProxies(settings.app_proxies ?? {});
      setError(null);
    } catch (e) {
      if (!isStale(gen)) setError(formatInvokeError(e));
    } finally {
      if (!isStale(gen)) {
        setLoading(false);
        setLoaded(true);
      }
    }
  }, [isStale, nextGeneration]);

  useEffect(() => {
    if (!active) return;
    void load();
  }, [active, load]);

  const counts = useMemo(() => {
    const system = apps.filter((app) => app.system).length;
    return { all: apps.length, system, user: apps.length - system };
  }, [apps]);

  const filtered = useMemo(() => {
    const needle = keyword.trim().toLowerCase();
    return apps.filter((app) => {
      if (filter === "user" && app.system) return false;
      if (filter === "system" && !app.system) return false;
      if (!needle) return true;
      return (
        app.label.toLowerCase().includes(needle) ||
        app.package_name.toLowerCase().includes(needle)
      );
    });
  }, [apps, filter, keyword]);

  async function chooseMode(mode: AppProxyMode) {
    const app = selected;
    if (!app || pending !== null) return;
    if ((proxies[app.package_name] ?? "follow") === mode) {
      setSelected(null);
      return;
    }
    setPending({ packageName: app.package_name, mode });
    setError(null);
    try {
      await api.setAppProxy(app.package_name, mode);
      setProxies((prev) => {
        const next = { ...prev };
        if (mode === "follow") delete next[app.package_name];
        else next[app.package_name] = mode;
        return next;
      });
      setSelected(null);
    } catch (e) {
      setError(t("apps.applyFailed", { detail: formatInvokeError(e) }));
    } finally {
      setPending(null);
    }
  }

  return (
    <div className="apps-panel flex min-h-0 flex-1 flex-col gap-3" data-testid="apps-panel">
      <p className="shrink-0 text-xs leading-relaxed text-muted-foreground">
        {t("apps.desc")}
      </p>
      {error && <ErrorAlert className="shrink-0">{error}</ErrorAlert>}

      <div className="relative shrink-0">
        <Search
          className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground"
          aria-hidden="true"
        />
        <Input
          type="search"
          value={keyword}
          onChange={(event) => setKeyword(event.target.value)}
          placeholder={t("apps.searchPlaceholder")}
          aria-label={t("apps.searchAria")}
          className="pl-9"
        />
      </div>

      <ToggleGroup
        type="single"
        variant="outline"
        size="sm"
        value={filter}
        onValueChange={(value) => {
          if (value === "all" || value === "user" || value === "system") {
            setFilter(value);
          }
        }}
        className="shrink-0"
        aria-label={t("apps.filterAria")}
      >
        <ToggleGroupItem value="all" className="flex-1">
          {t("apps.filter.all")}
          <span className="ml-1 text-xs text-muted-foreground tabular-nums">
            {counts.all}
          </span>
        </ToggleGroupItem>
        <ToggleGroupItem value="user" className="flex-1">
          {t("apps.filter.user")}
          <span className="ml-1 text-xs text-muted-foreground tabular-nums">
            {counts.user}
          </span>
        </ToggleGroupItem>
        <ToggleGroupItem value="system" className="flex-1">
          {t("apps.filter.system")}
          <span className="ml-1 text-xs text-muted-foreground tabular-nums">
            {counts.system}
          </span>
        </ToggleGroupItem>
      </ToggleGroup>

      <Card
        size="sm"
        className="flex min-h-0 flex-1 flex-col overflow-hidden py-0"
      >
        {!loaded && loading ? (
          <p className="my-auto text-center text-sm text-muted-foreground">
            {t("apps.loading")}
          </p>
        ) : filtered.length === 0 ? (
          <div className="my-auto px-4 text-center">
            <p className="text-sm font-medium">{t("apps.emptyTitle")}</p>
            <p className="mt-1 text-xs text-muted-foreground">
              {t("apps.emptyDesc")}
            </p>
          </div>
        ) : (
          <ScrollArea
            type="scroll"
            scrollHideDelay={600}
            className="min-h-0 flex-1 overflow-hidden"
          >
            <div role="list" aria-label={t("apps.listAria")}>
              {filtered.map((app) => {
                const mode = proxies[app.package_name] ?? "follow";
                return (
                  <div
                    key={app.package_name}
                    role="listitem"
                    className="border-b border-border/60 last:border-b-0 [content-visibility:auto]"
                  >
                    <button
                      type="button"
                      aria-label={t("apps.selectAria", { app: app.label })}
                      className="flex w-full items-center gap-3 px-3 py-2.5 text-left transition-colors active:bg-muted"
                      onClick={() => setSelected(app)}
                    >
                      {app.icon ? (
                        <img
                          src={app.icon}
                          alt=""
                          loading="lazy"
                          className="size-9 shrink-0 rounded-lg object-contain"
                          aria-hidden="true"
                        />
                      ) : (
                        <span
                          className="flex size-9 shrink-0 items-center justify-center rounded-lg bg-muted text-sm font-medium uppercase"
                          aria-hidden="true"
                        >
                          {app.label.slice(0, 1) || "?"}
                        </span>
                      )}
                      <span className="min-w-0 flex-1">
                        <span className="flex min-w-0 items-center gap-1.5">
                          <span className="truncate text-sm font-medium">
                            {app.label}
                          </span>
                          {app.system ? (
                            <span className="shrink-0 rounded bg-muted px-1 py-0.5 text-[10px] leading-none text-muted-foreground">
                              {t("apps.systemBadge")}
                            </span>
                          ) : null}
                        </span>
                        <span className="mt-0.5 block truncate font-mono text-[11px] text-muted-foreground">
                          {app.package_name}
                        </span>
                      </span>
                      {mode === "follow" ? (
                        <span className="shrink-0 text-xs text-muted-foreground">
                          {modeLabel(mode)}
                        </span>
                      ) : (
                        <Badge variant={modeBadgeVariant(mode)}>
                          {modeLabel(mode)}
                        </Badge>
                      )}
                      <ChevronRight
                        className="size-4 shrink-0 text-muted-foreground"
                        aria-hidden="true"
                      />
                    </button>
                  </div>
                );
              })}
            </div>
          </ScrollArea>
        )}
      </Card>

      <Sheet
        open={selected !== null}
        onOpenChange={(open) => {
          if (!open) setSelected(null);
        }}
      >
        <SheetContent
          side="bottom"
          className="max-h-[82svh] gap-0 rounded-t-xl pb-[max(1rem,env(safe-area-inset-bottom))]"
        >
          <SheetHeader>
            <SheetTitle className="truncate">
              {selected?.label ?? ""}
            </SheetTitle>
            <SheetDescription>
              {t("apps.sheetDesc")}
              {selected ? (
                <span className="mt-0.5 block truncate font-mono text-[11px]">
                  {selected.package_name}
                </span>
              ) : null}
            </SheetDescription>
          </SheetHeader>
          {error ? (
            <ErrorAlert className="mx-3 mb-2 shrink-0">{error}</ErrorAlert>
          ) : null}
          <div
            className="flex min-h-0 flex-1 flex-col gap-1.5 overflow-y-auto px-3 pb-2"
            role="group"
            aria-label={t("apps.modeAria")}
          >
            {MODES.map(({ mode, labelKey, descKey }) => {
              const current = selected
                ? (proxies[selected.package_name] ?? "follow") === mode
                : false;
              const busy = selected
                ? pending?.packageName === selected.package_name &&
                  pending.mode === mode
                : false;
              return (
                <button
                  key={mode}
                  type="button"
                  className={cn(
                    "flex w-full items-start gap-3 rounded-lg border px-3 py-2.5 text-left transition-colors disabled:opacity-60",
                    current
                      ? "border-primary bg-primary/5"
                      : "border-border active:bg-muted",
                  )}
                  aria-pressed={current}
                  disabled={pending !== null}
                  onClick={() => void chooseMode(mode)}
                >
                  <span
                    className={cn(
                      "mt-0.5 flex size-5 shrink-0 items-center justify-center rounded-full border",
                      current
                        ? "border-primary bg-primary text-primary-foreground"
                        : "border-border",
                    )}
                    aria-hidden="true"
                  >
                    {busy ? (
                      <Loader2 className="size-3.5 animate-spin" />
                    ) : current ? (
                      <Check className="size-3" />
                    ) : null}
                  </span>
                  <span className="min-w-0 flex-1">
                    <span className="block text-sm font-medium">
                      {t(labelKey)}
                    </span>
                    <span className="mt-0.5 block text-xs leading-relaxed text-muted-foreground">
                      {t(descKey)}
                    </span>
                  </span>
                </button>
              );
            })}
          </div>
        </SheetContent>
      </Sheet>
    </div>
  );
}
