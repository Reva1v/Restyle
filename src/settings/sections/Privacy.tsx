import { useEffect, useMemo, useState } from "react";
import { commands, type RunningApp, type Settings } from "../../lib/ipc";
import type { T } from "../../lib/i18n";
import { Button, Caption, Modal, SectionTitle, ToggleRow } from "../../ui/kit";

/** Исключения по умолчанию (те же, что в `settings.rs`): менеджеры паролей и банки. */
const DEFAULT_EXCLUDED_LIST = [
  "KeePass.exe", "KeePassXC.exe", "1Password.exe", "Bitwarden.exe", "LastPass.exe",
  "Dashlane.exe", "NordPass.exe", "Enpass.exe", "Proton Pass.exe", "RoboForm.exe",
  "Keeper.exe", "SberBank.exe", "Tinkoff.exe", "AlfaBank.exe",
];
/** «Только выделение» по умолчанию: редакторы, где Ctrl+A выделяет весь файл. */
const DEFAULT_SELECT_ONLY_LIST = [
  "Code.exe", "idea64.exe", "webstorm64.exe", "pycharm64.exe", "rider64.exe",
  "clion64.exe", "goland64.exe", "devenv.exe", "notepad++.exe", "sublime_text.exe",
  "WINWORD.EXE", "Obsidian.exe", "Notion.exe",
];
const DEFAULT_EXCLUDED = new Set(DEFAULT_EXCLUDED_LIST.map((s) => s.toLowerCase()));
const DEFAULT_SELECT_ONLY = new Set(DEFAULT_SELECT_ONLY_LIST.map((s) => s.toLowerCase()));

/**
 * «Приватность» (макет, раздел 03): выключатель скриншота, выбор «окно или
 * монитор», список процессов-исключений и список «только выделение».
 *
 * Оба списка пополняются выбором из запущенных приложений: вспоминать точное
 * имя exe (`olk.exe`? `ms-teams.exe`?) — заведомо проигрышное занятие. Ручной
 * ввод остался для приложений, которые сейчас не запущены.
 */
export default function Privacy({ t, settings, apply }: { t: T; settings: Settings; apply: (s: Settings) => void }) {
  const [picking, setPicking] = useState<null | "shot" | "select">(null);

  const add = (list: "shot" | "select", names: string[]) => {
    const key = list === "shot" ? "screenshotExcludedProcesses" : "selectOnlyProcesses";
    const current = settings[key];
    const merged = [...current];
    for (const n of names) {
      if (!merged.some((x) => x.toLowerCase() === n.toLowerCase())) merged.push(n);
    }
    apply({ ...settings, [key]: merged });
  };
  const removeFrom = (list: "shot" | "select", name: string) => {
    const key = list === "shot" ? "screenshotExcludedProcesses" : "selectOnlyProcesses";
    apply({ ...settings, [key]: settings[key].filter((x) => x !== name) });
  };

  return (
    <div className="flex h-full min-h-0 flex-col gap-4 overflow-y-auto pr-1">
      <ToggleRow
        title={t("privacy.shot")}
        hint={t("privacy.shotHint")}
        checked={settings.screenshotEnabled}
        onChange={(v) => apply({ ...settings, screenshotEnabled: v })}
      />

      <div className="flex flex-col gap-2.5">
        <Caption>{t("privacy.what")}</Caption>
        <div className="flex gap-2.5">
          <ModeCard
            title={t("privacy.window")}
            hint={t("privacy.windowHint")}
            active={settings.screenshotMode === "window"}
            disabled={!settings.screenshotEnabled}
            onPick={() => apply({ ...settings, screenshotMode: "window" })}
            preview="window"
          />
          <ModeCard
            title={t("privacy.monitor")}
            hint={t("privacy.monitorHint")}
            active={settings.screenshotMode === "monitor"}
            disabled={!settings.screenshotEnabled}
            onPick={() => apply({ ...settings, screenshotMode: "monitor" })}
            preview="monitor"
          />
        </div>
        <div className="flex items-center gap-4 rounded-md border border-black/[.07] bg-black/[.035] px-3 py-2.5 dark:border-white/[.07] dark:bg-white/[.03]">
          <span className="text-[11.5px] text-paper-mid dark:text-ink-text/65">{t("privacy.specs")}</span>
        </div>
      </div>

      <ProcessList
        t={t}
        title={t("privacy.never")}
        hint={t("privacy.neverHint")}
        items={settings.screenshotExcludedProcesses}
        isDefault={(p) => DEFAULT_EXCLUDED.has(p.toLowerCase())}
        onPick={() => setPicking("shot")}
        onRemove={(p) => removeFrom("shot", p)}
        onAdd={(p) => add("shot", [p])}
        missingDefaults={missing(settings.screenshotExcludedProcesses, DEFAULT_EXCLUDED_LIST)}
        onRestore={() => add("shot", missing(settings.screenshotExcludedProcesses, DEFAULT_EXCLUDED_LIST))}
      />

      <ProcessList
        t={t}
        title={t("privacy.selectOnly")}
        hint={t("privacy.selectOnlyHint")}
        items={settings.selectOnlyProcesses}
        isDefault={(p) => DEFAULT_SELECT_ONLY.has(p.toLowerCase())}
        onPick={() => setPicking("select")}
        onRemove={(p) => removeFrom("select", p)}
        onAdd={(p) => add("select", [p])}
        missingDefaults={missing(settings.selectOnlyProcesses, DEFAULT_SELECT_ONLY_LIST)}
        onRestore={() => add("select", missing(settings.selectOnlyProcesses, DEFAULT_SELECT_ONLY_LIST))}
      />

      {picking && (
        <AppPicker
          t={t}
          already={picking === "shot" ? settings.screenshotExcludedProcesses : settings.selectOnlyProcesses}
          onClose={() => setPicking(null)}
          onAdd={(names) => {
            add(picking, names);
            setPicking(null);
          }}
        />
      )}
    </div>
  );
}

