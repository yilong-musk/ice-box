// SPDX-License-Identifier: GPL-3.0-or-later

import assert from "node:assert/strict";
import test from "node:test";
import { SUPPORTED_LANGUAGES } from "../../desktop/src/lib/language.ts";
import { marketingDictionaries, type MarketingKey } from "./i18n.ts";

function placeholders(template: string): string[] {
  return Array.from(template.matchAll(/\{(\w+)\}/g), (match) => match[1]).sort();
}

test("every marketing locale defines every key with matching placeholders", () => {
  const keys = Object.keys(marketingDictionaries.zh) as MarketingKey[];
  for (const lang of SUPPORTED_LANGUAGES) {
    const dict = marketingDictionaries[lang];
    assert.equal(Object.keys(dict).length, keys.length, `${lang} key count`);
    for (const key of keys) {
      assert.ok(dict[key].length > 0, `${lang}:${key} must not be empty`);
      assert.deepEqual(
        placeholders(dict[key]),
        placeholders(marketingDictionaries.zh[key]),
        `${lang}:${key} placeholders`,
      );
    }
  }
});
