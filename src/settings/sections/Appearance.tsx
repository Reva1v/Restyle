import type { Settings } from "../../lib/ipc";
import type { Lang, T } from "../../lib/i18n";
import { ACCENTS } from "../../lib/theme";
import { SectionTitle, Segmented } from "../../ui/kit";

/** Порядок и подписи пресетов акцента (цвета — в `lib/theme.ts`). */
const ACCENT_ORDER = ["amber", "blue", "green", "violet", "rose", "teal"] as const;

/**
 * «Интерфейс»: тема, язык и акцентный цвет. Раньше тема и язык жили внизу
 * «Вставки и истории», где их никто не искал.
 */
export default function Appearance({ t, settings, apply }: { t: T; settings: Settings; apply: (s: Settings) => void }) {
  return (
    <div className="flex flex-col gap-5">
      <SectionTitle title={t("theme.title")} />
      <div className="flex">
        <Segmented
          value={settings.theme as "system" | "light" | "dark"}
          onChange={(v) => apply({ ...settings, theme: v })}
          options={[
            { value: "system", label: t("theme.system") },
            { value: "light", label: t("theme.light") },
            { value: "dark", label: t("theme.dark") },
          ]}
        />
      </div>

      <div className="h-px bg-black/[.08] dark:bg-white/[.07]" />

      <SectionTitle title={t("accent.title")} hint={t("accent.hint")} />
      <div className="flex flex-wrap gap-2.5">
        {ACCENT_ORDER.map((id) => {
          const [dark, , light] = ACCENTS[id];
          const active = (settings.accent || "amber") === id;
          return (
            <button
              key={id}
              type="button"
              onClick={() => apply({ ...settings, accent: id })}
              className={`flex cursor-pointer items-center gap-2.5 rounded-lg border bg-white py-2 pl-2 pr-3.5 text-[12.5px] transition-colors dark:bg-white/[.04] ${
                active
                  ? "border-amber-deep text-paper-text shadow-[0_0_0_2px_rgb(var(--amber-deep)/.13)] dark:border-amber dark:text-ink-text"
                  : "border-black/[.12] text-paper-mid hover:border-black/25 dark:border-white/[.08] dark:text-ink-text/70 dark:hover:border-white/20"
              }`}
            >
              {/* Кружок: половина — оттенок светлой темы, половина — тёмной. */}
              <span
                className="h-5 w-5 flex-none rounded-full"
                style={{ background: `linear-gradient(135deg, rgb(${light}) 0 50%, rgb(${dark}) 50% 100%)` }}
              />
              {t(`accent.${id}`)}
              {active && <span className="text-[11px] text-amber-ink dark:text-amber">✓</span>}
            </button>
          );
        })}
      </div>

      <div className="h-px bg-black/[.08] dark:bg-white/[.07]" />

      <SectionTitle title={t("lang.title")} />
      <div className="flex">
        <Segmented
          value={settings.language as Lang | "system"}
          onChange={(v) => apply({ ...settings, language: v })}
          options={[
            { value: "system", label: t("lang.system") },
            { value: "ru", label: "Русский" },
            { value: "uk", label: "Українська" },
            { value: "en", label: "English" },
          ]}
        />
      </div>
    </div>
  );
}
