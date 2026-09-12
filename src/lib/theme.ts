import { useEffect, useState } from "react";
import { commands, events, type Settings } from "./ipc";
import { resolveLang, translator, type Lang, type T } from "./i18n";

/**
 * Тема и язык всех окон. Тема ставится атрибутом `data-theme` на `<html>`
 * (Tailwind настроен на него), «system» слушает `prefers-color-scheme`.
 * Оверлею дополнительно нужен акриловый тинт на стороне WinAPI — окно красит
 * DWM, а не CSS, поэтому цвет уходит командой `set_overlay_tint`.
 */
export type Theme = "system" | "dark" | "light";

export function isDark(theme: string): boolean {
  if (theme === "dark") return true;
  if (theme === "light") return false;
  return window.matchMedia?.("(prefers-color-scheme: dark)").matches ?? true;
}

export function applyTheme(theme: string) {
  document.documentElement.dataset.theme = isDark(theme) ? "dark" : "light";
}

/**
 * Акцентные пресеты: [основной для тёмной темы, мягкий, основной для светлой,
 * текст по светлому фону, средний, текст на заливке] — «R G B» для
 * CSS-переменных (`tailwind.config.ts`). Порядок id = `ACCENTS` в settings.rs.
 */
export const ACCENTS: Record<string, [string, string, string, string, string, string]> = {
  amber: ["232 163 61", "244 198 126", "200 130 31", "138 90 17", "169 108 20", "26 18 6"],
  blue: ["96 165 250", "147 197 253", "37 99 235", "30 64 175", "29 78 216", "8 18 40"],
  green: ["89 168 11", "140 204 76", "72 138 8", "46 88 5", "61 117 7", "12 24 2"],
  violet: ["167 139 250", "196 181 253", "124 58 237", "76 29 149", "109 40 217", "20 10 40"],
  rose: ["251 113 133", "253 164 175", "225 29 72", "136 19 55", "190 18 60", "40 6 14"],
  teal: ["45 212 191", "94 234 212", "13 148 136", "19 78 74", "15 118 110", "4 26 24"],
};

export function applyAccent(id: string | undefined) {
  const v = ACCENTS[id ?? "amber"] ?? ACCENTS.amber;
  const st = document.documentElement.style;
  ["--amber", "--amber-soft", "--amber-deep", "--amber-ink", "--amber-mid", "--amber-on"].forEach((name, i) =>
    st.setProperty(name, v[i]),
  );
}

/**
 * Настройки + производные тема/язык.
 */
export function useAppShell(): { settings: Settings | null; t: T; lang: Lang; dark: boolean } {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [dark, setDark] = useState(() => isDark("system"));

  useEffect(() => {
    let cancelled = false;
    const apply = (s: Settings) => {
      if (cancelled) return;
      setSettings(s);
      applyTheme(s.theme);
      applyAccent(s.accent);
      const d = isDark(s.theme);
      setDark(d);
    };
    void commands.getSettings().then(apply);
    const un = events.onSettingsChanged(apply);
    // «Системная» тема может смениться, пока окно открыто
    const mq = window.matchMedia?.("(prefers-color-scheme: dark)");
    const onScheme = () => {
      void commands.getSettings().then(apply);
    };
    mq?.addEventListener("change", onScheme);
    return () => {
      cancelled = true;
      mq?.removeEventListener("change", onScheme);
      void un.then((f) => f());
    };
  }, []);

  const lang = resolveLang(settings?.language);
  return { settings, t: translator(lang), lang, dark };
}
