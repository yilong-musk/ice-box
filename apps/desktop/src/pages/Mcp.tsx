// SPDX-License-Identifier: GPL-3.0-or-later

import { useCallback, useEffect, useRef, useState } from "react";
import { Bot, Check, Copy, RefreshCw } from "lucide-react";
import { api, formatInvokeError, type McpStatus } from "../api/client";
import { ErrorAlert, WarnAlert } from "../components/StatusAlert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Field, FieldDescription, FieldLabel } from "@/components/ui/field";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Switch } from "@/components/ui/switch";
import { copyText } from "../lib/logCopy";
import { t, useLanguagePreference } from "../lib/i18n";

const POLL_MS = 2000;

export function Mcp({ active = true }: { active?: boolean }) {
  useLanguagePreference();
  const [status, setStatus] = useState<McpStatus | null>(null);
  const [statusError, setStatusError] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [copied, setCopied] = useState(false);
  const copiedTimerRef = useRef<number | null>(null);
  const requestSeqRef = useRef(0);

  const applyStatus = useCallback((next: McpStatus) => {
    setStatus(next);
    setStatusError(next.error);
  }, []);

  const refresh = useCallback(async () => {
    const seq = ++requestSeqRef.current;
    try {
      const next = await api.getMcpStatus();
      if (seq !== requestSeqRef.current) return;
      applyStatus(next);
    } catch (invokeError) {
      if (seq !== requestSeqRef.current) return;
      setStatusError(formatInvokeError(invokeError));
    }
  }, [applyStatus]);

  useEffect(() => {
    if (!active) return;
    void refresh();
    const timer = window.setInterval(() => void refresh(), POLL_MS);
    return () => {
      requestSeqRef.current += 1;
      window.clearInterval(timer);
    };
  }, [active, refresh]);

  useEffect(
    () => () => {
      if (copiedTimerRef.current !== null) {
        window.clearTimeout(copiedTimerRef.current);
      }
    },
    [],
  );

  async function setEnabled(enabled: boolean) {
    requestSeqRef.current += 1;
    setBusy(true);
    setActionError(null);
    try {
      applyStatus(await api.setMcpEnabled(enabled));
    } catch (invokeError) {
      setActionError(formatInvokeError(invokeError));
      await refresh();
    } finally {
      setBusy(false);
    }
  }

  async function restart() {
    requestSeqRef.current += 1;
    setBusy(true);
    setActionError(null);
    try {
      applyStatus(await api.restartMcp());
    } catch (invokeError) {
      setActionError(formatInvokeError(invokeError));
      await refresh();
    } finally {
      setBusy(false);
    }
  }

  async function copyConfig() {
    if (!status?.client_config) return;
    try {
      await copyText(status.client_config);
      setCopied(true);
      setActionError(null);
      if (copiedTimerRef.current !== null) {
        window.clearTimeout(copiedTimerRef.current);
      }
      copiedTimerRef.current = window.setTimeout(() => {
        copiedTimerRef.current = null;
        setCopied(false);
      }, 1600);
    } catch (copyError) {
      setActionError(formatInvokeError(copyError));
    }
  }

  const supported = status?.supported !== false;
  const running = status?.running === true;
  const error = actionError ?? statusError;

  return (
    <div
      className="flex min-h-0 flex-1 flex-col overflow-hidden"
      data-testid="mcp-panel"
    >
      <ScrollArea className="min-h-0 flex-1">
        <div className="flex w-full max-w-3xl flex-col gap-3 pb-4">
          {error ? <ErrorAlert>{error}</ErrorAlert> : null}
          {!supported && status ? (
            <WarnAlert>{t("mcp.unavailable")}</WarnAlert>
          ) : null}

          <Card size="sm" className="w-full">
            <CardHeader>
              <CardTitle className="flex items-center gap-2">
                <Bot className="size-4 text-primary" aria-hidden="true" />
                {t("mcp.service")}
                <Badge
                  variant={running ? "secondary" : "outline"}
                  className={running ? "text-ok" : undefined}
                >
                  {running ? t("mcp.running") : t("mcp.stopped")}
                </Badge>
              </CardTitle>
              <CardDescription>{t("mcp.description")}</CardDescription>
            </CardHeader>
            <CardContent className="flex flex-col gap-4">
              <Field orientation="horizontal" className="w-auto gap-2">
                <Switch
                  id="mcp-enabled"
                  size="sm"
                  checked={status?.enabled ?? false}
                  disabled={busy || !status || !supported}
                  aria-label={t("mcp.enable")}
                  onCheckedChange={(checked) => void setEnabled(checked === true)}
                />
                <FieldLabel htmlFor="mcp-enabled">{t("mcp.enable")}</FieldLabel>
              </Field>
              <FieldDescription>{t("mcp.enableDesc")}</FieldDescription>

              <dl className="grid grid-cols-[auto_1fr] gap-x-6 gap-y-2 rounded-lg border bg-muted/30 p-3 text-sm">
                <dt className="text-muted-foreground">{t("mcp.endpoint")}</dt>
                <dd className="min-w-0 break-all font-mono text-xs">
                  {status?.endpoint ?? t("common.dash")}
                </dd>
                <dt className="text-muted-foreground">{t("mcp.clients")}</dt>
                <dd className="tabular-nums">{status?.client_count ?? 0}</dd>
                <dt className="text-muted-foreground">{t("mcp.tools")}</dt>
                <dd className="tabular-nums">{status?.tool_count ?? 0}</dd>
              </dl>

              <div className="flex flex-wrap gap-2">
                <Button
                  type="button"
                  size="sm"
                  variant="outline"
                  disabled={busy || !status?.enabled}
                  onClick={() => void restart()}
                >
                  <RefreshCw
                    className={busy ? "animate-spin" : undefined}
                    aria-hidden="true"
                  />
                  {t("mcp.restart")}
                </Button>
              </div>
            </CardContent>
          </Card>

          <Card size="sm" className="w-full">
            <CardHeader>
              <CardTitle>{t("mcp.integration")}</CardTitle>
              <CardDescription>{t("mcp.integrationDesc")}</CardDescription>
            </CardHeader>
            <CardContent className="flex flex-col gap-3">
              <pre
                className="max-h-64 overflow-auto rounded-lg border bg-muted/30 p-3 font-mono text-xs leading-relaxed whitespace-pre-wrap break-all"
                data-testid="mcp-client-config"
              >
                {status?.client_config || t("common.dash")}
              </pre>
              <div className="flex flex-wrap items-center gap-2">
                <Button
                  type="button"
                  size="sm"
                  variant="outline"
                  disabled={!status?.client_config}
                  onClick={() => void copyConfig()}
                >
                  {copied ? (
                    <Check aria-hidden="true" />
                  ) : (
                    <Copy aria-hidden="true" />
                  )}
                  {copied ? t("mcp.copied") : t("mcp.copyConfig")}
                </Button>
              </div>
              <FieldDescription>{t("mcp.localOnly")}</FieldDescription>
            </CardContent>
          </Card>
        </div>
      </ScrollArea>
    </div>
  );
}
