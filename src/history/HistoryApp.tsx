import { useCallback, useEffect, useMemo, useState } from "react";
import { motion } from "motion/react";
import { commands, events, type HistoryItem, type Settings } from "../lib/ipc";
import { useAppShell } from "../lib/theme";
import { actionName } from "../lib/i18n";
import { TitleBar, Toggle } from "../ui/kit";

/**
 * Окно истории (макет, раздел 04): фильтры по стилям, карточки «исходник —
 * результат», «Вернуть» у каждой записи, тумблер записи на диск внизу.
 * Список приходит из control-цикла (там живёт история), обновляется по
 * событию `history-changed`.
 */
export default function HistoryApp() {
  const { t, settings } = useAppShell();
  const [items, setItems] = useState<HistoryItem[]>([]);
  const [filter, setFilter] = useState<string>("all");

  const reload = useCallback(() => {
    void commands.getHistory().then(setItems);
  }, []);

  useEffect(() => {
    document.documentElement.classList.add("app-window");
    reload();
    const un = events.onHistoryChanged(reload);
    return () => void un.then((f) => f());
  }, [reload]);

  const styles = settings?.styles ?? [];
  const styleName = (id: string) => styles.find((s) => s.id === id)?.name ?? actionName(t, id) ?? id;

  // Фильтры: «Все», стили, встречающиеся в истории, и «Свои».
  const used = useMemo(() => {
    const ids: string[] = [];
    for (const it of items) if (!ids.includes(it.styleId)) ids.push(it.styleId);
    return ids.slice(0, 4);
  }, [items]);

  const shown = items.filter((it) => {
    if (filter === "all") return true;
    if (filter === "custom") return !styles.find((s) => s.id === it.styleId)?.builtin;
    return it.styleId === filter;
  });

  const patchSettings = (p: Partial<Settings>) => {
    if (settings) void commands.saveSettings({ ...settings, ...p });
  };

  return (
    <div className="flex h-screen flex-col bg-paper text-paper-text dark:bg-ink-800 dark:text-ink-text">
      <TitleBar title={`Restyle — ${t("app.history")}`} t={t} />

      <div className="flex flex-none items-center gap-2.5 border-b border-black/[.07] px-4 py-3 dark:border-white/[.06]">
        <FilterChip label={t("hist.all")} active={filter === "all"} onClick={() => setFilter("all")} />
        {used.map((id) => (
          <FilterChip key={id} label={styleName(id)} active={filter === id} onClick={() => setFilter(id)} />
        ))}
        <FilterChip label={t("hist.own")} active={filter === "custom"} onClick={() => setFilter("custom")} />
        <span className="ml-auto font-mono text-[11px] text-paper-dim dark:text-ink-text/50">
          {t("hist.inMemory", { n: `${items.length} / ${settings?.historyLimit ?? 20}` })}
        </span>
        <button
          type="button"
          disabled={items.length === 0}
          onClick={() => void commands.clearHistory()}
          className="text-[11.5px] text-amber-ink disabled:opacity-40 hover:underline dark:text-amber"
        >
          {t("common.clear")}
        </button>
      </div>

      <div className="flex min-h-0 flex-1 flex-col gap-2.5 overflow-y-auto p-4">
        {shown.length === 0 && (
          <div className="m-auto text-[13px] text-paper-dim dark:text-ink-text/45">{t("hist.empty")}</div>
        )}
        {shown.map((it, i) => (
          <motion.div
            key={`${it.at}-${i}`}
            initial={{ opacity: 0, y: 4 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ duration: 0.12, delay: Math.min(i * 0.015, 0.1) }}
            className={`flex flex-col gap-2.5 rounded-[9px] border p-3.5 ${
              i === 0
                ? "border-amber-deep/[.28] bg-amber-deep/[.05] dark:border-amber/[.28] dark:bg-white/[.035]"
                : "border-black/[.08] bg-white dark:border-white/[.06] dark:bg-white/[.025]"
            }`}
          >
            <div className="flex items-center gap-2.5">
              <span
                className={`rounded px-1.5 py-[3px] font-mono text-[10px] font-medium uppercase ${
                  i === 0
                    ? "bg-amber-deep/[.16] text-amber-ink dark:bg-amber/[.16] dark:text-amber-soft"
                    : "bg-black/[.06] text-paper-mid dark:bg-white/[.07] dark:text-ink-text/70"
                }`}
              >
                {styleName(it.styleId)}
              </span>
              <span className="font-mono text-[11px] text-paper-dim dark:text-ink-text/50">{it.targetExe}</span>
              {it.selectOnly && (
                <span className="font-mono text-[10.5px] text-paper-dim dark:text-ink-text/45">
                  {t("hud.selectionOnly").toLowerCase()}
                </span>
              )}
              <span className="ml-auto font-mono text-[10.5px] text-paper-dim dark:text-ink-text/50">
                {new Date(it.at).toLocaleTimeString()}
              </span>
              <button
                type="button"
                onClick={() => void commands.undoEntry(items.indexOf(it))}
                className="text-[11px] text-amber-ink hover:underline dark:text-amber"
              >
                {it.showingOriginal ? t("hist.redo") : t("hist.undo")}
              </button>
              <CopyLink
                label={t("hist.copy")}
                done={t("menu.copied")}
                onClick={() => commands.copyHistory(items.indexOf(it))}
              />
            </div>
            <div className="grid grid-cols-2 gap-3">
              <div className="flex flex-col gap-1.5">
                <span className="font-mono text-[9.5px] font-medium uppercase tracking-[.08em] text-paper-dim dark:text-ink-text/50">
                  {t("hist.source")}
                </span>
                <p className="m-0 line-clamp-3 whitespace-pre-wrap break-words text-[12.5px] leading-relaxed text-paper-dim dark:text-ink-text/50">
                  {it.original}
                </p>
              </div>
              <div className="flex flex-col gap-1.5">
                <span className="font-mono text-[9.5px] font-medium uppercase tracking-[.08em] text-amber-mid dark:text-amber/60">
                  {t("hist.result")}
                </span>
                <p className="m-0 line-clamp-3 whitespace-pre-wrap break-words text-[12.5px] leading-relaxed text-paper-text dark:text-ink-text/90">
                  {it.result}
                </p>
              </div>
            </div>
          </motion.div>
        ))}
      </div>

      <div className="flex flex-none items-center gap-3.5 border-t border-black/[.07] bg-black/[.02] px-4 py-3 dark:border-white/[.06] dark:bg-white/[.02]">
        <button
          type="button"
          onClick={() => patchSettings({ historyToDisk: !settings?.historyToDisk })}
          className="-m-1.5 flex items-center gap-2.5 rounded-md p-1.5 transition-colors hover:bg-black/[.05] dark:hover:bg-white/[.06]"
        >
          <Toggle
            size="sm"
            checked={settings?.historyToDisk ?? false}
            onChange={(v) => patchSettings({ historyToDisk: v })}
          />
          <span className="text-[11.5px] text-paper-mid dark:text-ink-text/60">{t("history.toDisk")}</span>
        </button>
        {!settings?.historyToDisk && (
          <span className="text-[11px] text-paper-dim dark:text-ink-text/45">{t("hist.diskOff")}</span>
        )}
        <button
          type="button"
          onClick={() => void commands.openSettingsWindow()}
          className="ml-auto text-[11.5px] text-paper-dim hover:underline dark:text-ink-text/55"
        >
          {t("app.settings")}
        </button>
      </div>
    </div>
  );
}

