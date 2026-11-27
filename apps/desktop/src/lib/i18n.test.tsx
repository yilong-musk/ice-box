// SPDX-License-Identifier: GPL-3.0-or-later

import { Activity } from "react";
import { act, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import {
  LANGUAGE_CHANGE_EVENT,
  SUPPORTED_LANGUAGES,
  dictionaries,
  formatLocale,
  isRtlLanguage,
  persistLanguagePreference,
  resolveLocaleTag,
  t,
  useLanguagePreference,
  type MessageKey,
} from "./i18n";

function Label() {
  const { resolved } = useLanguagePreference();
  return <span data-testid={`label-${resolved}`}>{t("nodes.select")}</span>;
}

function Pages({ showSecond }: { showSecond: boolean }) {
  return (
    <>
      <Activity mode={showSecond ? "hidden" : "visible"}>
        <Label />
      </Activity>
      <Activity mode={showSecond ? "visible" : "hidden"}>
        <Label />
      </Activity>
    </>
  );
}

function placeholders(template: string): string[] {
  return Array.from(template.matchAll(/\{(\w+)\}/g), (match) => match[1]).sort();
}

describe("useLanguagePreference", () => {
  it("does not revert the language when a hidden page becomes visible", () => {
    act(() => persistLanguagePreference("zh"));
    const { rerender } = render(<Pages showSecond={false} />);

    // The second page is hidden (its effects are torn down) while the
    // language changes from the visible one.
    act(() => persistLanguagePreference("en"));
    rerender(<Pages showSecond />);

    expect(document.documentElement.lang).toBe("en");
    expect(document.documentElement.dir).toBe("ltr");
    expect(t("nodes.select")).toBe("Select");
    expect(screen.getAllByTestId("label-en")).toHaveLength(2);
    expect(screen.queryByTestId("label-zh")).toBeNull();
  });

  it("follows a language change event even while every page is hidden", () => {
    act(() => persistLanguagePreference("zh"));
    const { rerender } = render(
      <Activity mode="hidden">
        <Label />
      </Activity>,
    );

    act(() => {
      window.dispatchEvent(
        new CustomEvent(LANGUAGE_CHANGE_EVENT, { detail: "en" }),
      );
    });
    rerender(
      <Activity mode="visible">
        <Label />
      </Activity>,
    );

    expect(screen.getByTestId("label-en")).toHaveTextContent("Select");
  });

  it("ignores a missing preference instead of breaking t()", () => {
    act(() => persistLanguagePreference("zh"));
    act(() =>
      persistLanguagePreference(undefined as unknown as "zh"),
    );
    expect(t("nodes.select")).toBe("选用");
  });

  it("sets rtl direction for Persian and Arabic and restores ltr for other locales", () => {
    for (const lang of SUPPORTED_LANGUAGES) {
      act(() => persistLanguagePreference(lang));
      expect(document.documentElement.lang).toBe(lang);
      expect(document.documentElement.dir).toBe(
        isRtlLanguage(lang) ? "rtl" : "ltr",
      );
    }
    expect(isRtlLanguage("fa")).toBe(true);
    expect(isRtlLanguage("ar")).toBe(true);
    expect(isRtlLanguage("en")).toBe(false);
    act(() => persistLanguagePreference("zh"));
  });
});

describe("locale resolution and dictionaries", () => {
  it("resolves locale tags across all 16 supported languages", () => {
    expect(resolveLocaleTag("zh")).toBe("zh");
    expect(resolveLocaleTag("zh-CN")).toBe("zh");
    expect(resolveLocaleTag("zh-SG")).toBe("zh");
    expect(resolveLocaleTag("zh-Hans-CN")).toBe("zh");
    expect(resolveLocaleTag("zh-TW")).toBe("zh-TW");
    expect(resolveLocaleTag("zh_HK.UTF-8")).toBe("zh-TW");
    expect(resolveLocaleTag("zh-MO")).toBe("zh-TW");
    expect(resolveLocaleTag("zh-Hant")).toBe("zh-TW");
    expect(resolveLocaleTag("ru")).toBe("ru");
    expect(resolveLocaleTag("ru-RU")).toBe("ru");
    expect(resolveLocaleTag("ja")).toBe("ja");
    expect(resolveLocaleTag("ja-JP")).toBe("ja");
    expect(resolveLocaleTag("fa")).toBe("fa");
    expect(resolveLocaleTag("fa-IR")).toBe("fa");
    expect(resolveLocaleTag("fa-AF")).toBe("fa");
    expect(resolveLocaleTag("ar")).toBe("ar");
    expect(resolveLocaleTag("ar-SA")).toBe("ar");
    expect(resolveLocaleTag("ko")).toBe("ko");
    expect(resolveLocaleTag("ko-KR")).toBe("ko");
    expect(resolveLocaleTag("tr")).toBe("tr");
    expect(resolveLocaleTag("tr-TR")).toBe("tr");
    expect(resolveLocaleTag("vi")).toBe("vi");
    expect(resolveLocaleTag("vi-VN")).toBe("vi");
    expect(resolveLocaleTag("id")).toBe("id");
    expect(resolveLocaleTag("in-ID")).toBe("id");
    expect(resolveLocaleTag("es")).toBe("es");
    expect(resolveLocaleTag("es-MX")).toBe("es");
    expect(resolveLocaleTag("de")).toBe("de");
    expect(resolveLocaleTag("de-DE")).toBe("de");
    expect(resolveLocaleTag("fr")).toBe("fr");
    expect(resolveLocaleTag("fr-CA")).toBe("fr");
    expect(resolveLocaleTag("pt")).toBe("pt-BR");
    expect(resolveLocaleTag("pt-BR")).toBe("pt-BR");
    expect(resolveLocaleTag("pt-PT")).toBe("pt-BR");
    expect(resolveLocaleTag("uk")).toBe("uk");
    expect(resolveLocaleTag("uk-UA")).toBe("uk");
    expect(resolveLocaleTag("en-US")).toBe("en");
    expect(resolveLocaleTag("nl-NL")).toBe("en");
    expect(resolveLocaleTag("")).toBe("en");
  });

  it("maps every supported language to a valid BCP-47 formatting locale", () => {
    for (const lang of SUPPORTED_LANGUAGES) {
      expect(formatLocale(lang)).toMatch(/^[a-z]{2}-[A-Z]{2}$/);
    }
    expect(formatLocale("zh")).toBe("zh-CN");
    expect(formatLocale("zh-TW")).toBe("zh-TW");
    expect(formatLocale("fa")).toBe("fa-IR");
    expect(formatLocale("ar")).toBe("ar-SA");
    expect(formatLocale("pt-BR")).toBe("pt-BR");
    expect(formatLocale("uk")).toBe("uk-UA");
  });

  it("keeps every message key non-empty with identical placeholders in all locales", () => {
    const keys = Object.keys(dictionaries.zh) as MessageKey[];
    expect(SUPPORTED_LANGUAGES).toHaveLength(16);
    for (const lang of SUPPORTED_LANGUAGES) {
      const dict = dictionaries[lang];
      expect(Object.keys(dict)).toHaveLength(keys.length);
      for (const key of keys) {
        expect(dict[key].length, `${lang}:${key}`).toBeGreaterThan(0);
        expect(placeholders(dict[key]), `${lang}:${key}`).toEqual(
          placeholders(dictionaries.zh[key]),
        );
      }
    }
  });

  it("isolates LTR rate and memory placeholders inside RTL templates", () => {
    for (const lang of ["ar", "fa"] as const) {
      const dict = dictionaries[lang];
      expect(dict["traffic.peak"]).toContain("\u2066{rate}\u2069");
      expect(dict["home.memory.tooltip"]).toContain("\u2066{core}\u2069");
      expect(dict["home.memory.tooltip"]).toContain("\u2066{app}\u2069");
      expect(dict["home.memory.tooltipAppOnly"]).toContain("\u2066{app}\u2069");
      expect(dict["home.memory.tooltipUnreadable"]).toContain("\u2066{app}\u2069");
    }
  });
});
