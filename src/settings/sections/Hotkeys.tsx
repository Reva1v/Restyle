import type { Settings } from "../../lib/ipc";
import type { T } from "../../lib/i18n";
import { EMPTY_COMBO, comboOrNull } from "../../lib/hotkey";
import HotkeyField from "../HotkeyField";
import { SectionTitle, Select, ToggleRow } from "../../ui/kit";

/** Пороги удержания: миллисекунды, которые есть смысл выбирать. */
const RING_MS = [200, 300, 400, 500];
const OPEN_MS = [400, 600, 800, 1000];

/**
 * «Хоткеи» (макет, раздел 03). Три блока: основные комбинации, разбор
 * короткого и долгого нажатия и отдельный список быстрых стилей — он
 * дублирует поле из раздела «Стили» намеренно: все комбинации должны быть
 * видны в одном месте.
 * Конфликты приходят с бэкенда парами (первым — то действие, что сработает).
 */
export default function Hotkeys({
  t,
  settings,
  apply,
  conflicts,
  hookOk,
  lang,
}: {
  t: T;
  settings: Settings;
  apply: (s: Settings) => void;
  conflicts: [string, string][];
  hookOk: boolean;
  lang: string;
}) {
  const actionName = (id: string) => {
    if (id === "main") return t("hotkeys.main");
    if (id === "undo") return t("hotkeys.undo");
    if (id === "layout") return t("hotkeys.layout");
    const styleId = id.replace(/^style:/, "");
    return settings.styles.find((s) => s.id === styleId)?.name ?? styleId;
  };
  const conflictOf = (id: string): string | null => {
    const loses = conflicts.find(([, b]) => b === id);
    if (loses) return t("hotkeys.taken", { n: actionName(loses[0]) });
    const wins = conflicts.find(([a]) => a === id);
    return wins ? t("hotkeys.dup", { n: actionName(wins[1]) }) : null;
  };
  const setStyleHotkey = (id: string, c: Settings["mainHotkey"] | null) =>
    apply({ ...settings, styles: settings.styles.map((x) => (x.id === id ? { ...x, hotkey: c } : x)) });

  return (
    <div className="flex h-full flex-col gap-5 overflow-y-auto pr-1">
      <SectionTitle title={t("hotkeys.title")} hint={t("hotkeys.hint")} />
      <div className="flex flex-col gap-2">
        <HotkeyRow
          label={t("hotkeys.main")}
          warn={conflictOf("main")}
          value={settings.mainHotkey}
          lang={lang}
          onChange={(c) => apply({ ...settings, mainHotkey: c ?? EMPTY_COMBO })}
        />
        <HotkeyRow
          label={t("hotkeys.undo")}
          warn={conflictOf("undo")}
          value={comboOrNull(settings.undoHotkey)}
          allowEmpty
          lang={lang}
          onChange={(c) => apply({ ...settings, undoHotkey: c ?? EMPTY_COMBO })}
        />
        <HotkeyRow
          label={t("hotkeys.layout")}
          sub={t("hotkeys.layoutSub")}
          warn={conflictOf("layout")}
          value={comboOrNull(settings.layoutHotkey)}
          allowEmpty
          lang={lang}
          onChange={(c) => apply({ ...settings, layoutHotkey: c ?? EMPTY_COMBO })}
        />
      </div>

      <div className="h-px bg-black/[.08] dark:bg-white/[.07]" />

      <SectionTitle title={t("hotkeys.longTitle")} hint={t("hotkeys.longHint")} />
      <div className="flex flex-col gap-2">
        <ToggleRow
          title={t("hotkeys.longEnable")}
          hint={settings.longPress ? undefined : t("hotkeys.longOff")}
          checked={settings.longPress}
          onChange={(v) =>
            apply({
              ...settings,
              longPress: v,
              // Без стиля короткому нажатию нечего применять — берём первый.
              quickStyle: v && !settings.quickStyle ? (settings.styles[0]?.id ?? "") : settings.quickStyle,
            })
          }
        />
        {settings.longPress && (
          <>
            <div className="flex items-center gap-3.5 rounded-lg border border-black/10 bg-white p-3.5 dark:border-white/[.08] dark:bg-white/[.04]">
              <span className="flex-1 text-[13px] text-paper-text dark:text-ink-text">{t("hotkeys.quickStyle")}</span>
              <Select
                className="w-[220px]"
                value={settings.quickStyle}
                onChange={(v) => apply({ ...settings, quickStyle: v })}
                options={settings.styles.map((s) => ({ value: s.id, label: s.name }))}
              />
            </div>
            <div className="flex gap-2">
              <MsRow
                label={t("hotkeys.ring")}
                unit={t("hotkeys.ms")}
                value={settings.longPressRingMs}
                options={RING_MS}
                onChange={(v) => apply({ ...settings, longPressRingMs: v })}
              />
              <MsRow
                label={t("hotkeys.open")}
                unit={t("hotkeys.ms")}
                value={settings.longPressOpenMs}
                options={OPEN_MS}
                onChange={(v) => apply({ ...settings, longPressOpenMs: v })}
              />
            </div>
          </>
        )}
      </div>

      <div className="h-px bg-black/[.08] dark:bg-white/[.07]" />

      <SectionTitle title={t("hotkeys.quickTitle")} hint={t("hotkeys.quickHint")} />
      <div className="flex flex-col gap-2">
        {settings.styles.map((s) => (
          <HotkeyRow
            key={s.id}
            label={s.name}
            sub={s.kind === "translate" ? `${t("styles.kindTranslate")} · ${s.targetLang}` : undefined}
            warn={conflictOf(`style:${s.id}`)}
            value={s.hotkey}
            allowEmpty
            lang={lang}
            onChange={(c) => setStyleHotkey(s.id, c)}
          />
        ))}
      </div>

      <div
        className={`mt-auto flex flex-none items-center gap-3 rounded-lg border p-3 ${
          hookOk
            ? "border-black/[.07] bg-black/[.035] dark:border-white/[.07] dark:bg-white/[.03]"
            : "border-danger/30 bg-danger/10"
        }`}
      >
        <span className={`h-1.5 w-1.5 flex-none rounded-full ${hookOk ? "bg-ok" : "bg-danger"}`} />
        <span className="text-xs leading-relaxed text-paper-mid dark:text-ink-text/65">{t("hotkeys.hookOk")}</span>
      </div>
    </div>
  );
}

