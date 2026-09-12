import { useCallback, useEffect, useState } from "react";
import { commands, type DeeplUsage, type ModelInfo, type Settings } from "../../lib/ipc";
import type { T } from "../../lib/i18n";
import { Button, SectionTitle, Segmented, Select } from "../../ui/kit";

/** Поле ключа — обычный управляемый input, а не DraftField: тот отдаёт значение
 *  только по потере фокуса, и кнопка рядом оставалась неактивной, пока ключ уже вписан. */
const KEY_INPUT =
  "h-9 flex-1 rounded-md border border-black/[.13] bg-white px-3 font-mono text-[13px] text-paper-text outline-none transition-colors placeholder:text-paper-dim/70 focus:border-amber-deep focus:shadow-[0_0_0_2px_rgb(var(--amber-deep)/.13)] dark:border-white/10 dark:bg-white/[.05] dark:text-ink-text dark:placeholder:text-ink-text/35 dark:focus:border-amber";

const PRESETS = [
  { id: "gemini-3.5-flash-lite", hintKey: "model.lite" as const },
  { id: "gemini-3.6-flash", hintKey: "model.flash" as const },
];

/**
 * «Модель и ключ» (макет, раздел 03). Ключ показываем только фактом наличия:
 * из keyring он не возвращается никогда, поэтому маски вида «AIza••••7dQ2»
 * в интерфейсе нет — вместо неё состояние и кнопка «Заменить».
 */
