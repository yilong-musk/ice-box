// SPDX-License-Identifier: GPL-3.0-or-later

import { type Dispatch, type SetStateAction } from "react";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import {
  Field,
  FieldDescription,
  FieldError,
  FieldGroup,
  FieldLabel,
} from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import {
  formatPortValidationError,
} from "../../lib/listenValidation";
import { t } from "../../lib/i18n";
import type { AppSettings } from "../../api/tauri";

export function PortsCard({
  form,
  setForm,
  fieldErrors,
  busy,
  loaded,
  clearFieldError,
  setFieldErrors,
}: {
  form: AppSettings;
  setForm: (next: AppSettings) => void;
  fieldErrors: Record<string, string>;
  busy: boolean;
  loaded: boolean;
  clearFieldError: (key: string) => void;
  setFieldErrors: Dispatch<SetStateAction<Record<string, string>>>;
}) {
  return (
    <Card size="sm" className="w-full">
      <CardHeader className="shrink-0">
        <CardTitle>{t("settings.inbound")}</CardTitle>
        <CardDescription>{t("settings.inboundDesc")}</CardDescription>
      </CardHeader>
      <CardContent>
        <div className="flex flex-col gap-3">
          <FieldGroup className="grid grid-cols-1 gap-3 min-[560px]:grid-cols-2">
            <Field data-invalid={!!fieldErrors.mixed_listen || undefined}>
              <FieldLabel htmlFor="settings-mixed-listen">
                {t("settings.mixedListen")}
              </FieldLabel>
              <Input
                id="settings-mixed-listen"
                value={form.mixed_listen}
                aria-invalid={!!fieldErrors.mixed_listen || undefined}
                onChange={(e) => {
                  clearFieldError("mixed_listen");
                  setForm({ ...form, mixed_listen: e.target.value });
                }}
                disabled={busy || !loaded || form.allow_lan}
              />
              {fieldErrors.mixed_listen ? (
                <FieldError>{fieldErrors.mixed_listen}</FieldError>
              ) : null}
            </Field>
            <Field data-invalid={!!fieldErrors.mixed_port || undefined}>
              <FieldLabel htmlFor="settings-mixed-port">
                {t("settings.mixedPort")}
              </FieldLabel>
              <Input
                id="settings-mixed-port"
                type="number"
                min={1024}
                max={65535}
                value={Number.isFinite(form.mixed_port) ? form.mixed_port : ""}
                aria-invalid={!!fieldErrors.mixed_port || undefined}
                onChange={(e) => {
                  const raw = e.target.value;
                  clearFieldError("mixed_port");
                  if (raw.trim() === "") {
                    setFieldErrors((prev) => ({
                      ...prev,
                      mixed_port: formatPortValidationError(
                        t("settings.mixedPort"),
                      ),
                    }));
                    setForm({ ...form, mixed_port: Number.NaN });
                    return;
                  }
                  const n = Number(raw);
                  if (Number.isFinite(n)) {
                    setForm({ ...form, mixed_port: n });
                  }
                }}
                disabled={busy || !loaded}
              />
              {fieldErrors.mixed_port ? (
                <FieldError>{fieldErrors.mixed_port}</FieldError>
              ) : null}
            </Field>
            <Field data-invalid={!!fieldErrors.clash_api_listen || undefined}>
              <FieldLabel htmlFor="settings-clash-listen">
                {t("settings.clashListen")}
              </FieldLabel>
              <Input
                id="settings-clash-listen"
                value={form.clash_api_listen}
                aria-invalid={!!fieldErrors.clash_api_listen || undefined}
                onChange={(e) => {
                  clearFieldError("clash_api_listen");
                  setForm({ ...form, clash_api_listen: e.target.value });
                }}
                disabled={busy || !loaded}
              />
              {fieldErrors.clash_api_listen ? (
                <FieldError>{fieldErrors.clash_api_listen}</FieldError>
              ) : null}
            </Field>
            <Field data-invalid={!!fieldErrors.clash_api_port || undefined}>
              <FieldLabel htmlFor="settings-clash-port">
                {t("settings.clashPort")}
              </FieldLabel>
              <Input
                id="settings-clash-port"
                type="number"
                min={1024}
                max={65535}
                value={
                  Number.isFinite(form.clash_api_port) ? form.clash_api_port : ""
                }
                aria-invalid={!!fieldErrors.clash_api_port || undefined}
                onChange={(e) => {
                  const raw = e.target.value;
                  clearFieldError("clash_api_port");
                  if (raw.trim() === "") {
                    setFieldErrors((prev) => ({
                      ...prev,
                      clash_api_port: formatPortValidationError(
                        t("settings.clashPort"),
                      ),
                    }));
                    setForm({ ...form, clash_api_port: Number.NaN });
                    return;
                  }
                  const n = Number(raw);
                  if (Number.isFinite(n)) {
                    setForm({ ...form, clash_api_port: n });
                  }
                }}
                disabled={busy || !loaded}
              />
              {fieldErrors.clash_api_port ? (
                <FieldError>{fieldErrors.clash_api_port}</FieldError>
              ) : null}
            </Field>
          </FieldGroup>
          <Field orientation="horizontal" className="w-auto gap-2">
            <Switch
              id="settings-allow-lan"
              size="sm"
              checked={form.allow_lan}
              disabled={busy || !loaded}
              aria-label={t("settings.allowLan")}
              onCheckedChange={(checked) => {
                clearFieldError("mixed_listen");
                setForm({ ...form, allow_lan: checked === true });
              }}
            />
            <FieldLabel htmlFor="settings-allow-lan">
              {t("settings.allowLan")}
            </FieldLabel>
          </Field>
          {form.allow_lan ? (
            <FieldDescription>{t("settings.allowLanDesc")}</FieldDescription>
          ) : null}
          <Field orientation="horizontal" className="w-auto gap-2">
            <Switch
              id="settings-auto-default-rules"
              size="sm"
              checked={form.auto_default_rules}
              disabled={busy || !loaded}
              aria-label={t("settings.autoDefaultRules")}
              onCheckedChange={(checked) => {
                setForm({
                  ...form,
                  auto_default_rules: checked === true,
                });
              }}
            />
            <FieldLabel htmlFor="settings-auto-default-rules">
              {t("settings.autoDefaultRules")}
            </FieldLabel>
          </Field>
          {form.auto_default_rules ? (
            <FieldDescription>
              {t("settings.autoDefaultRulesDesc")}
            </FieldDescription>
          ) : null}
        </div>
      </CardContent>
    </Card>
  );
}
