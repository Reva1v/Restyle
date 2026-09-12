import { useCallback, useEffect, useRef, useState } from "react";
import { commands, events, type HistoryItem, type Settings, type Style } from "../lib/ipc";
import { EMPTY_COMBO } from "../lib/hotkey";
import HotkeyField from "./HotkeyField";
import DraftInput from "./DraftInput";

/**
 * Окно настроек (фаза 6): хоткеи с проверкой конфликтов, общее, API-ключ,
 * стили с редактированием и быстрыми хоткеями, приватность (скриншот и списки
 * процессов), история. Оформление — заглушка до переноса макета (фаза 7).
 *
 * Правки уходят в бэкенд сразу: тумблеры и хоткеи — по изменению, текстовые
 * поля — по потере фокуса (каждое сохранение пересобирает ComboSet и пишет
 * store, на каждый символ это лишнее). Бэкенд нормализует данные и возвращает
 * их обратно — состояние окна всегда равно тому, что реально сохранено.
 */
const MODEL_PRESETS = [
  { id: "gemini-3.5-flash-lite", label: "Flash-Lite (быстрее)" },
  { id: "gemini-3.6-flash", label: "Flash (точнее)" },
];

export default function SettingsApp() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [saved, setSaved] = useState(false);
  const savedTimer = useRef<number | undefined>(undefined);
  const [hasKey, setHasKey] = useState<boolean | null>(null);
  const [keyDraft, setKeyDraft] = useState("");
  const [keyError, setKeyError] = useState("");
  const [conflicts, setConflicts] = useState<[string, string][]>([]);
  const [history, setHistory] = useState<HistoryItem[]>([]);
  const [defaults, setDefaults] = useState<Style[]>([]);
  // Наше же сохранение прилетает обратно событием settings-changed —
  // не затираем им состояние, иначе теряются правки, набранные следом.
  const pendingEcho = useRef(0);

  const reloadHistory = useCallback(() => {
    void commands.getHistory().then(setHistory);
  }, []);

  useEffect(() => {
    void commands.getSettings().then((s) => {
      setSettings(s);
      void commands.hotkeyConflicts(s).then(setConflicts);
    });
    void commands.hasApiKey().then(setHasKey);
    void commands.defaultStyles().then(setDefaults);
    reloadHistory();
    // автозапуск могли перещёлкнуть из трея, историю — очистить
    const un = events.onSettingsChanged((s) => {
      if (pendingEcho.current > 0) {
        pendingEcho.current -= 1;
        return;
      }
      setSettings(s);
      void commands.hotkeyConflicts(s).then(setConflicts);
    });
    return () => void un.then((f) => f());
  }, [reloadHistory]);

  /** сохранить, показать «Сохранено» и принять нормализованный ответ */
  function apply(next: Settings) {
    setSettings(next);
    pendingEcho.current += 1;
    void commands.saveSettings(next).then((stored) => {
      setSettings(stored);
      void commands.hotkeyConflicts(stored).then(setConflicts);
    });
    setSaved(true);
    window.clearTimeout(savedTimer.current);
    savedTimer.current = window.setTimeout(() => setSaved(false), 1500);
  }

  async function saveKey() {
    setKeyError("");
    try {
      await commands.setApiKey(keyDraft);
      setKeyDraft(""); // ключ живёт только в keyring; в состоянии фронта не держим
      setHasKey(await commands.hasApiKey());
    } catch (e) {
      setKeyError(String(e));
    }
  }

  async function clearKey() {
    setKeyError("");
    try {
      await commands.clearApiKey();
      setHasKey(false);
    } catch (e) {
      setKeyError(String(e));
    }
  }

  if (!settings) return null;

  /** человеческое имя действия по id комбинации */
  function actionName(id: string): string {
    if (id === "main") return "Панель стилей";
    if (id === "undo") return "Вернуть предыдущий текст";
    const styleId = id.replace(/^style:/, "");
    return settings?.styles.find((s) => s.id === styleId)?.name ?? styleId;
  }

  /**
   * Конфликт комбинации (null — всё в порядке). В паре первым идёт тот, кто
   * реально сработает: хук отдаёт комбинацию первому подходящему действию.
   */
  function conflictWith(id: string): string | null {
    const loses = conflicts.find(([, b]) => b === id);
    if (loses) return `не сработает — занято: ${actionName(loses[0])}`;
    const wins = conflicts.find(([a]) => a === id);
    return wins ? `дублируется в «${actionName(wins[1])}»` : null;
  }

  function patchStyle(i: number, patch: Partial<Style>) {
    const styles = settings!.styles.map((s, n) => (n === i ? { ...s, ...patch } : s));
    apply({ ...settings!, styles });
  }

  function addStyle() {
    apply({
      ...settings!,
      styles: [
        ...settings!.styles,
        { id: "", name: "Новый стиль", instruction: "", hotkey: null, builtin: false },
      ],
    });
  }

  function removeStyle(i: number) {
    apply({ ...settings!, styles: settings!.styles.filter((_, n) => n !== i) });
  }

  return (
    <div className="min-h-screen bg-neutral-900 p-6 text-neutral-100">
      <div className="mb-4 flex items-baseline justify-between">
        <h1 className="text-xl font-semibold">Restyle — настройки</h1>
        <span className={`text-sm text-green-400 transition-opacity ${saved ? "opacity-100" : "opacity-0"}`}>
          Сохранено
        </span>
      </div>

      <Section title="Хоткеи">
        <Row label="Открыть панель стилей" note={conflictWith("main")}>
          <HotkeyField
            value={settings.mainHotkey}
            onChange={(c) => apply({ ...settings, mainHotkey: c ?? EMPTY_COMBO })}
          />
        </Row>
        <Row label="Вернуть предыдущий текст" note={conflictWith("undo")}>
          <HotkeyField
            value={settings.undoHotkey.key === 0 ? null : settings.undoHotkey}
            allowEmpty
            onChange={(c) => apply({ ...settings, undoHotkey: c ?? EMPTY_COMBO })}
          />
        </Row>
      </Section>

      <Section title="Общее">
        <Row label="Автозапуск">
          <input
            type="checkbox"
            checked={settings.autostart}
            onChange={(e) => apply({ ...settings, autostart: e.target.checked })}
          />
        </Row>
        <Row label="Вставлять сразу" note="для стилей с собственным хоткеем — без Enter">
          <input
            type="checkbox"
            checked={settings.autoPaste}
            onChange={(e) => apply({ ...settings, autoPaste: e.target.checked })}
          />
        </Row>
        <Row label="Модель">
          <div className="flex gap-2">
            <select
              className="rounded border border-neutral-600 bg-neutral-800 px-2 py-1 text-sm"
              value={MODEL_PRESETS.some((m) => m.id === settings.model) ? settings.model : "custom"}
              onChange={(e) => e.target.value !== "custom" && apply({ ...settings, model: e.target.value })}
            >
              {MODEL_PRESETS.map((m) => (
                <option key={m.id} value={m.id}>{m.label}</option>
              ))}
              <option value="custom">Другая…</option>
            </select>
            <DraftInput
              className="w-48 font-mono"
              value={settings.model}
              onCommit={(v) => apply({ ...settings, model: v })}
            />
          </div>
        </Row>
        <Row label="Тема">
          <select
            className="rounded border border-neutral-600 bg-neutral-800 px-2 py-1 text-sm"
            value={settings.theme}
            onChange={(e) => apply({ ...settings, theme: e.target.value })}
          >
            <option value="system">Системная</option>
            <option value="dark">Тёмная</option>
            <option value="light">Светлая</option>
          </select>
        </Row>
        <div className="space-y-1">
          <Row label="API-ключ Gemini">
            <span className="text-xs text-neutral-400">
              {hasKey === null ? "…" : hasKey ? "задан: ••••••••" : "не задан"}
            </span>
          </Row>
          <div className="flex gap-2">
            <input
              type="password"
              autoComplete="off"
              placeholder="вставьте ключ AI Studio"
              className="flex-1 rounded border border-neutral-600 bg-neutral-800 px-2 py-1 font-mono text-sm"
              value={keyDraft}
              onChange={(e) => setKeyDraft(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && keyDraft && void saveKey()}
            />
            <button
              type="button"
              disabled={!keyDraft.trim()}
              className="rounded bg-blue-600 px-3 py-1 text-sm disabled:opacity-40"
              onClick={() => void saveKey()}
            >
              Сохранить
            </button>
            {hasKey && (
              <button type="button" className="rounded bg-white/10 px-3 py-1 text-sm" onClick={() => void clearKey()}>
                Удалить
              </button>
            )}
          </div>
          {keyError && <div className="text-xs text-red-400">{keyError}</div>}
          <div className="text-xs text-neutral-500">Хранится в Windows Credential Manager, в файлы настроек не пишется.</div>
        </div>
      </Section>

      <Section title="Стили">
        <div className="space-y-3">
          {settings.styles.map((s, i) => {
            const def = defaults.find((d) => d.id === s.id);
            const changed = def && (def.name !== s.name || def.instruction !== s.instruction);
            return (
              <div key={`${s.id}-${i}`} className="rounded border border-neutral-800 p-2">
                <div className="flex items-center gap-2">
                  <span className="w-6 text-center text-xs text-neutral-500">{i + 1}</span>
                  <DraftInput
                    className="flex-1"
                    value={s.name}
                    onCommit={(v) => patchStyle(i, { name: v })}
                  />
                  <HotkeyField
                    value={s.hotkey}
                    allowEmpty
                    onChange={(c) => patchStyle(i, { hotkey: c })}
                  />
                  {s.builtin ? (
                    <button
                      type="button"
                      disabled={!changed}
                      title="Вернуть исходные имя и инструкцию"
                      className="rounded bg-white/10 px-2 py-1 text-xs disabled:opacity-30"
                      onClick={() => def && patchStyle(i, { name: def.name, instruction: def.instruction })}
                    >
                      Вернуть
                    </button>
                  ) : (
                    <button
                      type="button"
                      title="Удалить стиль"
                      className="rounded bg-red-900/60 px-2 py-1 text-xs"
                      onClick={() => removeStyle(i)}
                    >
                      Удалить
                    </button>
                  )}
                </div>
                <DraftInput
                  multiline
                  rows={2}
                  className="mt-2 w-full"
                  placeholder="Инструкция для модели (на английском работает точнее)"
                  value={s.instruction}
                  onCommit={(v) => patchStyle(i, { instruction: v })}
                />
                <div className="mt-1 flex justify-between text-xs text-neutral-500">
                  <span className="font-mono">
                    id: {s.id}
                    {s.builtin ? " · встроенный" : ""}
                  </span>
                  <span className="text-amber-400">{conflictWith(`style:${s.id}`)}</span>
                </div>
              </div>
            );
          })}
        </div>
        <button type="button" className="rounded bg-white/10 px-3 py-1 text-sm" onClick={addStyle}>
          + Добавить стиль
        </button>
        <p className="text-xs text-neutral-500">
          Хоткей стиля переписывает текст сразу выбранным стилем, без панели.
          Встроенные стили можно править, но не удалять.
        </p>
      </Section>

      <Section title="Приватность">
        <Row label="Снимок экрана как контекст" note="что за приложение, кому и о чём пишем">
          <input
            type="checkbox"
            checked={settings.screenshotEnabled}
            onChange={(e) => apply({ ...settings, screenshotEnabled: e.target.checked })}
          />
        </Row>
        <Row label="Что снимать">
          <select
            className="rounded border border-neutral-600 bg-neutral-800 px-2 py-1 text-sm disabled:opacity-40"
            disabled={!settings.screenshotEnabled}
            value={settings.screenshotMode}
            onChange={(e) => apply({ ...settings, screenshotMode: e.target.value })}
          >
            <option value="window">Активное окно</option>
            <option value="monitor">Весь монитор</option>
          </select>
        </Row>
        <ProcessList
          label="Без скриншота в этих программах"
          hint="имена exe, по одному в строке; менеджеры паролей и банки — уже в списке"
          value={settings.screenshotExcludedProcesses}
          onCommit={(v) => apply({ ...settings, screenshotExcludedProcesses: v })}
        />
        <ProcessList
          label="Брать только выделение"
          hint="редакторы и IDE, где Ctrl+A забрал бы весь документ"
          value={settings.selectOnlyProcesses}
          onCommit={(v) => apply({ ...settings, selectOnlyProcesses: v })}
        />
      </Section>

      <Section title="История">
        <Row label="Хранить на диске" note="выключено — история живёт только в памяти до выхода">
          <input
            type="checkbox"
            checked={settings.historyToDisk}
            onChange={(e) => apply({ ...settings, historyToDisk: e.target.checked })}
          />
        </Row>
        <div className="flex gap-2">
          <button type="button" className="rounded bg-white/10 px-3 py-1 text-sm" onClick={reloadHistory}>
            Обновить
          </button>
          <button
            type="button"
            disabled={history.length === 0}
            className="rounded bg-white/10 px-3 py-1 text-sm disabled:opacity-30"
            onClick={() => {
              void commands.clearHistory();
              setTimeout(reloadHistory, 100);
            }}
          >
            Очистить
          </button>
        </div>
        {history.length === 0 ? (
          <p className="text-xs text-neutral-500">Пока пусто — история пополняется после вставки результата.</p>
        ) : (
          <ul className="space-y-1">
            {history.map((h, i) => (
              <li key={`${h.at}-${i}`} className="rounded border border-neutral-800 p-2 text-sm">
                <div className="flex items-baseline justify-between gap-2 text-xs text-neutral-500">
                  <span>
                    {new Date(h.at).toLocaleTimeString()} · {h.targetExe} ·{" "}
                    {settings.styles.find((s) => s.id === h.styleId)?.name ?? h.styleId}
                  </span>
                  <button
                    type="button"
                    className="rounded bg-white/10 px-2 py-0.5"
                    onClick={() => void commands.copyHistory(i)}
                  >
                    В буфер
                  </button>
                </div>
                <div className="line-clamp-2 whitespace-pre-wrap break-words text-neutral-200">{h.result}</div>
                <div className="line-clamp-1 whitespace-pre-wrap break-words text-neutral-500">было: {h.original}</div>
              </li>
            ))}
          </ul>
        )}
      </Section>
    </div>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="mb-6 space-y-3">
      <h2 className="text-sm uppercase tracking-wide text-neutral-400">{title}</h2>
      {children}
    </section>
  );
}

function Row({ label, note, children }: { label: string; note?: string | null; children: React.ReactNode }) {
  return (
    <label className="flex items-center justify-between gap-4">
      <span className="text-sm">
        {label}
        {note && <span className="ml-2 text-xs text-amber-400">{note}</span>}
      </span>
      {children}
    </label>
  );
}

/** Список имён процессов: одна строка — один exe. */
function ProcessList({
  label,
  hint,
  value,
  onCommit,
}: {
  label: string;
  hint: string;
  value: string[];
  onCommit: (v: string[]) => void;
}) {
  return (
    <div className="space-y-1">
      <div className="text-sm">{label}</div>
      <DraftInput
        multiline
        rows={4}
        className="w-full font-mono text-xs"
        value={value.join("\n")}
        onCommit={(v) => onCommit(v.split("\n").map((s) => s.trim()).filter(Boolean))}
      />
      <div className="text-xs text-neutral-500">{hint}</div>
    </div>
  );
}
