// SPDX-License-Identifier: GPL-3.0-or-later

import { useEffect, useRef, useState } from "react";
import { AlertDialog as AlertDialogPrimitive } from "radix-ui";

import { api, formatInvokeError, type SubscriptionMeta } from "@/api/client";
import { Button } from "@/components/ui/button";
import { ErrorAlert } from "@/components/StatusAlert";
import { t, useLanguagePreference } from "@/lib/i18n";
import { copyText } from "@/lib/logCopy";

type ShareKind = "url" | "singbox" | "export";

type ShareSubscriptionDialogProps = {
  subscription: SubscriptionMeta | null;
  onOpenChange: (open: boolean) => void;
};

function ShareSubscriptionDialog({
  subscription,
  onOpenChange,
}: ShareSubscriptionDialogProps) {
  useLanguagePreference();
  const [copying, setCopying] = useState<ShareKind | null>(null);
  const [copied, setCopied] = useState<ShareKind | null>(null);
  const [error, setError] = useState<string | null>(null);
  const copiedTimer = useRef<number | null>(null);

  useEffect(() => {
    setCopying(null);
    setCopied(null);
    setError(null);
    return () => {
      if (copiedTimer.current !== null) {
        window.clearTimeout(copiedTimer.current);
        copiedTimer.current = null;
      }
    };
  }, [subscription?.id]);

  async function copy(kind: "url" | "singbox") {
    if (!subscription || copying) return;
    setCopying(kind);
    setError(null);
    try {
      const text = await api.subscriptionShare(subscription.id, kind);
      await copyText(text);
      markDone(kind);
    } catch (err) {
      setCopied(null);
      setError(
        err instanceof Error && err.message === "clipboard unavailable"
          ? t("subs.shareCopyFailed")
          : formatInvokeError(err),
      );
    } finally {
      setCopying(null);
    }
  }

  function markDone(kind: ShareKind) {
    setCopied(kind);
    if (copiedTimer.current !== null) {
      window.clearTimeout(copiedTimer.current);
    }
    copiedTimer.current = window.setTimeout(() => {
      copiedTimer.current = null;
      setCopied((current) => (current === kind ? null : current));
    }, 2000);
  }

  async function exportFile() {
    if (!subscription || copying) return;
    setCopying("export");
    setError(null);
    try {
      const outcome = await api.exportSubscriptionSingbox(
        subscription.id,
        subscription.name,
        t("subs.shareExportSingbox"),
      );
      if (outcome === "saved") markDone("export");
    } catch (err) {
      setCopied(null);
      setError(formatInvokeError(err));
    } finally {
      setCopying(null);
    }
  }

  const open = subscription !== null;
  const fileSource = subscription?.source === "file";

  return (
    <AlertDialogPrimitive.Root
      open={open}
      onOpenChange={(next) => {
        if (!next && copying) return;
        onOpenChange(next);
      }}
    >
      <AlertDialogPrimitive.Portal>
        <AlertDialogPrimitive.Overlay
          data-slot="alert-dialog-overlay"
          className="fixed inset-0 z-50 bg-black/40 data-open:animate-in data-open:fade-in-0 data-closed:animate-out data-closed:fade-out-0"
        />
        <AlertDialogPrimitive.Content
          data-slot="alert-dialog-content"
          data-testid="sub-share-dialog"
          className="fixed left-1/2 top-1/2 z-50 grid w-full max-w-sm -translate-x-1/2 -translate-y-1/2 gap-4 border border-border bg-background p-5 shadow-lg outline-none data-open:animate-in data-open:fade-in-0 data-open:zoom-in-95 data-closed:animate-out data-closed:fade-out-0 data-closed:zoom-out-95"
        >
          <div className="flex flex-col gap-1.5">
            <AlertDialogPrimitive.Title
              data-slot="alert-dialog-title"
              className="font-heading text-sm font-medium"
            >
              {subscription
                ? t("subs.shareTitle", { name: subscription.name })
                : t("subs.share")}
            </AlertDialogPrimitive.Title>
            <AlertDialogPrimitive.Description
              data-slot="alert-dialog-description"
              className="text-sm text-muted-foreground"
            >
              {fileSource ? t("subs.shareDescFile") : t("subs.shareDesc")}
            </AlertDialogPrimitive.Description>
          </div>
          {error ? <ErrorAlert>{error}</ErrorAlert> : null}
          <div className="flex flex-col gap-2">
            {fileSource ? null : (
              <Button
                type="button"
                size="sm"
                variant="outline"
                disabled={copying !== null}
                aria-label={t("subs.shareCopyUrl")}
                onClick={() => void copy("url")}
              >
                {copied === "url" ? t("subs.shareCopied") : t("subs.shareCopyUrl")}
              </Button>
            )}
            <Button
              type="button"
              size="sm"
              variant="outline"
              disabled={copying !== null}
              aria-label={t("subs.shareCopySingbox")}
              onClick={() => void copy("singbox")}
            >
              {copied === "singbox"
                ? t("subs.shareCopied")
                : t("subs.shareCopySingbox")}
            </Button>
            <Button
              type="button"
              size="sm"
              variant="outline"
              disabled={copying !== null}
              aria-label={t("subs.shareExportSingbox")}
              onClick={() => void exportFile()}
            >
              {copied === "export"
                ? t("subs.shareExported")
                : t("subs.shareExportSingbox")}
            </Button>
            <Button
              type="button"
              size="sm"
              variant="ghost"
              disabled={copying !== null}
              onClick={() => onOpenChange(false)}
            >
              {t("common.cancel")}
            </Button>
          </div>
        </AlertDialogPrimitive.Content>
      </AlertDialogPrimitive.Portal>
    </AlertDialogPrimitive.Root>
  );
}

export { ShareSubscriptionDialog };