export default function ModelKey({
  t,
  settings,
  apply,
}: {
  t: T;
  settings: Settings;
  apply: (s: Settings) => void;
}) {
  const [hasKey, setHasKey] = useState<boolean | null>(null);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const [state, setState] = useState<"idle" | "checking" | "ok" | "error">("idle");
  const [error, setError] = useState("");
  const [models, setModels] = useState<ModelInfo[] | null>(null);
  const [modelsError, setModelsError] = useState("");

  // Каталог моделей у каждого ключа свой (2.5 новым ключам недоступна),
  // поэтому тянем его у API, а не показываем зашитый список.
  const loadModels = useCallback(() => {
    setModelsError("");
    setModels(null);
    void commands
      .listModels()
      .then(setModels)
      .catch((e) => {
        setModels([]);
        setModelsError(String(e));
      });
  }, []);

  useEffect(() => {
    void commands.hasApiKey().then((v) => {
      setHasKey(v);
      setEditing(!v);
      if (v) loadModels();
    });
  }, [loadModels]);

  async function saveAndCheck() {
    const key = draft.trim();
    if (!key) return;
    setState("checking");
    setError("");
    try {
      await commands.testApiKey(key);
      await commands.setApiKey(key);
      setDraft("");
      setHasKey(true);
      setEditing(false);
      setState("ok");
      loadModels();
    } catch (e) {
      setState("error");
      setError(String(e));
    }
  }

  const custom = !PRESETS.some((p) => p.id === settings.model);

  return (
    <div className="flex h-full flex-col gap-6">
      <div className="flex flex-col gap-3">
        <SectionTitle title={t("key.title")} hint={t("key.hint")} />
        {editing ? (
          <div className="flex flex-col gap-2">
            <div className="flex items-center gap-2.5">
              <input
                type="password"
                autoComplete="off"
                spellCheck={false}
                autoFocus
                className={KEY_INPUT}
                value={draft}
                placeholder={t("key.placeholder")}
                onChange={(e) => setDraft(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && void saveAndCheck()}
              />
              <Button kind="primary" onClick={() => void saveAndCheck()} disabled={state === "checking" || !draft.trim()}>
                {t("key.check")}
              </Button>
              {hasKey && (
                <Button onClick={() => { setEditing(false); setState("idle"); }}>{t("common.cancel")}</Button>
              )}
            </div>
            <StatusLine t={t} state={state} error={error} />
          </div>
        ) : (
          <div className="flex items-center gap-2.5">
            <div className="flex h-9 flex-1 items-center gap-2.5 rounded-md border border-black/[.14] bg-white px-3 dark:border-white/10 dark:bg-white/[.05]">
              <span className="flex-1 font-mono text-[13px] tracking-[.06em] text-[#33363c] dark:text-ink-text/80">
                {hasKey === null ? "…" : hasKey ? "AIza••••••••••••••••••••••••••••" : "—"}
              </span>
            </div>
            <Button onClick={() => { setDraft(""); setEditing(true); setState("idle"); }}>{t("key.replace")}</Button>
            {hasKey && (
              <Button
                kind="danger"
                onClick={() => void commands.clearApiKey().then(() => { setHasKey(false); setEditing(true); })}
              >
                {t("common.delete")}
              </Button>
            )}
          </div>
        )}
        {!editing && <StatusLine t={t} state={hasKey ? "ok" : "idle"} error="" storedOnly={state === "idle"} hasKey={!!hasKey} />}
      </div>

      <div className="h-px bg-black/[.08] dark:bg-white/[.07]" />

      <div className="flex flex-col gap-3">
        <SectionTitle title={t("model.title")} hint={t("model.hint")} />
        <div className="flex flex-col gap-[7px]">
          {PRESETS.map((p) => (
            <ModelRow
              key={p.id}
              id={p.id}
              note={t(p.hintKey)}
              checked={settings.model === p.id}
              onPick={() => apply({ ...settings, model: p.id })}
            />
          ))}
          <div
            role="radio"
            aria-checked={custom}
            onClick={() => !custom && models?.[0] && apply({ ...settings, model: models[0].id })}
            className={`flex cursor-pointer items-center gap-3 rounded-lg border bg-white p-3 transition-colors dark:bg-white/[.04] ${
              custom
                ? "border-amber-deep shadow-[0_0_0_2px_rgb(var(--amber-deep)/.13)] dark:border-amber"
                : "border-black/[.12] hover:border-black/25 dark:border-white/[.08] dark:hover:border-white/20"
            }`}
          >
            {/* Выбор — кликом по всей строке, кружок только показывает состояние. */}
            <Radio checked={custom} onPick={() => {}} />
            <span className="flex flex-col gap-0.5">
              <span className="text-[13px] text-paper-text dark:text-ink-text">{t("model.list")}</span>
              <span className="text-[11px] text-paper-dim dark:text-ink-text/50">
                {modelsError || (models === null ? t("model.loading") : hasKey ? t("model.listHint") : t("model.needKey"))}
              </span>
            </span>
            {/* Список и «Обновить» кликаются сами по себе и строку не выбирают. */}
            <span className="ml-auto flex cursor-default items-center gap-2" onClick={(e) => e.stopPropagation()}>
              <Select
                className="w-[230px]"
                disabled={!models || models.length === 0}
                value={custom ? settings.model : ""}
                onChange={(v) => v && apply({ ...settings, model: v })}
                options={[
                  { value: "", label: models && models.length ? "—" : t("model.loading") },
                  ...(models ?? []).map((m) => ({
                    value: m.id,
                    label: m.id,
                    hint: m.fast ? t("model.fast") : t("model.heavy"),
                  })),
                ]}
              />
              <Button onClick={loadModels} disabled={!hasKey}>
                {t("model.refresh")}
              </Button>
            </span>
          </div>
        </div>
      </div>

      <div className="h-px bg-black/[.08] dark:bg-white/[.07]" />

      <Deepl t={t} settings={settings} apply={apply} />

      <div className="mt-auto flex items-center gap-3 rounded-lg border border-amber-deep/[.22] bg-amber-deep/[.09] p-3 dark:border-amber/20 dark:bg-amber/[.08]">
        <span className="font-mono text-xs font-medium text-amber-mid dark:text-amber">i</span>
        <span className="text-xs leading-relaxed text-[#5c4a2c] dark:text-ink-text/70">{t("model.provider")}</span>
      </div>
    </div>
  );
}

/**
 * Перевод отдельным движком. Ключ DeepL живёт в Credential Manager рядом с
 * ключом Gemini; без него переводит модель — работать приложение не перестаёт.
 */
function Deepl({ t, settings, apply }: { t: T; settings: Settings; apply: (s: Settings) => void }) {
  const [has, setHas] = useState<boolean | null>(null);
  const [draft, setDraft] = useState("");
  const [usage, setUsage] = useState<DeeplUsage | null>(null);
  const [state, setState] = useState<"idle" | "checking" | "ok" | "error">("idle");
  const [error, setError] = useState("");

  useEffect(() => {
    void commands.hasDeeplKey().then((v) => {
      setHas(v);
      if (v) {
        void commands
          .testDeeplKey("")
          .then((u) => {
            setUsage(u);
            setState("ok");
          })
          .catch(() => {});
      }
    });
  }, []);

  async function saveAndCheck() {
    const key = draft.trim();
    if (!key) return;
    setState("checking");
    setError("");
    try {
      const u = await commands.testDeeplKey(key);
      await commands.setDeeplKey(key);
      setUsage(u);
      setHas(true);
      // Ключ остаётся в поле: видно, что именно сохранено.
      setState("ok");
    } catch (e) {
      setState("error");
      setError(String(e));
    }
  }

  return (
    <div className="flex flex-col gap-3">
      <SectionTitle title={t("tr.title")} hint={t("tr.hint")} />
      <div className="flex items-center gap-3">
        <span className="text-[13px] text-paper-text dark:text-ink-text">{t("tr.engine")}</span>
        <Segmented
          value={settings.translator === "gemini" ? "gemini" : "deepl"}
          onChange={(v) => apply({ ...settings, translator: v })}
          options={[
            { value: "deepl", label: t("tr.deepl") },
            { value: "gemini", label: t("tr.gemini") },
          ]}
        />
      </div>
      {settings.translator !== "gemini" && (
        <div className="flex flex-col gap-2">
          <div className="flex items-center gap-2.5">
            <input
              type="password"
              autoComplete="off"
              spellCheck={false}
              className={KEY_INPUT}
              value={draft}
              placeholder={has ? "••••••••••••••••••••:fx" : "xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx:fx"}
              onChange={(e) => {
                setDraft(e.target.value);
                if (state === "error") setState("idle");
              }}
              onKeyDown={(e) => e.key === "Enter" && void saveAndCheck()}
            />
            {/* «Сохранить» = проверка живым запросом и запись в Credential Manager. */}
            <Button kind="primary" onClick={() => void saveAndCheck()} disabled={state === "checking" || !draft.trim()}>
              {state === "checking" ? t("key.checking") : t("tr.save")}
            </Button>
            {has && (
              <Button
                kind="danger"
                onClick={() =>
                  void commands.clearDeeplKey().then(() => {
                    setHas(false);
                    setUsage(null);
                    setState("idle");
                  })
                }
              >
                {t("common.delete")}
              </Button>
            )}
          </div>
          <div className="flex items-center gap-2">
            <span
              className={`h-1.5 w-1.5 flex-none rounded-full ${
                state === "error" ? "bg-danger" : has ? "bg-ok" : "bg-black/25 dark:bg-white/25"
              }`}
            />
            <span
              className={`text-[11.5px] ${
                state === "error" ? "text-danger-ink dark:text-danger" : "text-paper-mid dark:text-ink-text/60"
              }`}
            >
              {state === "error"
                ? error
                : usage
                  ? t("tr.ok", {
                      used: usage.characterCount.toLocaleString(),
                      limit: usage.characterLimit.toLocaleString(),
                    })
                  : has
                    ? t("tr.set")
                    : t("tr.unset")}
            </span>
            {!has && (
              <a
                href="https://www.deepl.com/pro-api"
                onClick={(e) => {
                  e.preventDefault();
                  void commands.openUrl("https://www.deepl.com/pro-api");
                }}
                className="ml-auto text-[11.5px] text-amber-ink hover:underline dark:text-amber"
              >
                {t("tr.get")}
              </a>
            )}
          </div>
          {!has && <span className="text-[11px] text-paper-dim dark:text-ink-text/45">{t("tr.free")}</span>}
        </div>
      )}
    </div>
  );
}

function StatusLine({
  t,
  state,
  error,
  storedOnly = false,
  hasKey = false,
}: {
  t: T;
  state: "idle" | "checking" | "ok" | "error";
  error: string;
  storedOnly?: boolean;
  hasKey?: boolean;
}) {
  if (storedOnly) {
    return (
      <div className="flex items-center gap-2">
        <span className={`h-1.5 w-1.5 rounded-full ${hasKey ? "bg-ok" : "bg-black/25 dark:bg-white/25"}`} />
        <span className="text-[11.5px] text-paper-mid dark:text-ink-text/60">{hasKey ? t("key.set") : t("key.unset")}</span>
      </div>
    );
  }
  if (state === "idle") return null;
  const color = state === "ok" ? "bg-ok" : state === "error" ? "bg-danger" : "bg-amber-deep animate-pulse2 dark:bg-amber";
  const label = state === "ok" ? t("key.ok") : state === "error" ? error : t("key.checking");
  return (
    <div className="flex items-center gap-2">
      <span className={`h-1.5 w-1.5 flex-none rounded-full ${color}`} />
      <span className={`text-[11.5px] ${state === "error" ? "text-danger-ink dark:text-danger" : "text-paper-mid dark:text-ink-text/60"}`}>
        {label}
      </span>
    </div>
  );
}

function ModelRow({ id, note, checked, onPick }: { id: string; note: string; checked: boolean; onPick: () => void }) {
  return (
    <button
      type="button"
      onClick={onPick}
      className={`flex items-center gap-3 rounded-lg border bg-white p-3 text-left transition-colors dark:bg-white/[.04] ${
        checked
          ? "border-amber-deep shadow-[0_0_0_2px_rgb(var(--amber-deep)/.13)] dark:border-amber"
          : "border-black/[.12] hover:border-black/25 dark:border-white/[.08] dark:hover:border-white/20"
      }`}
    >
      <Radio checked={checked} onPick={onPick} />
      <span className="flex flex-1 flex-col gap-0.5">
        <span className="font-mono text-[13px] font-medium leading-tight text-paper-text dark:text-ink-text">{id}</span>
      </span>
      <span className="font-mono text-[11px] text-paper-soft dark:text-ink-text/50">{note}</span>
    </button>
  );
}

function Radio({ checked, onPick }: { checked: boolean; onPick: () => void }) {
  return (
    <span
      role="radio"
      aria-checked={checked}
      onClick={onPick}
      className={`h-[15px] w-[15px] flex-none rounded-full bg-white ${
        checked ? "border-4 border-amber-deep dark:border-amber" : "border border-black/25 dark:border-white/30 dark:bg-transparent"
      }`}
    />
  );
}

export { Radio };
