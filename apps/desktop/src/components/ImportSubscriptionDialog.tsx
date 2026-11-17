// SPDX-License-Identifier: GPL-3.0-or-later

import { useEffect, useRef, useState, type FormEvent } from "react";
import { Dialog as DialogPrimitive } from "radix-ui";

import { Button } from "@/components/ui/button";
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import {
  NativeSelect,
  NativeSelectOption,
} from "@/components/ui/native-select";
import { ErrorAlert, WarnAlert } from "@/components/StatusAlert";
import type { SubscriptionAutoUpdateInterval } from "@/api/client";
import { isInsecureSubscriptionUrl } from "@/lib/subscriptions";
import {
  AUTO_UPDATE_INTERVALS,
  AUTO_UPDATE_NEVER,
  autoUpdateNeverLabel,
  intervalLabel,
  parseAutoUpdateChoice,
} from "@/lib/subscriptionAutoUpdate";
import { t, useLanguagePreference } from "@/lib/i18n";
import { isPhoneShell } from "@platform/shell";

/** Matches `MAX_BODY_BYTES` in ice-subscription. */
const MAX_SUBSCRIPTION_FILE_BYTES = 8 * 1024 * 1024;

/** Frequency stored with an import that starts on "Never": the dropdown's
 * default, kept so turning auto-update on later has a sensible interval. */
const IMPORT_DEFAULT_INTERVAL: SubscriptionAutoUpdateInterval = "twelve_hours";

type ImportMode = "url" | "file";

type ImportSubscriptionDialogProps = {
  open: boolean;
  busy: boolean;
  error: string | null;
  onOpenChange: (open: boolean) => void;
  /** `autoUpdate` is false when "Never" is chosen; `interval` is then the last
   * chosen frequency, kept for when auto-update is turned back on. */
  onImportUrl: (
    url: string,
    name: string | undefined,
    autoUpdate: boolean,
    interval: SubscriptionAutoUpdateInterval,
  ) => Promise<void>;
  onImportFile: (content: string, name: string | undefined) => Promise<void>;
};

function nameFromFilename(filename: string): string {
  const base = filename.split(/[/\\]/).pop() ?? "";
  return base.replace(/\.json$/i, "").trim().slice(0, 200);
}

function readFileText(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      resolve(typeof reader.result === "string" ? reader.result : "");
    };
    reader.onerror = () => {
      reject(reader.error ?? new Error("read failed"));
    };
    reader.readAsText(file);
  });
}

