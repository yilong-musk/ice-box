// SPDX-License-Identifier: GPL-3.0-or-later

import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Field, FieldDescription, FieldLabel } from "@/components/ui/field";
import { Switch } from "@/components/ui/switch";
import { t } from "../../lib/i18n";
import type { AppSettings } from "../../api/client";

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
    <Card
      size="sm"
      className="w-full shrink-0 data-[size=sm]:[--card-spacing:--spacing(2)]"
    >
      <CardHeader>
        <CardTitle>{t("settings.ipv6")}</CardTitle>
      </CardHeader>
      <CardContent>
        <div className="flex flex-col gap-3">
          <Field orientation="horizontal" className="w-auto gap-2">
            <Switch
              id="settings-ipv6"
              size="sm"
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
            <FieldLabel htmlFor="settings-ipv6">
              {t("settings.ipv6Enable")}
            </FieldLabel>
          </Field>
          <Field orientation="horizontal" className="w-auto gap-2">
            <Switch
              id="settings-prefer-ipv6"
              size="sm"
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
            <FieldLabel htmlFor="settings-prefer-ipv6">
              {t("settings.preferIpv6")}
            </FieldLabel>
          </Field>
          {tunIpv6Disabled ? (
            <FieldDescription>{t("settings.ipv6TunIpv4Only")}</FieldDescription>
          ) : null}
        </div>
      </CardContent>
    </Card>
  );
}
