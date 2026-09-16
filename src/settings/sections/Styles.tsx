import { useEffect, useState } from "react";
import { commands, type Settings, type Style, type StyleKind } from "../../lib/ipc";
import type { T } from "../../lib/i18n";
import { comboLabel } from "../../lib/hotkey";
import HotkeyField from "../HotkeyField";
import { Button, Caption, DraftField, MoreMenu, Segmented, Select, Toggle } from "../../ui/kit";
import { LANGS, langLabel } from "../../lib/langs";


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
  /** Итог последнего импорта/экспорта: строка под кнопками, не тост. */
  const [ioNote, setIoNote] = useState<{ text: string; error: boolean } | null>(null);
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

  /** Диалоги и файлы — на бэкенде; сохранённые настройки придут событием. */
  async function importStyles() {
    setIoNote(null);
    try {
      const n = await commands.importStyles();
      if (n === null) return;
      setIoNote({ text: n > 0 ? t("styles.imported", { n }) : t("styles.importedNone"), error: false });
    } catch (e) {
      setIoNote({ text: String(e), error: true });
    }
  }

  async function exportStyles() {
    setIoNote(null);
    try {
      await commands.exportStyles();
    } catch (e) {
      setIoNote({ text: String(e), error: true });
    }
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

  /** Подстрочник строки: язык перевода и/или хоткей. */
  const subline = (s: Style) =>
    [s.kind === "translate" ? `${t("styles.kindTranslate")} → ${langLabel(s.targetLang)}` : "", s.hotkey ? comboLabel(s.hotkey) : ""]
      .filter(Boolean)
      .join(" · ");

  const row = ({ s, i }: { s: Style; i: number }) => {
    const active = i === selected;
    return (
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
        className={`group relative flex select-none items-center gap-2.5 rounded-[8px] py-[8px] pl-3 pr-2 text-left transition-colors ${
          drag === null ? "cursor-pointer" : "cursor-grabbing"
        } ${
          active ? "bg-amber-deep/[.10] dark:bg-amber/[.12]" : "hover:bg-black/[.04] dark:hover:bg-white/[.05]"
        } ${drag === i ? "opacity-40" : ""} ${dropLine(i)}`}
      >
        {/* Выбранная строка — акцентная полоска слева вместо рамки: спокойнее и читается в обеих темах. */}
        {active && <span className="absolute bottom-[7px] left-0 top-[7px] w-[3px] rounded-full bg-amber-deep dark:bg-amber" />}
        <span
          className={`flex h-[20px] w-[20px] flex-none items-center justify-center rounded-[5px] font-mono text-[10.5px] ${
            active
              ? "bg-amber-deep/[.16] text-amber-ink dark:bg-amber/[.18] dark:text-amber"
              : "bg-black/[.05] text-paper-dim dark:bg-white/[.06] dark:text-ink-text/50"
          }`}
          title={i < 9 ? t("styles.chips") : undefined}
        >
          {i < 9 ? i + 1 : "·"}
        </span>
        <span className="flex min-w-0 flex-1 flex-col gap-[2px]">
          <span
            className={`truncate text-[12.5px] leading-tight ${
              active ? "font-medium text-paper-text dark:text-ink-text" : "text-[#33363c] dark:text-ink-text/90"
            }`}
          >
            {s.name}
          </span>
          {subline(s) && (
            <span className="truncate font-mono text-[10px] leading-tight text-paper-dim dark:text-ink-text/45">{subline(s)}</span>
          )}
        </span>
        {/* Стрелки — на случай, когда таскать мышью неудобно */}
        <span className="flex flex-none gap-0.5 opacity-0 transition-opacity group-hover:opacity-100">
          <Arrow label={t("styles.up")} onClick={() => move(i, i - 1)} glyph="▲" />
          <Arrow label={t("styles.down")} onClick={() => move(i, i + 1)} glyph="▼" />
        </span>
      </div>
    );
  };

  const resetToDefault = () =>
    def &&
    patch({
      name: def.name,
      instruction: def.instruction,
      screenshot: def.screenshot,
      kind: def.kind,
      targetLang: def.targetLang,
      postStyle: def.postStyle,
    });

  return (
    <div className="flex h-full min-h-0">
      <div className="flex w-[268px] flex-none flex-col border-r border-black/[.08] dark:border-white/[.07]">
        {/* Шапка в одну строку: название и счётчик, справа «Добавить» и «···»
            с импортом/экспортом/возвратом. Подсказка про цифры 1–9 — в title
            счётчика: цифры и так стоят у каждого стиля. */}
        <div className="flex items-center gap-2 px-4 pb-2.5 pt-4">
          <span className="text-[13px] font-semibold text-paper-text dark:text-ink-text">{t("styles.title")}</span>
          <span title={t("styles.chips")} className="font-mono text-[11px] text-paper-dim dark:text-ink-text/50">
            {settings.styles.length}
          </span>
          <button
            type="button"
            onClick={add}
            className="ml-auto text-[11.5px] font-medium text-amber-ink hover:underline dark:text-amber"
          >
            {t("common.add")}
          </button>
          <MoreMenu
            title={t("styles.more")}
            items={[
              { label: t("styles.import"), onClick: () => void importStyles() },
              { label: t("styles.export"), onClick: () => void exportStyles() },
              {
                label: t("styles.restoreBuiltin"),
                onClick: restoreBuiltin,
                hidden: !defaults.some((d) => !settings.styles.some((s) => s.id === d.id)),
              },
            ]}
          />
        </div>
        {ioNote && (
          <span
            className={`px-4 pb-2 text-[10.5px] leading-snug ${
              ioNote.error ? "text-danger-ink dark:text-danger" : "text-paper-dim dark:text-ink-text/60"
            }`}
          >
            {ioNote.text}
          </span>
        )}
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
        <div className="flex min-w-0 flex-1 flex-col gap-3.5 overflow-y-auto p-5">
          {/* Имя — на всю ширину, тип — строкой под ним: панель узкая, в одну
              строку имя вроде «Translate to Ukrainian» не помещалось. */}
          <label className="flex min-w-0 flex-col gap-1.5">
            <Caption>{t("styles.name")}</Caption>
            <DraftField className="h-[36px] py-0 text-[14px] font-medium" value={style.name} onCommit={(v) => patch({ name: v })} />
          </label>
          {/* Переключатель типа — сам себе рамка: без подписи и внутренних отступов. */}
          <div className="flex h-[34px] w-full flex-none items-center overflow-hidden rounded-lg border border-black/10 bg-black/[.03] dark:border-white/[.08] dark:bg-white/[.04]">
            <Segmented<StyleKind>
              fill
              value={style.kind}
              onChange={(v) =>
                patch({ kind: v, targetLang: v === "translate" && !style.targetLang ? "EN-US" : style.targetLang })
              }
              options={[
                { value: "prompt", label: t("styles.kindPrompt") },
                { value: "translate", label: t("styles.kindTranslate") },
              ]}
            />
          </div>

          {style.kind === "translate" && (
            <Section title={t("styles.sectionTranslate")} hint={t("styles.postHint")}>
              <div className="grid grid-cols-2 gap-3">
                <label className="flex min-w-0 flex-col gap-1.5">
                  <Caption>{t("styles.lang")}</Caption>
                  <Select
                    className="h-[34px]"
                    value={style.targetLang || "EN-US"}
                    onChange={(v) => patch({ targetLang: v })}
                    options={LANGS}
                  />
                </label>
                <label className="flex min-w-0 flex-col gap-1.5">
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
            </Section>
          )}

          <Section
            title={t("styles.instructionTitle")}
            hint={style.kind === "translate" ? t("styles.instructionTranslateHint") : t("styles.instructionHint")}
          >
            <DraftField
              multiline
              className={`max-h-[300px] resize-y leading-relaxed ${style.kind === "translate" ? "min-h-[76px]" : "min-h-[96px]"}`}
              placeholder={t("styles.placeholder")}
              value={style.instruction}
              onCommit={(v) => patch({ instruction: v })}
            />
          </Section>

          {/* Поведение: хоткей строкой на всю ширину (подпись + поле), ниже —
              тумблер скриншота. Одна карточка, чтобы не плодить рамки. */}
          <div className="flex flex-none flex-col divide-y divide-black/[.08] rounded-lg border border-black/10 bg-white dark:divide-white/[.07] dark:border-white/[.08] dark:bg-white/[.04]">
            <div className="flex flex-col gap-2 p-3">
              <div className="flex flex-col gap-[3px]">
                <span className="text-[13px] font-medium leading-tight text-paper-text dark:text-ink-text">{t("styles.hotkeyTitle")}</span>
                <span className="text-[11.5px] leading-snug text-paper-dim dark:text-ink-text/55">{t("styles.hotkeyHint")}</span>
              </div>
              <HotkeyField fill lang={lang} value={style.hotkey} allowEmpty onChange={(c) => patch({ hotkey: c })} />
            </div>
            <label className="flex cursor-pointer items-center gap-3.5 p-3">
              <span className="flex min-w-0 flex-1 flex-col gap-[3px]">
                <span className="text-[13px] font-medium leading-tight text-paper-text dark:text-ink-text">{t("styles.screenshotTitle")}</span>
                <span className="text-[11.5px] leading-snug text-paper-dim dark:text-ink-text/55">{t("styles.screenshotHint")}</span>
              </span>
              <Toggle checked={style.screenshot} onChange={(v) => patch({ screenshot: v })} />
            </label>
          </div>

          <div className="mt-auto flex flex-none items-center gap-3 border-t border-black/[.08] pt-3 dark:border-white/[.07]">
            <span className="min-w-0 flex-1 truncate font-mono text-[11px] text-paper-soft dark:text-ink-text/40">
              id: {style.id || "…"}
            </span>
            <span className="flex flex-none items-center gap-2.5">
              {style.builtin &&
                (changed ? (
                  <Button onClick={resetToDefault} title={t("styles.resetHint")}>
                    {t("styles.resetToDefault")}
                  </Button>
                ) : (
                  <span className="text-[11px] text-paper-dim dark:text-ink-text/45">{t("styles.isDefault")}</span>
                ))}
              {/* Удалять можно и встроенные: вернуть их можно из меню «···». */}
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

/** Карточка редактора: заголовок, подсказка и содержимое. */
function Section({ title, hint, children }: { title: string; hint?: string; children: React.ReactNode }) {
  return (
    <div className="flex flex-none flex-col gap-2.5 rounded-lg border border-black/10 bg-white p-3 dark:border-white/[.08] dark:bg-white/[.04]">
      <div className="flex flex-col gap-[3px]">
        <span className="text-[13px] font-medium leading-tight text-paper-text dark:text-ink-text">{title}</span>
        {hint && <span className="text-[11.5px] leading-snug text-paper-dim dark:text-ink-text/55">{hint}</span>}
      </div>
      {children}
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