/**
 * Отклик прямо в кнопке: раньше об успехе сообщал тост поверх чужих окон,
 * а это ровно то место, куда пользователь смотрит.
 */
function CopyLink({ label, done, onClick }: { label: string; done: string; onClick: () => Promise<unknown> }) {
  const [ok, setOk] = useState(false);
  useEffect(() => {
    if (!ok) return;
    const id = setTimeout(() => setOk(false), 1400);
    return () => clearTimeout(id);
  }, [ok]);
  return (
    <button
      type="button"
      onClick={() => void onClick().then(() => setOk(true))}
      className={`text-[11px] transition-colors hover:underline ${
        ok ? "text-ok" : "text-paper-dim dark:text-ink-text/60"
      }`}
    >
      {ok ? `✓ ${done}` : label}
    </button>
  );
}

function FilterChip({ label, active, onClick }: { label: string; active: boolean; onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={`rounded-md px-2.5 py-1.5 text-[11.5px] transition-colors ${
        active
          ? "border border-amber-deep/40 bg-amber-deep/[.15] font-medium text-amber-ink dark:border-amber/40 dark:bg-amber/[.15] dark:text-amber-soft"
          : "border border-transparent bg-black/[.04] text-paper-mid hover:bg-black/[.07] dark:bg-white/5 dark:text-ink-text/65 dark:hover:bg-white/10"
      }`}
    >
      {label}
    </button>
  );
}