function MsRow({
  label,
  unit,
  value,
  options,
  onChange,
}: {
  label: string;
  unit: string;
  value: number;
  options: number[];
  onChange: (v: number) => void;
}) {
  return (
    <div className="flex flex-1 items-center gap-3 rounded-lg border border-black/10 bg-white p-3.5 dark:border-white/[.08] dark:bg-white/[.04]">
      <span className="flex-1 text-[12.5px] text-paper-text dark:text-ink-text">{label}</span>
      <Select
        className="w-[96px]"
        value={String(value)}
        onChange={(v) => onChange(Number(v))}
        options={options.map((ms) => ({ value: String(ms), label: `${ms} ${unit}` }))}
      />
    </div>
  );
}

function HotkeyRow({
  label,
  sub,
  warn,
  value,
  onChange,
  allowEmpty = false,
  lang,
}: {
  label: string;
  sub?: string;
  warn: string | null;
  value: Settings["mainHotkey"] | null;
  onChange: (c: Settings["mainHotkey"] | null) => void;
  allowEmpty?: boolean;
  lang: string;
}) {
  return (
    <div
      className={`flex items-center gap-3 rounded-lg border bg-white p-3.5 dark:bg-white/[.04] ${
        warn ? "border-danger/55" : "border-black/10 dark:border-white/[.08]"
      }`}
    >
      <span className="flex min-w-0 flex-1 flex-col gap-[3px]">
        <span className="truncate text-[13px] leading-tight text-paper-text dark:text-ink-text">{label}</span>
        {sub && <span className="text-[11px] leading-tight text-paper-dim dark:text-ink-text/45">{sub}</span>}
        {warn && <span className="text-[11.5px] leading-tight text-danger-ink dark:text-danger">{warn}</span>}
      </span>
      <HotkeyField lang={lang} value={value} onChange={onChange} allowEmpty={allowEmpty} />
    </div>
  );
}