/** Чего из стандартного списка в настройках уже нет. */
function missing(items: string[], defaults: string[]): string[] {
  const have = new Set(items.map((s) => s.toLowerCase()));
  return defaults.filter((d) => !have.has(d.toLowerCase()));
}

function ProcessList({
  t,
  title,
  hint,
  items,
  isDefault,
  onPick,
  onRemove,
  onAdd,
  missingDefaults,
  onRestore,
}: {
  t: T;
  title: string;
  hint: string;
  items: string[];
  isDefault: (p: string) => boolean;
  onPick: () => void;
  onRemove: (p: string) => void;
  onAdd: (p: string) => void;
  /** Удалённые пункты стандартного списка — их можно вернуть одной ссылкой. */
  missingDefaults: string[];
  onRestore: () => void;
}) {
  const [draft, setDraft] = useState("");
  const commit = () => {
    const v = draft.trim();
    if (v) onAdd(v);
    setDraft("");
  };
  return (
    <div className="flex flex-col gap-2.5">
      <div className="flex items-center gap-2">
        <SectionTitle title={title} />
        <span className="font-mono text-[11px] text-paper-soft dark:text-ink-text/45">
          {t("privacy.processes", { n: items.length })}
        </span>
        {missingDefaults.length > 0 && (
          <button
            type="button"
            onClick={onRestore}
            className="text-[11.5px] font-medium text-amber-ink hover:underline dark:text-amber"
          >
            {t("privacy.restoreDefaults", { n: missingDefaults.length })}
          </button>
        )}
        <span className="ml-auto">
          <Button kind="primary" onClick={onPick}>
            {t("privacy.pick")}
          </Button>
        </span>
      </div>
      <span className="-mt-1 text-[11px] leading-snug text-paper-dim dark:text-ink-text/45">{hint}</span>
      <div className="flex flex-wrap gap-1.5 rounded-lg border border-black/10 bg-white p-2.5 dark:border-white/[.08] dark:bg-white/[.04]">
        {items.map((p) => (
          <span
            key={p}
            className="group flex items-center gap-1.5 rounded-md border border-black/10 bg-paper-line py-1 pl-2 pr-1 font-mono text-[11px] text-[#33363c] dark:border-white/10 dark:bg-white/[.06] dark:text-ink-text/80"
          >
            {p}
            {isDefault(p) && (
              <span className="font-sans text-[9.5px] text-paper-soft dark:text-ink-text/40">
                {t("common.default")}
              </span>
            )}
            <button
              type="button"
              title={t("common.delete")}
              onClick={() => onRemove(p)}
              className="px-1 text-paper-soft transition-colors hover:text-danger-ink dark:text-ink-text/40 dark:hover:text-danger"
            >
              ✕
            </button>
          </span>
        ))}
        {items.length === 0 && (
          <span className="px-1 py-0.5 text-[11.5px] text-paper-dim dark:text-ink-text/45">{t("common.empty")}</span>
        )}
        <input
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={commit}
          onKeyDown={(e) => e.key === "Enter" && e.currentTarget.blur()}
          placeholder={t("privacy.manual")}
          className="min-w-[130px] flex-1 rounded-md border border-dashed border-black/[.18] bg-transparent px-2 py-1 font-mono text-[11px] text-paper-text outline-none placeholder:font-sans placeholder:text-paper-soft focus:border-amber-deep dark:border-white/15 dark:text-ink-text dark:placeholder:text-ink-text/35 dark:focus:border-amber"
        />
      </div>
    </div>
  );
}

/**
 * Выбор из запущенных приложений. UWP-программы все показываются как
 * `ApplicationFrameHost.exe`, поэтому рядом с exe идёт заголовок окна —
 * иначе такие строки не отличить одну от другой.
 */