function ImportSubscriptionDialog({
  open,
  busy,
  error,
  onOpenChange,
  onImportUrl,
  onImportFile,
}: ImportSubscriptionDialogProps) {
  useLanguagePreference();
  const phone = isPhoneShell();
  const fileInputRef = useRef<HTMLInputElement>(null);
  const [mode, setMode] = useState<ImportMode>("url");
  const [url, setUrl] = useState("");
  const [name, setName] = useState("");
  const [autoUpdateChoice, setAutoUpdateChoice] = useState<string>("twelve_hours");
  const [file, setFile] = useState<File | null>(null);
  const [fileError, setFileError] = useState<string | null>(null);
  const [reading, setReading] = useState(false);

  useEffect(() => {
    if (!open) return;
    setMode("url");
    setUrl("");
    setName("");
    setAutoUpdateChoice("twelve_hours");
    setFile(null);
    setFileError(null);
    setReading(false);
    if (fileInputRef.current) fileInputRef.current.value = "";
  }, [open]);

  const locked = busy || reading;
  const httpWarn = mode === "url" && isInsecureSubscriptionUrl(url);
  const canSubmit =
    !locked && (mode === "url" ? url.trim().length > 0 : file !== null);

  async function submit(event: FormEvent) {
    event.preventDefault();
    if (!canSubmit) return;
    if (mode === "url") {
      const choice = parseAutoUpdateChoice(autoUpdateChoice);
      const interval = choice?.autoUpdate ? choice.interval : IMPORT_DEFAULT_INTERVAL;
      await onImportUrl(
        url.trim(),
        name.trim() || undefined,
        phone ? false : (choice?.autoUpdate ?? false),
        interval,
      );
      return;
    }
    if (!file) return;
    if (file.size > MAX_SUBSCRIPTION_FILE_BYTES) {
      setFile(null);
      setFileError(t("subs.importFileTooLarge"));
      if (fileInputRef.current) fileInputRef.current.value = "";
      return;
    }
    setReading(true);
    setFileError(null);
    try {
      const text = await readFileText(file);
      if (!text.trim()) {
        setFileError(t("subs.importFileReadFailed"));
        return;
      }
      const chosen = name.trim() || nameFromFilename(file.name) || undefined;
      await onImportFile(text, chosen);
    } catch {
      setFileError(t("subs.importFileReadFailed"));
    } finally {
      setReading(false);
    }
  }

  return (
    <DialogPrimitive.Root
      open={open}
      onOpenChange={(next) => {
        if (!next && locked) return;
        onOpenChange(next);
      }}
    >
      <DialogPrimitive.Portal>
        <DialogPrimitive.Overlay
          data-slot="dialog-overlay"
          className="fixed inset-0 z-50 bg-black/40 data-open:animate-in data-open:fade-in-0 data-closed:animate-out data-closed:fade-out-0"
        />
        <DialogPrimitive.Content
          data-slot="dialog-content"
          data-testid="sub-import-dialog"
          className="fixed left-1/2 top-1/2 z-50 grid w-full max-w-lg -translate-x-1/2 -translate-y-1/2 gap-4 rounded-xl border border-border bg-background p-5 shadow-lg outline-none data-open:animate-in data-open:fade-in-0 data-open:zoom-in-95 data-closed:animate-out data-closed:fade-out-0 data-closed:zoom-out-95"
        >
          <div className="flex flex-col gap-1.5">
            <DialogPrimitive.Title
              data-slot="dialog-title"
              className="font-heading text-sm font-medium"
            >
              {t("subs.importTitle")}
            </DialogPrimitive.Title>
            <DialogPrimitive.Description
              data-slot="dialog-description"
              className="text-sm text-muted-foreground"
            >
              {t("subs.importDesc")}
            </DialogPrimitive.Description>
          </div>
          <form className="flex flex-col gap-4" onSubmit={(event) => void submit(event)}>
            <div className="flex gap-2" role="group" aria-label={t("subs.importMode")}>
              <Button
                type="button"
                size="sm"
                variant={mode === "url" ? "default" : "outline"}
                aria-pressed={mode === "url"}
                disabled={locked}
                onClick={() => {
                  setMode("url");
                  setFileError(null);
                }}
              >
                {t("subs.importFromUrl")}
              </Button>
              <Button
                type="button"
                size="sm"
                variant={mode === "file" ? "default" : "outline"}
                aria-pressed={mode === "file"}
                disabled={locked}
                onClick={() => {
                  setMode("file");
                  setFileError(null);
                }}
              >
                {t("subs.importFromFile")}
              </Button>
            </div>
            <FieldGroup>
              {mode === "url" ? (
                <Field>
                  <FieldLabel htmlFor="import-sub-url">{t("subs.url")}</FieldLabel>
                  <Input
                    id="import-sub-url"
                    type="url"
                    placeholder={t("subs.urlPlaceholder")}
                    value={url}
                    onChange={(event) => setUrl(event.target.value)}
                    disabled={locked}
                    required
                  />
                </Field>
              ) : (
                <Field>
                  <FieldLabel htmlFor="import-sub-file">
                    {t("subs.importFileLabel")}
                  </FieldLabel>
                  <Input
                    ref={fileInputRef}
                    id="import-sub-file"
                    type="file"
                    accept=".json,application/json"
                    disabled={locked}
                    onChange={(event) => {
                      const next = event.target.files?.[0] ?? null;
                      setFileError(null);
                      if (next && next.size > MAX_SUBSCRIPTION_FILE_BYTES) {
                        setFile(null);
                        setFileError(t("subs.importFileTooLarge"));
                        event.target.value = "";
                        return;
                      }
                      setFile(next);
                    }}
                  />
                  <p className="text-sm text-muted-foreground">
                    {t("subs.importFileHint")}
                  </p>
                </Field>
              )}
              <Field>
                <FieldLabel htmlFor="import-sub-name">{t("subs.name")}</FieldLabel>
                <Input
                  id="import-sub-name"
                  type="text"
                  placeholder={t("subs.namePlaceholder")}
                  value={name}
                  onChange={(event) => setName(event.target.value)}
                  disabled={locked}
                />
              </Field>
              {phone || mode === "file" ? null : (
                <Field className="w-auto">
                  <FieldLabel htmlFor="import-sub-auto-update">
                    {t("subs.autoUpdate")}
                  </FieldLabel>
                  <NativeSelect
                    id="import-sub-auto-update"
                    size="sm"
                    className="w-auto"
                    value={autoUpdateChoice}
                    disabled={locked}
                    onChange={(event) => setAutoUpdateChoice(event.target.value)}
                  >
                    <NativeSelectOption value={AUTO_UPDATE_NEVER}>
                      {autoUpdateNeverLabel()}
                    </NativeSelectOption>
                    {AUTO_UPDATE_INTERVALS.map((interval) => (
                      <NativeSelectOption key={interval} value={interval}>
                        {intervalLabel(interval)}
                      </NativeSelectOption>
                    ))}
                  </NativeSelect>
                </Field>
              )}
            </FieldGroup>
            {httpWarn ? <WarnAlert>{t("subs.httpWarn")}</WarnAlert> : null}
            {fileError ? <ErrorAlert>{fileError}</ErrorAlert> : null}
            {error ? <ErrorAlert>{error}</ErrorAlert> : null}
            <div className="flex flex-col-reverse justify-end gap-2 sm:flex-row sm:justify-end">
              <Button
                type="button"
                size="sm"
                variant="outline"
                disabled={locked}
                onClick={() => onOpenChange(false)}
              >
                {t("common.cancel")}
              </Button>
              <Button type="submit" size="sm" disabled={!canSubmit}>
                {t("subs.importAction")}
              </Button>
            </div>
          </form>
        </DialogPrimitive.Content>
      </DialogPrimitive.Portal>
    </DialogPrimitive.Root>
  );
}

export { ImportSubscriptionDialog };
