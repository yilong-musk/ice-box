// SPDX-License-Identifier: GPL-3.0-or-later

import { Switch } from "@/components/ui/switch";
import { t } from "../../lib/i18n";
import type { AppSettings } from "../../api/client";
import { SettingsRow, SettingsSection } from "./SettingsLayout";

/** DNS address-family switches, mirroring Shadowrocket's `ipv6` and
 * `prefer-ipv6` options. TUN capture follows them except where the platform
 * keeps IPv6 disabled (Windows, pinned core #4178). */
export function Ipv6Card({
  form,
  setForm,
  busy,
  loaded,
  tunIpv6Disabled,
}: {
  form: AppSettings;
  setForm: (next: AppSettings) => void;
  busy: boolean;
  loaded: boolean;
  tunIpv6Disabled: boolean;
}) {
  return (
    <SettingsSection title={t("settings.ipv6")}>
      <SettingsRow label={t("settings.ipv6Enable")} htmlFor="settings-ipv6">
        <Switch
          id="settings-ipv6"
          checked={form.ipv6}
          disabled={busy || !loaded}
          aria-label={t("settings.ipv6Enable")}
          onCheckedChange={(checked) => {
            const enabled = checked === true;
            setForm({
              ...form,
              ipv6: enabled,
              prefer_ipv6: enabled ? form.prefer_ipv6 : false,
            });
          }}
        />
      </SettingsRow>
      <SettingsRow
        label={t("settings.preferIpv6")}
        htmlFor="settings-prefer-ipv6"
        description={
          tunIpv6Disabled ? t("settings.ipv6TunIpv4Only") : undefined
        }
      >
        <Switch
          id="settings-prefer-ipv6"
          checked={form.prefer_ipv6}
          disabled={busy || !loaded || !form.ipv6}
          aria-label={t("settings.preferIpv6")}
          onCheckedChange={(checked) => {
            const prefer = checked === true;
            setForm({
              ...form,
              ipv6: prefer ? true : form.ipv6,
              prefer_ipv6: prefer,
            });
          }}
        />
      </SettingsRow>
    </SettingsSection>
  );
}