function AppPicker({
  t,
  already,
  onAdd,
  onClose,
}: {
  t: T;
  already: string[];
  onAdd: (names: string[]) => void;
  onClose: () => void;
}) {
  const [apps, setApps] = useState<RunningApp[] | null>(null);
  const [query, setQuery] = useState("");
  const [chosen, setChosen] = useState<string[]>([]);

  useEffect(() => {
    void commands.runningApps().then(setApps);
  }, []);

  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    return (apps ?? []).filter(
      (a) => !q || a.exe.toLowerCase().includes(q) || a.title.toLowerCase().includes(q),
    );
  }, [apps, query]);

  return (
    <Modal title={t("privacy.pickTitle")} onClose={onClose}>
      <div className="flex flex-none items-center gap-2 border-b border-black/[.07] p-3 dark:border-white/[.06]">
        <input
          autoFocus
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder={t("privacy.search")}
          className="h-8 flex-1 rounded-md border border-black/[.14] bg-white px-2.5 text-[12.5px] text-paper-text outline-none focus:border-amber-deep dark:border-white/10 dark:bg-white/[.05] dark:text-ink-text dark:focus:border-amber"
        />
      </div>
      <div className="flex min-h-0 flex-1 flex-col overflow-y-auto">
        {apps === null && (
          <span className="px-3 py-3 text-[12px] text-paper-dim dark:text-ink-text/45">{t("common.waiting")}</span>
        )}
        {apps !== null && shown.length === 0 && (
          <span className="px-3 py-3 text-[12px] text-paper-dim dark:text-ink-text/45">{t("privacy.nothing")}</span>
        )}
        {shown.map((a) => {
          const has = already.some((x) => x.toLowerCase() === a.exe.toLowerCase());
          const picked = chosen.includes(a.exe);
          return (
            <button
              key={a.exe}
              type="button"
              disabled={has}
              onClick={() => setChosen((c) => (picked ? c.filter((x) => x !== a.exe) : [...c, a.exe]))}
              className={`flex items-center gap-2.5 border-b border-black/[.05] px-3 py-2 text-left last:border-b-0 disabled:opacity-45 dark:border-white/[.05] ${
                picked ? "bg-amber-deep/[.12] dark:bg-amber/[.14]" : "hover:bg-black/[.04] dark:hover:bg-white/[.05]"
              }`}
            >
              <span
                className={`flex h-3.5 w-3.5 flex-none items-center justify-center rounded border text-[9px] ${
                  picked
                    ? "border-amber-deep bg-amber-deep text-white dark:border-amber dark:bg-amber dark:text-ink-900"
                    : "border-black/25 dark:border-white/25"
                }`}
              >
                {picked ? "✓" : ""}
              </span>
              <span className="flex min-w-0 flex-1 flex-col">
                <span className="truncate font-mono text-[12px] text-paper-text dark:text-ink-text/90">{a.exe}</span>
                <span className="truncate text-[11px] text-paper-dim dark:text-ink-text/45">{a.title}</span>
              </span>
              {has && <span className="text-[10.5px] text-paper-soft dark:text-ink-text/40">{t("privacy.added")}</span>}
            </button>
          );
        })}
      </div>
      <div className="flex flex-none items-center gap-2 border-t border-black/[.07] p-3 dark:border-white/[.06]">
        <span className="font-mono text-[11px] text-paper-soft dark:text-ink-text/45">{chosen.length}</span>
        <span className="ml-auto flex gap-2">
          <Button onClick={onClose}>{t("common.cancel")}</Button>
          <Button kind="primary" disabled={chosen.length === 0} onClick={() => onAdd(chosen)}>
            {t("common.add")}
          </Button>
        </span>
      </div>
    </Modal>
  );
}

function ModeCard({
  title,
  hint,
  active,
  disabled,
  onPick,
  preview,
}: {
  title: string;
  hint: string;
  active: boolean;
  disabled: boolean;
  onPick: () => void;
  preview: "window" | "monitor";
}) {
  return (
    <button
      type="button"
      disabled={disabled}
      onClick={onPick}
      className={`flex flex-1 flex-col gap-2 rounded-lg border p-3 text-left transition-colors disabled:opacity-40 ${
        active
          ? "border-amber-deep bg-white shadow-[0_0_0_2px_rgb(var(--amber-deep)/.13)] dark:border-amber dark:bg-white/[.06]"
          : "border-black/[.12] bg-white hover:border-black/25 dark:border-white/[.08] dark:bg-white/[.04] dark:hover:border-white/20"
      }`}
    >
      <span className="flex h-[52px] items-center justify-center rounded-md border border-black/[.08] bg-paper-line dark:border-white/[.06] dark:bg-white/[.04]">
        <span
          className={`rounded-[3px] border ${
            preview === "window"
              ? "h-6 w-10 border-amber-deep bg-amber-deep/20 dark:border-amber dark:bg-amber/20"
              : "h-8 w-16 border-black/25 bg-black/5 dark:border-white/25 dark:bg-white/10"
          }`}
        />
      </span>
      <span className="text-[12.5px] font-medium text-paper-text dark:text-ink-text">{title}</span>
      <span className="text-[11px] leading-snug text-paper-dim dark:text-ink-text/50">{hint}</span>
    </button>
  );
}
