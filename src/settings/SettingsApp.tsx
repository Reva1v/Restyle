import { useEffect, useRef, useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import { commands, events, type Settings } from "../lib/ipc";
import { useAppShell } from "../lib/theme";
import type { Key, T } from "../lib/i18n";
import { TitleBar } from "../ui/kit";
import ModelKey from "./sections/ModelKey";
import Hotkeys from "./sections/Hotkeys";
import Styles from "./sections/Styles";
import Privacy from "./sections/Privacy";
import PasteHistory from "./sections/PasteHistory";
import Appearance from "./sections/Appearance";

/** Репозиторий проекта — ссылка в карточке версии. */
const REPO_URL = "https://github.com/Reva1v/Restyle";

type Tab = "model" | "hotkeys" | "styles" | "privacy" | "paste" | "appearance";
const TABS: { id: Tab; label: Key }[] = [
  { id: "model", label: "nav.model" },
  { id: "hotkeys", label: "nav.hotkeys" },
  { id: "styles", label: "nav.styles" },
  { id: "privacy", label: "nav.privacy" },
  { id: "paste", label: "nav.paste" },
  { id: "appearance", label: "nav.appearance" },
];

/**
 * Окно настроек (макет, раздел 03): своя шапка вместо системных декораций,
 * слева пять разделов, справа содержимое. Правки уходят в бэкенд сразу
 * (тумблеры и хоткеи — по изменению, текст — по потере фокуса), бэкенд
 * нормализует данные и возвращает то, что реально сохранено.
 */
export default function SettingsApp() {
  const { t, lang } = useAppShell();
  const [settings, setSettings] = useState<Settings | null>(null);
  const [tab, setTab] = useState<Tab>("model");
  const [conflicts, setConflicts] = useState<[string, string][]>([]);
  const [hookOk, setHookOk] = useState(true);
  const [saved, setSaved] = useState(false);
  const savedTimer = useRef<number | undefined>(undefined);
  // Наше же сохранение прилетает обратно событием settings-changed —
  // не затираем им состояние, иначе теряются правки, набранные следом.
  const pendingEcho = useRef(0);

  useEffect(() => {
    document.documentElement.classList.add("app-window");
    void commands.getSettings().then((s) => {
      setSettings(s);
      void commands.hotkeyConflicts(s).then(setConflicts);
    });
    void commands.diagnostics().then((d) => setHookOk(d.hookInstalled));
    const un = events.onSettingsChanged((s) => {
      if (pendingEcho.current > 0) {
        pendingEcho.current -= 1;
        return;
      }
      setSettings(s);
      void commands.hotkeyConflicts(s).then(setConflicts);
    });
    return () => void un.then((f) => f());
  }, []);

  function apply(next: Settings) {
    setSettings(next);
    pendingEcho.current += 1;
    void commands.saveSettings(next).then((stored) => {
      setSettings(stored);
      void commands.hotkeyConflicts(stored).then(setConflicts);
    });
    setSaved(true);
    window.clearTimeout(savedTimer.current);
    savedTimer.current = window.setTimeout(() => setSaved(false), 1200);
  }

  if (!settings) return null;

  return (
    <div className="flex h-screen flex-col bg-paper text-paper-text dark:bg-ink-800 dark:text-ink-text">
      <TitleBar title={`Restyle — ${t("app.settings")}`} t={t} />
      <div className="flex min-h-0 flex-1">
        <nav className="flex w-[196px] flex-none flex-col gap-0.5 border-r border-black/[.07] bg-paper-sand p-2.5 py-3.5 dark:border-white/[.06] dark:bg-ink-750">
          {TABS.map((x) => (
            <button
              key={x.id}
              type="button"
              onClick={() => setTab(x.id)}
              className={`rounded-[7px] px-3 py-2.5 text-left text-[13px] transition-colors ${
                x.id === tab
                  ? "border-l-2 border-amber-deep bg-white font-medium text-paper-text shadow-card dark:border-amber dark:bg-white/[.08] dark:text-ink-text"
                  : "text-paper-mid hover:bg-black/[.04] dark:text-ink-text/60 dark:hover:bg-white/[.05]"
              }`}
            >
              {t(x.label)}
            </button>
          ))}
          <div className="mt-auto flex flex-col gap-1 rounded-[7px] bg-black/[.04] p-2.5 dark:bg-white/[.05]">
            <span className="flex items-center gap-2">
              <span className="text-[11px] font-medium text-[#33363c] dark:text-ink-text/85">Restyle 0.1.0</span>
              <button
                type="button"
                onClick={() => void commands.openUrl(REPO_URL)}
                title={REPO_URL}
                className="ml-auto flex cursor-pointer items-center gap-1 text-[11px] text-paper-mid hover:text-amber-ink dark:text-ink-text/55 dark:hover:text-amber"
              >
                <svg viewBox="0 0 16 16" className="h-3.5 w-3.5 fill-current" aria-hidden>
                  <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.013 8.013 0 0016 8c0-4.42-3.58-8-8-8z" />
                </svg>
                GitHub
              </button>
            </span>
            <span
              className={`font-mono text-[10.5px] leading-snug ${
                hookOk ? "text-paper-soft dark:text-ink-text/50" : "text-danger-ink dark:text-danger"
              }`}
            >
              {hookOk ? t("nav.hookOk") : t("nav.hookDown")}
            </span>
          </div>
        </nav>

        <main className="relative min-w-0 flex-1">
          <AnimatePresence mode="wait" initial={false}>
            <motion.div
              key={tab}
              initial={{ opacity: 0, x: 6 }}
              animate={{ opacity: 1, x: 0 }}
              exit={{ opacity: 0, x: -6 }}
              transition={{ duration: 0.12 }}
              className={`h-full ${tab === "styles" ? "" : "overflow-y-auto"}`}
            >
              {/* Отступы — на внутреннем блоке, а не на скроллере: нижний
                  padding самого скроллера в конец прокрутки не попадает, и
                  последний элемент раздела прилипает к краю окна. */}
              {tab === "styles" ? (
                <Section tab={tab} t={t} lang={lang} settings={settings} apply={apply} conflicts={conflicts} hookOk={hookOk} />
              ) : (
                <div className="p-6">
                  <Section tab={tab} t={t} lang={lang} settings={settings} apply={apply} conflicts={conflicts} hookOk={hookOk} />
                </div>
              )}
            </motion.div>
          </AnimatePresence>

          <AnimatePresence>
            {saved && (
              <motion.span
                initial={{ opacity: 0, y: 6 }}
                animate={{ opacity: 1, y: 0 }}
                exit={{ opacity: 0 }}
                className="pointer-events-none absolute bottom-3 right-4 rounded-md bg-ok/15 px-2.5 py-1 text-[11px] font-medium text-ok"
              >
                ✓
              </motion.span>
            )}
          </AnimatePresence>
        </main>
      </div>
    </div>
  );
}

function Section({
  tab,
  t,
  lang,
  settings,
  apply,
  conflicts,
  hookOk,
}: {
  tab: Tab;
  t: T;
  lang: string;
  settings: Settings;
  apply: (s: Settings) => void;
  conflicts: [string, string][];
  hookOk: boolean;
}) {
  switch (tab) {
    case "model":
      return <ModelKey t={t} settings={settings} apply={apply} />;
    case "hotkeys":
      return <Hotkeys t={t} settings={settings} apply={apply} conflicts={conflicts} hookOk={hookOk} lang={lang} />;
    case "styles":
      return <Styles t={t} settings={settings} apply={apply} lang={lang} />;
    case "privacy":
      return <Privacy t={t} settings={settings} apply={apply} />;
    case "paste":
      return <PasteHistory t={t} settings={settings} apply={apply} />;
    case "appearance":
      return <Appearance t={t} settings={settings} apply={apply} />;
  }
}
