// Reactive translations. `t()` reads `i18n.locale`, so any template calling
// it re-renders when the language changes and nothing else does.

import type { Text } from "../types";
import en from "./en";
import zhTW from "./zh-TW";

export type Locale = "en" | "zh-TW";
export type MessageKey = keyof typeof en;
type Params = Record<string, string | number>;

const dictionaries: Record<Locale, Record<MessageKey, string>> = { en, "zh-TW": zhTW };

export const LOCALES: { id: Locale; label: string }[] = [
  { id: "en", label: "English" },
  { id: "zh-TW", label: "正體中文" },
];

class I18n {
  locale = $state<Locale>("en");

  t = (key: MessageKey, params?: Params): string => {
    let text = dictionaries[this.locale][key] ?? en[key] ?? key;
    if (params) {
      for (const [name, value] of Object.entries(params)) text = text.replaceAll(`{${name}}`, String(value));
    }
    return text;
  };

  /** Core-supplied text in the current language, falling back to English. */
  tx = (text: Text | string | null | undefined): string => {
    if (!text) return "";
    if (typeof text === "string") return text;
    return text[this.locale] ?? text[this.locale.split("-")[0]] ?? text.en ?? Object.values(text)[0] ?? "";
  };

  /** Pluralised count for English; Chinese has no plural forms. */
  plural = (one: MessageKey, many: MessageKey, count: number): string => this.t(count === 1 ? one : many, { count });

  number = (value: number, digits = 0): string =>
    value.toLocaleString(this.locale, { maximumFractionDigits: digits, minimumFractionDigits: digits });
}

export const i18n = new I18n();
export const t = i18n.t;
export const tx = i18n.tx;
