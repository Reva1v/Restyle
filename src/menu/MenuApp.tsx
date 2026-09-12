import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import { commands, events, type HistoryItem, type Settings } from "../lib/ipc";
import { useAppShell } from "../lib/theme";
import { comboLabel } from "../lib/hotkey";
import { KeyCap, Toggle } from "../ui/kit";
import { FMT_KEYS, FORMATS, actionName, type T } from "../lib/i18n";

/** Сколько записей показывает меню; остальное — в окне истории. */
const MENU_ITEMS = 10;

/**
 * Меню трея (макет, раздел 07) — обычное окно без фокуса, а не системное меню.
 * Системное в Tauri v2 статично, дублировало пункты при пересборке подменю и
 * выглядело чужеродно. Окно не активируется, поэтому «вернуть предыдущий
 * текст» по-прежнему вставляет в то приложение, где остался курсор.
 *
 * Высота уезжает в бэкенд (`resize_menu`) после каждой перерисовки: подменю
 * истории раскрывается на месте, окно должно следовать за содержимым.
 */
export default function MenuApp() {
  const { settings, t } = useAppShell();
  const [items, setItems] = useState<HistoryItem[]>([]);
  const [view, setView] = useState<"root" | "history">("root");
  const [copied, setCopied] = useState<number | null>(null);
  /** Окно меню служит и списком регистров для панели оверлея. */
  const [mode, setMode] = useState<"tray" | "format">("tray");
  const [fmtCurrent, setFmtCurrent] = useState("");
  const rootRef = useRef<HTMLDivElement>(null);

  const reload = useCallback(() => {
    void commands.getHistory().then(setItems);
  }, []);

  useEffect(() => {
    reload();
    const subs = [
      events.onHistoryChanged(reload),
      // Каждое открытие начинается с корня: меню — не окно, состояние копить незачем.
      events.onMenuFormat((p) => {
        setMode("format");
        setFmtCurrent(p.current);
      }),
      events.onMenuShow(() => {
        setMode("tray");
        setView("root");
        setCopied(null);
        reload();
      }),
    ];
    return () => void Promise.all(subs).then((fns) => fns.forEach((f) => f()));
  }, [reload]);

  // Высота по содержимому: раскрытая история выше корня.
  // `settings` в зависимостях обязателен: до их загрузки рисовать нечего,
  // ref пустой, и без перезапуска эффекта бэкенд навсегда остался бы с
  // дефолтной высотой — меню, открытое вверх, висело бы над курсором.
  useLayoutEffect(() => {
    const el = rootRef.current;
    if (!el) return;
    const send = () => void commands.resizeMenu(el.getBoundingClientRect().height);
    send();
    const ro = new ResizeObserver(send);
    ro.observe(el);
    return () => ro.disconnect();
  }, [view, items.length, copied, settings, mode, fmtCurrent]);

  if (!settings) return null;

  const close = () => void commands.closeTrayMenu();
  const run = (fn: () => void | Promise<unknown>) => {
    void Promise.resolve(fn()).then(close);
  };

  if (mode === "format") {
    return (
      <div
        ref={rootRef}
        className="flex flex-col overflow-hidden rounded-xl border border-black/[.13] bg-[rgba(250,248,245,.97)] p-1.5 text-paper-text dark:border-white/[.11] dark:bg-[rgba(24,26,30,.96)] dark:text-ink-text"
      >
        {FORMATS.map((f) => (
          <button
            key={f}
            type="button"
            onClick={() => void commands.selectStyle(`format:${f}`).then(close)}
            className={`flex items-center gap-2 rounded-lg px-2.5 py-[7px] text-left text-[12.5px] transition-colors ${
              f === fmtCurrent
                ? "bg-amber-deep/[.13] text-amber-ink dark:bg-amber/[.16] dark:text-amber-soft"
                : "text-paper-text hover:bg-black/[.06] dark:text-ink-text dark:hover:bg-white/[.07]"
            }`}
          >
            <span className="min-w-0 flex-1 truncate leading-snug">{t(FMT_KEYS[f])}</span>
            {f === fmtCurrent && <span className="text-[11px]">✓</span>}
          </button>
        ))}
      </div>
    );
  }

  return (
    <div
      ref={rootRef}
      className="overflow-hidden rounded-xl border border-black/[.13] bg-[rgba(250,248,245,.97)] p-1.5 text-paper-text dark:border-white/[.11] dark:bg-[rgba(24,26,30,.96)] dark:text-ink-text"
    >
      <AnimatePresence mode="wait" initial={false}>
        {view === "root" ? (
          <motion.div
            key="root"
            initial={{ opacity: 0, x: -6 }}
            animate={{ opacity: 1, x: 0 }}
            exit={{ opacity: 0, x: -6 }}
            transition={{ duration: 0.12 }}
            className="flex flex-col"
          >
            <Item label={t("menu.settings")} onClick={() => run(commands.openSettingsWindow)} />
            <Item
              label={t("menu.history")}
              chevron
              badge={items.length ? String(items.length) : undefined}
              onClick={() => setView("history")}
            />
            <Item
              label={t("menu.undo")}
              hotkey={comboLabel(settings.undoHotkey)}
              disabled={!items.length}
              onClick={() => run(commands.undoLast)}
            />
            <Divider />
            <AutostartItem t={t} settings={settings} />
            <Divider />
            <Item label={t("menu.quit")} onClick={() => void commands.exitApp()} />
          </motion.div>
        ) : (
          <motion.div
            key="history"
            initial={{ opacity: 0, x: 6 }}
            animate={{ opacity: 1, x: 0 }}
            exit={{ opacity: 0, x: 6 }}
            transition={{ duration: 0.12 }}
            className="flex flex-col"
          >
            <Item label={`‹  ${t("menu.back")}`} onClick={() => setView("root")} />
            <Divider />
            {items.length === 0 && (
              <span className="px-2.5 py-2 text-[12px] text-paper-dim dark:text-ink-text/45">{t("menu.empty")}</span>
            )}
            {items.slice(0, MENU_ITEMS).map((it, i) => (
              <button
                key={`${it.at}-${i}`}
                type="button"
                onClick={() => {
                  void commands.copyHistory(i);
                  setCopied(i);
                }}
                className="flex flex-col items-start gap-[3px] rounded-lg px-2.5 py-1.5 text-left transition-colors hover:bg-black/[.06] dark:hover:bg-white/[.07]"
              >
                <span className="flex w-full items-center gap-2">
                  <span className="truncate font-mono text-[10px] uppercase tracking-[.06em] text-amber-mid dark:text-amber">
                    {styleName(settings, it.styleId, t)}
                  </span>
                  <span className="ml-auto flex-none font-mono text-[10px] text-paper-soft dark:text-ink-text/40">
                    {copied === i ? `✓ ${t("menu.copied")}` : time(it.at)}
                  </span>
                </span>
                <span className="line-clamp-2 w-full text-[12px] leading-snug text-paper-text dark:text-ink-text/85">
                  {it.result}
                </span>
              </button>
            ))}
            <Divider />
            <Item label={t("menu.allHistory")} onClick={() => run(commands.openHistoryWindow)} />
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}

function styleName(settings: Settings, id: string, t: T) {
  return settings.styles.find((s) => s.id === id)?.name ?? actionName(t, id) ?? id;
}

function time(at: number) {
  return new Date(at).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
}

function Divider() {
  return <span className="my-1 h-px bg-black/[.08] dark:bg-white/[.07]" />;
}

function Item({
  label,
  hotkey,
  badge,
  chevron = false,
  disabled = false,
  onClick,
}: {
  label: string;
  hotkey?: string;
  badge?: string;
  chevron?: boolean;
  disabled?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      disabled={disabled}
      onClick={onClick}
      className="flex items-center gap-2 rounded-lg px-2.5 py-[7px] text-left text-[12.5px] text-paper-text transition-colors hover:bg-black/[.06] disabled:opacity-40 disabled:hover:bg-transparent dark:text-ink-text dark:hover:bg-white/[.07]"
    >
      <span className="min-w-0 flex-1 leading-snug">{label}</span>
      {badge && (
        <span className="rounded-full bg-black/[.07] px-1.5 font-mono text-[10px] text-paper-dim dark:bg-white/10 dark:text-ink-text/55">
          {badge}
        </span>
      )}
      <span className="flex flex-none items-center gap-1.5">
        {hotkey &&
          hotkey !== "—" &&
          hotkey.split(" + ").map((part, i) => <KeyCap key={`${part}-${i}`}>{part}</KeyCap>)}
        {chevron && <span className="text-[10px] text-paper-dim dark:text-ink-text/45">›</span>}
      </span>
    </button>
  );
}

/** Автозапуск переключается на месте — меню при этом не закрывается. */
function AutostartItem({ t, settings }: { t: T; settings: Settings }) {
  return (
    <button
      type="button"
      onClick={() => void commands.saveSettings({ ...settings, autostart: !settings.autostart })}
      className="flex items-center gap-2 rounded-lg px-2.5 py-[7px] text-left text-[12.5px] text-paper-text transition-colors hover:bg-black/[.06] dark:text-ink-text dark:hover:bg-white/[.07]"
    >
      <span>{t("menu.autostart")}</span>
      <span className="ml-auto">
        <Toggle
          size="sm"
          checked={settings.autostart}
          onChange={(v) => void commands.saveSettings({ ...settings, autostart: v })}
        />
      </span>
    </button>
  );
}
