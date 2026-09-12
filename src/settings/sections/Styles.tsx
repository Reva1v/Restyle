import { useEffect, useState } from "react";
import { commands, type Settings, type Style, type StyleKind } from "../../lib/ipc";
import type { T } from "../../lib/i18n";
import { comboLabel } from "../../lib/hotkey";
import HotkeyField from "../HotkeyField";
import { Button, Caption, DraftField, Select, Toggle } from "../../ui/kit";

/** Языки DeepL: коды как в `settings.rs` (TRANSLATE_LANGS). */
const LANGS: { value: string; label: string }[] = [
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

/**
 * «Стили» (макет, раздел 03): слева список, справа редактор выбранного.
 * Список разделён на встроенные и свои и переупорядочивается перетаскиванием —
 * порядок виден в панели (первые пять чипов) и в цифрах 1–9.
 * Правки применяются сразу, поэтому «Сохранить» тут нет: у встроенных есть
 * «Вернуть», у своих — «Удалить».
 */
export default function Styles({
  t,
  settings,
  apply,
  lang,
}: {
  t: T;
  settings: Settings;
  apply: (s: Settings) => void;
  lang: string;
}) {
  const [selected, setSelected] = useState(0);
  const [defaults, setDefaults] = useState<Style[]>([]);
  const [drag, setDrag] = useState<number | null>(null);
  /** Строка, над которой сейчас висит перетаскиваемая: рисуем линию вставки. */
  const [over, setOver] = useState<number | null>(null);
  useEffect(() => {
    void commands.defaultStyles().then(setDefaults);
  }, []);

  const style = settings.styles[Math.min(selected, settings.styles.length - 1)];
  const def = defaults.find((d) => d.id === style?.id);
  const changed =
    def &&
    (def.name !== style.name ||
      def.instruction !== style.instruction ||
      def.screenshot !== style.screenshot ||
      def.kind !== style.kind ||
      def.targetLang !== style.targetLang ||
      def.postStyle !== style.postStyle);

  function patch(p: Partial<Style>) {
    apply({ ...settings, styles: settings.styles.map((s, i) => (i === selected ? { ...s, ...p } : s)) });
  }

  function add() {
    const next: Style = {
      id: "",
      name: t("styles.new"),
      instruction: "",
      hotkey: null,
      builtin: false,
      screenshot: true,
      kind: "prompt",
      targetLang: "",
      postStyle: "",
    };
    apply({ ...settings, styles: [...settings.styles, next] });
    setSelected(settings.styles.length);
  }

  function remove() {
    apply({ ...settings, styles: settings.styles.filter((_, i) => i !== selected) });
    setSelected((i) => Math.max(0, i - 1));
  }

  /** Удалённые встроенные — обратно в список, в исходном порядке. */
  function restoreBuiltin() {
    const have = new Set(settings.styles.map((s) => s.id));
    const missing = defaults.filter((d) => !have.has(d.id));
    if (missing.length === 0) return;
    const builtinNow = settings.styles.filter((s) => s.builtin);
    const customNow = settings.styles.filter((s) => !s.builtin);
    apply({ ...settings, styles: [...builtinNow, ...missing, ...customNow] });
  }

  /** Перенос строки: порядок в массиве и есть порядок чипов и цифр 1–9. */
  function move(from: number, to: number) {
    if (from === to || to < 0 || to >= settings.styles.length) return;
    // Внутри своей группы: иначе встроенные и свои перемешаются, а нумерация
    // в секциях пойдёт вразнобой (…7, 9 в одной и 8, 10 в другой).
    if (settings.styles[from].builtin !== settings.styles[to].builtin) return;
    const next = [...settings.styles];
    const [item] = next.splice(from, 1);
    next.splice(to, 0, item);
    apply({ ...settings, styles: next });
    setSelected(to);
  }

  /** Куда встанет строка: линия сверху, если тащим снизу вверх, и наоборот. */
  function dropLine(i: number): string {
    if (drag === null || over !== i || drag === i) return "";
    return drag > i
      ? "shadow-[inset_0_2px_0_0_rgb(var(--amber-deep))] dark:shadow-[inset_0_2px_0_0_rgb(var(--amber))]"
      : "shadow-[inset_0_-2px_0_0_rgb(var(--amber-deep))] dark:shadow-[inset_0_-2px_0_0_rgb(var(--amber))]";
  }

  const rows = settings.styles.map((s, i) => ({ s, i }));
  const builtin = rows.filter((r) => r.s.builtin);
  const custom = rows.filter((r) => !r.s.builtin);

  const row = ({ s, i }: { s: Style; i: number }) => (
    <div
      key={`${s.id}-${i}`}
      draggable
      onDragStart={(e) => {
        setDrag(i);
        // Chromium не начнёт перетаскивание без данных в dataTransfer.
        e.dataTransfer.setData("text/plain", String(i));
        e.dataTransfer.effectAllowed = "move";
      }}
      onDragOver={(e) => {
        e.preventDefault();
        e.dataTransfer.dropEffect = "move";
        if (over !== i) setOver(i);
      }}
      onDragLeave={() => setOver((o) => (o === i ? null : o))}
      onDrop={(e) => {
        e.preventDefault();
        const from = drag ?? Number(e.dataTransfer.getData("text/plain"));
        if (!Number.isNaN(from)) move(from, i);
        setDrag(null);
        setOver(null);
      }}
      onDragEnd={() => {
        setDrag(null);
        setOver(null);
      }}
      onClick={() => setSelected(i)}
      className={`group flex select-none items-center gap-2.5 rounded-[7px] px-2.5 py-2.5 text-left transition-colors ${
        drag === null ? "cursor-pointer" : "cursor-grabbing"
      } ${
        i === selected
          ? "border border-amber-deep bg-white shadow-card dark:border-amber dark:bg-white/[.07]"
          : "border border-transparent hover:bg-black/[.04] dark:hover:bg-white/[.05]"
      } ${drag === i ? "opacity-40" : ""} ${dropLine(i)}`}
    >
      <span className="font-mono text-[11px] text-paper-soft dark:text-ink-text/45">{i + 1}</span>
      <span className="flex-1 truncate text-[12.5px] text-[#33363c] dark:text-ink-text/90">{s.name}</span>
      {s.kind === "translate" && (
        <span className="font-mono text-[9.5px] text-paper-dim dark:text-ink-text/45">{s.targetLang}</span>
      )}
      {s.hotkey && (
        <span className="font-mono text-[10px] text-paper-dim dark:text-ink-text/45">{comboLabel(s.hotkey)}</span>
      )}
      {/* Стрелки — на случай, когда таскать мышью неудобно */}
      <span className="flex flex-none gap-0.5 opacity-0 transition-opacity group-hover:opacity-100">
        <Arrow label={t("styles.up")} onClick={() => move(i, i - 1)} glyph="▲" />
        <Arrow label={t("styles.down")} onClick={() => move(i, i + 1)} glyph="▼" />
      </span>
    </div>
  );

  return (
    <div className="flex h-full min-h-0">
      <div className="flex w-[268px] flex-none flex-col border-r border-black/[.08] dark:border-white/[.07]">
        <div className="flex items-baseline gap-2 px-4 pb-1 pt-4">
          <span className="text-[13px] font-semibold text-paper-text dark:text-ink-text">{t("styles.title")}</span>
          <span className="font-mono text-[11px] text-paper-dim dark:text-ink-text/50">{settings.styles.length}</span>
          <button
            type="button"
            onClick={add}
            className="ml-auto text-[11.5px] font-medium text-amber-ink hover:underline dark:text-amber"
          >
            {t("common.add")}
          </button>
        </div>
        <div className="flex items-baseline gap-2 px-4">
          {defaults.some((d) => !settings.styles.some((s) => s.id === d.id)) && (
            <button
              type="button"
              onClick={restoreBuiltin}
              className="text-[11.5px] font-medium text-amber-ink hover:underline dark:text-amber"
            >
              {t("styles.restoreBuiltin")}
            </button>
          )}
        </div>
        <span className="px-4 pb-2 text-[10.5px] leading-snug text-paper-dim dark:text-ink-text/45">
          {t("styles.chips")}
        </span>
        <div className="flex min-h-0 flex-1 flex-col gap-0.5 overflow-y-auto px-2 pb-2">
          <GroupLabel>{t("styles.groupBuiltin")}</GroupLabel>
          {builtin.map(row)}
          <GroupLabel>{t("styles.groupCustom")}</GroupLabel>
          {custom.length === 0 && (
            <span className="px-2.5 py-1.5 text-[11.5px] text-paper-dim dark:text-ink-text/40">{t("common.empty")}</span>
          )}
          {custom.map(row)}
        </div>
      </div>

      {style && (
        <div className="flex min-w-0 flex-1 flex-col gap-4 overflow-y-auto p-5">
          <div className="flex flex-wrap gap-3.5">
            <label className="flex min-w-[140px] flex-1 basis-[140px] flex-col gap-1.5">
              <Caption>{t("styles.name")}</Caption>
              <DraftField className="h-[34px] py-0" value={style.name} onCommit={(v) => patch({ name: v })} />
            </label>
            <label className="flex w-[150px] flex-none flex-col gap-1.5">
              <Caption>{t("styles.kind")}</Caption>
              <Select<StyleKind>
                className="h-[34px]"
                value={style.kind}
                onChange={(v) =>
                  patch({ kind: v, targetLang: v === "translate" && !style.targetLang ? "EN-US" : style.targetLang })
                }
                options={[
                  { value: "prompt", label: t("styles.kindPrompt") },
                  { value: "translate", label: t("styles.kindTranslate") },
                ]}
              />
            </label>
          </div>

          {style.kind === "translate" ? (
            <div className="flex flex-col gap-4">
              <div className="flex flex-wrap gap-3.5">
                <label className="flex min-w-[140px] flex-1 basis-[180px] flex-col gap-1.5">
                  <Caption>{t("styles.lang")}</Caption>
                  <Select
                    className="h-[34px]"
                    value={style.targetLang || "EN-US"}
                    onChange={(v) => patch({ targetLang: v })}
                    options={LANGS}
                  />
                </label>
                <label className="flex min-w-[150px] flex-1 basis-[180px] flex-col gap-1.5">
                  <Caption>{t("styles.post")}</Caption>
                  <Select
                    className="h-[34px]"
                    value={style.postStyle}
                    onChange={(v) => patch({ postStyle: v })}
                    options={[
                      { value: "", label: t("styles.postNone") },
                      ...settings.styles
                        .filter((s) => s.kind === "prompt" && s.id !== style.id)
                        .map((s) => ({ value: s.id, label: s.name })),
                    ]}
                  />
                </label>
              </div>
              <span className="text-[11.5px] leading-snug text-paper-dim dark:text-ink-text/55">
                {t("styles.postHint")}
              </span>
              <label className="flex min-h-[120px] flex-1 flex-col gap-1.5">
                <Caption>{t("styles.instruction")}</Caption>
                <DraftField
                  multiline
                  className="min-h-0 flex-1 resize-none leading-relaxed"
                  placeholder={t("styles.placeholder")}
                  value={style.instruction}
                  onCommit={(v) => patch({ instruction: v })}
                />
              </label>
            </div>
          ) : (
            <label className="flex min-h-[120px] flex-1 flex-col gap-1.5">
              <Caption>{t("styles.instruction")}</Caption>
              <DraftField
                multiline
                className="min-h-0 flex-1 resize-none leading-relaxed"
                placeholder={t("styles.placeholder")}
                value={style.instruction}
                onCommit={(v) => patch({ instruction: v })}
              />
            </label>
          )}

          <div className="flex flex-wrap gap-3.5">
            <div className="flex min-w-[140px] flex-1 basis-[180px] flex-col gap-1.5">
              <Caption>{t("styles.hotkey")}</Caption>
              <HotkeyField fill lang={lang} value={style.hotkey} allowEmpty onChange={(c) => patch({ hotkey: c })} />
            </div>
            <div className="flex min-w-[124px] flex-1 basis-[140px] flex-col gap-1.5">
              <Caption>{t("styles.screenshot")}</Caption>
              <button
                type="button"
                onClick={() => patch({ screenshot: !style.screenshot })}
                className="flex h-[34px] items-center gap-2 rounded-md border border-black/[.13] bg-white px-3 text-left dark:border-white/10 dark:bg-white/[.05]"
              >
                <Toggle checked={style.screenshot} onChange={(v) => patch({ screenshot: v })} size="sm" />
                <span className="text-xs text-[#33363c] dark:text-ink-text/80">
                  {style.screenshot ? t("common.on") : t("common.off")}
                </span>
              </button>
            </div>
          </div>

          <div className="flex flex-wrap items-center gap-x-2.5 gap-y-2">
            <span className="font-mono text-[11px] text-paper-soft dark:text-ink-text/40">
              id: {style.id || "…"}
              {style.builtin ? ` · ${t("styles.builtin")}` : ""}
            </span>
            <span className="ml-auto flex items-center gap-2">
              {style.builtin && (
                <Button
                  disabled={!changed}
                  onClick={() =>
                    def &&
                    patch({
                      name: def.name,
                      instruction: def.instruction,
                      screenshot: def.screenshot,
                      kind: def.kind,
                      targetLang: def.targetLang,
                      postStyle: def.postStyle,
                    })
                  }
                >
                  {t("common.reset")}
                </Button>
              )}
              {/* Удалять можно и встроенные: вернуть их можно ссылкой над списком. */}
              <Button kind="danger" onClick={remove}>
                {t("styles.delete")}
              </Button>
            </span>
          </div>
        </div>
      )}
    </div>
  );
}

function GroupLabel({ children }: { children: React.ReactNode }) {
  return (
    <span className="px-2.5 pb-1 pt-2.5 font-mono text-[10px] uppercase tracking-[.09em] text-paper-soft dark:text-ink-text/40">
      {children}
    </span>
  );
}

function Arrow({ label, glyph, onClick }: { label: string; glyph: string; onClick: () => void }) {
  return (
    <button
      type="button"
      title={label}
      onClick={(e) => {
        e.stopPropagation();
        onClick();
      }}
      className="flex h-4 w-4 items-center justify-center rounded text-[7px] text-paper-dim hover:bg-black/[.08] dark:text-ink-text/50 dark:hover:bg-white/10"
    >
      {glyph}
    </button>
  );
}
