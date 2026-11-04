// SPDX-License-Identifier: GPL-3.0-or-later

import { type Dispatch, type SetStateAction } from "react";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import {
  formatPortValidationError,
} from "../../lib/listenValidation";
import { t } from "../../lib/i18n";
import type { AppSettings } from "../../api/client";
import { SettingsRow, SettingsSection } from "./SettingsLayout";

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
  const disabled = busy || !loaded;
  const listenError = fieldErrors.mixed_listen;
  const portError = fieldErrors.mixed_port;

  /** An empty port box is reported at once; the save path skips NaN. */
  function onPortChange(raw: string) {
    clearFieldError("mixed_port");
    if (raw.trim() === "") {
      setFieldErrors((prev) => ({
        ...prev,
        mixed_port: formatPortValidationError(t("settings.mixedPort")),
      }));
      setForm({ ...form, mixed_port: Number.NaN });
      return;
    }
    const n = Number(raw);
    if (Number.isFinite(n)) {
      setForm({ ...form, mixed_port: n });
    }
  }

  return (
    <SettingsSection title={t("settings.inbound")}>
      {/* Address and port share a row: `[host] : [port]`. */}
      <SettingsRow
        label={t("settings.mixedProxy")}
        htmlFor="settings-mixed-listen"
        description={t("settings.mixedProxyHint")}
        error={
          listenError || portError ? (
            <>
              {listenError ? <span className="block">{listenError}</span> : null}
              {portError ? <span className="block">{portError}</span> : null}
            </>
          ) : null
        }
      >
        <Input
          id="settings-mixed-listen"
          aria-label={t("settings.mixedListen")}
          className="h-8 w-36 font-mono text-[13px]"
          value={form.mixed_listen}
          aria-invalid={!!listenError || undefined}
          onChange={(e) => {
            clearFieldError("mixed_listen");
            setForm({ ...form, mixed_listen: e.target.value });
          }}
          // Allow LAN pins the bind address to all interfaces.
          disabled={disabled || form.allow_lan}
        />
        <span className="text-muted-foreground" aria-hidden="true">
          :
        </span>
        <Input
          id="settings-mixed-port"
          aria-label={t("settings.mixedPort")}
          type="number"
          min={1024}
          max={65535}
          className="h-8 w-24 font-mono text-[13px]"
          value={Number.isFinite(form.mixed_port) ? form.mixed_port : ""}
          aria-invalid={!!portError || undefined}
          onChange={(e) => onPortChange(e.target.value)}
          disabled={disabled}
        />
      </SettingsRow>
      <SettingsRow
        label={t("settings.allowLan")}
        htmlFor="settings-allow-lan"
        description={t("settings.allowLanHint")}
      >
        <Switch
          id="settings-allow-lan"
          checked={form.allow_lan}
          disabled={disabled}
          aria-label={t("settings.allowLan")}
          onCheckedChange={(checked) => {
            clearFieldError("mixed_listen");
            setForm({ ...form, allow_lan: checked === true });
          }}
        />
      </SettingsRow>
      <SettingsRow
        label={t("settings.autoDefaultRules")}
        htmlFor="settings-auto-default-rules"
      >
        <Switch
          id="settings-auto-default-rules"
          checked={form.auto_default_rules}
          disabled={disabled}
          aria-label={t("settings.autoDefaultRules")}
          onCheckedChange={(checked) => {
            setForm({ ...form, auto_default_rules: checked === true });
          }}
        />
      </SettingsRow>
      <SettingsRow
        label={t("settings.probeExitIp")}
        htmlFor="settings-probe-exit-ip"
      >
        <Switch
          id="settings-probe-exit-ip"
          checked={form.probe_exit_ip}
          disabled={disabled}
          aria-label={t("settings.probeExitIp")}
          onCheckedChange={(checked) => {
            setForm({ ...form, probe_exit_ip: checked === true });
          }}
        />
      </SettingsRow>
    </SettingsSection>
  );
}
