/** Языки DeepL: коды как в `settings.rs` (TRANSLATE_LANGS). */
export const LANGS: { value: string; label: string }[] = [
  { value: "EN-US", label: "English (US)" },
  { value: "EN-GB", label: "English (UK)" },
  { value: "RU", label: "Русский" },
  { value: "UK", label: "Українська" },
  { value: "DE", label: "Deutsch" },
  { value: "FR", label: "Français" },
  { value: "ES", label: "Español" },
  { value: "IT", label: "Italiano" },
  { value: "PL", label: "Polski" },
  { value: "PT-PT", label: "Português" },
  { value: "TR", label: "Türkçe" },
  { value: "ZH", label: "中文" },
  { value: "JA", label: "日本語" },
];

/** Подпись языка по коду; неизвестный код — сам код. */
export function langLabel(code: string): string {
  return LANGS.find((l) => l.value === code)?.label ?? code;
}
