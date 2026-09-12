import { useEffect, useRef, useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import { commands, events, type Settings } from "../lib/ipc";
import { useAppShell } from "../lib/theme";
import { comboLabel, EMPTY_COMBO, comboOrNull } from "../lib/hotkey";
import type { T } from "../lib/i18n";
import HotkeyField from "../settings/HotkeyField";
import { Button, KeyCaps, TitleBar, Toggle } from "../ui/kit";

/**
 * Мастер первого запуска (макет, раздел 05): ключ с проверкой живым запросом,
 * комбинации, пробный прогон на настоящем поле ввода. Третий шаг ждёт, пока
 * пользователь нажмёт хоткей прямо в поле ниже: события `overlay:text` и
 * `overlay:screenshot` и есть доказательство, что цепочка работает.
 */
export default function WelcomeApp() {
  const { t, settings, lang } = useAppShell();
  const [step, setStep] = useState(0);
  const [local, setLocal] = useState<Settings | null>(null);
  useEffect(() => {
    if (settings && !local) setLocal(settings);
  }, [settings, local]);

  useEffect(() => {
    document.documentElement.classList.add("app-window");
  }, []);

  if (!local) return null;

  const apply = (next: Settings) => {
    setLocal(next);
    void commands.saveSettings(next);
  };

  return (
    <div className="flex h-screen flex-col bg-paper text-paper-text dark:bg-ink-800 dark:text-ink-text">
      <TitleBar title="Restyle" minimize={false} t={t} />
      <div className="flex min-h-0 flex-1 flex-col gap-5 px-8 py-7">
        <div className="flex gap-1.5">
          {[0, 1, 2].map((i) => (
            <span
              key={i}
              className={`h-[3px] flex-1 rounded ${
                i <= step ? "bg-amber-deep dark:bg-amber" : "bg-black/[.12] dark:bg-white/15"
              }`}
            />
          ))}
        </div>

        <AnimatePresence mode="wait" initial={false}>
          <motion.div
            key={step}
            initial={{ opacity: 0, x: 8 }}
            animate={{ opacity: 1, x: 0 }}
            exit={{ opacity: 0, x: -8 }}
            transition={{ duration: 0.14 }}
            className="flex min-h-0 flex-1 flex-col gap-5"
          >
            {/* «Пропустить» на первом шаге пропускает ШАГ, а не мастер целиком:
                ключ можно вставить позже в настройках, хоткеи и проверку
                показать всё равно стоит. */}
            {step === 0 && <StepKey t={t} onNext={() => setStep(1)} onSkip={() => setStep(1)} />}
            {step === 1 && (
              <StepHotkeys t={t} lang={lang} settings={local} apply={apply} onBack={() => setStep(0)} onNext={() => setStep(2)} />
            )}
            {step === 2 && <StepTest t={t} settings={local} apply={apply} onBack={() => setStep(1)} />}
          </motion.div>
        </AnimatePresence>
      </div>
    </div>
  );
}

function finish() {
  void commands.finishOnboarding();
}

/** Шапка шага: «ШАГ N ИЗ 3», заголовок, пояснение. */
function Title({ caption, heading, hint }: { caption: string; heading: string; hint: React.ReactNode }) {
  return (
    <div className="flex flex-col gap-2">
      <span className="font-mono text-[10.5px] font-medium uppercase tracking-[.1em] text-amber-mid dark:text-amber">
        {caption}
      </span>
      <h2 className="m-0 text-[22px] font-semibold leading-tight tracking-[-.01em] text-paper-text dark:text-ink-text">
        {heading}
      </h2>
      <p className="m-0 text-[13px] leading-relaxed text-paper-dim dark:text-ink-text/60">{hint}</p>
    </div>
  );
}

function StepKey({ t, onNext, onSkip }: { t: T; onNext: () => void; onSkip: () => void }) {
  const [key, setKey] = useState("");
  const [state, setState] = useState<"idle" | "checking" | "ok" | "error">("idle");
  const [error, setError] = useState("");

  useEffect(() => {
    void commands.hasApiKey().then((has) => has && setState("ok"));
  }, []);

  async function check() {
    const v = key.trim();
    if (!v) return;
    setState("checking");
    setError("");
    try {
      await commands.testApiKey(v);
      await commands.setApiKey(v);
      setState("ok");
    } catch (e) {
      setState("error");
      setError(String(e));
    }
  }

  return (
    <>
      <Title caption={t("wiz.step", { n: 1 })} heading={t("wiz.key.title")} hint={t("wiz.key.hint")} />
      <div className="flex flex-col gap-2.5">
        <input
          autoFocus
          type="password"
          autoComplete="off"
          value={key}
          onChange={(e) => setKey(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && void check()}
          placeholder="AIza…"
          className="h-10 rounded-lg border border-black/[.14] bg-white px-3 font-mono text-[13px] text-paper-text outline-none focus:border-amber-deep focus:shadow-[0_0_0_2px_rgb(var(--amber-deep)/.13)] dark:border-white/10 dark:bg-white/[.05] dark:text-ink-text dark:focus:border-amber"
        />
        <div className="flex items-center gap-2">
          {state !== "idle" && (
            <span
              className={`h-1.5 w-1.5 flex-none rounded-full ${
                state === "ok" ? "bg-ok" : state === "error" ? "bg-danger" : "animate-pulse2 bg-amber-deep dark:bg-amber"
              }`}
            />
          )}
          <span className={`text-[11.5px] ${state === "error" ? "text-danger-ink dark:text-danger" : "text-paper-mid dark:text-ink-text/60"}`}>
            {state === "ok" ? t("key.ok") : state === "checking" ? t("key.checking") : state === "error" ? error : ""}
          </span>
          <Button onClick={() => void check()} disabled={!key.trim() || state === "checking"}>
            {t("key.check")}
          </Button>
        </div>
      </div>
      <div className="mt-auto flex items-center gap-2.5">
        <button type="button" onClick={onSkip} className="text-[12.5px] text-paper-dim hover:underline dark:text-ink-text/55">
          {t("common.skip")}
        </button>
        <span className="ml-auto">
          <Button kind="primary" onClick={onNext}>
            {t("common.next")}
          </Button>
        </span>
      </div>
    </>
  );
}

function StepHotkeys({
  t,
  lang,
  settings,
  apply,
  onBack,
  onNext,
}: {
  t: T;
  lang: string;
  settings: Settings;
  apply: (s: Settings) => void;
  onBack: () => void;
  onNext: () => void;
}) {
  const [quickId, setQuickId] = useState(settings.styles.find((s) => s.hotkey)?.id ?? settings.styles[0]?.id ?? "");
  const quick = settings.styles.find((s) => s.id === quickId);
  return (
    <>
      <Title caption={t("wiz.step", { n: 2 })} heading={t("wiz.hotkeys.title")} hint={t("wiz.hotkeys.hint")} />
      <div className="flex flex-col gap-2">
        <div className="flex items-center gap-3 rounded-lg border border-black/10 bg-white p-3.5 dark:border-white/[.08] dark:bg-white/[.04]">
          <span className="flex flex-1 flex-col gap-[3px]">
            <span className="text-[13px] font-medium leading-tight text-paper-text dark:text-ink-text">{t("hotkeys.main")}</span>
            <span className="text-[11px] leading-tight text-paper-dim dark:text-ink-text/50">основная комбинация</span>
          </span>
          <HotkeyField
            lang={lang}
            value={settings.mainHotkey}
            onChange={(c) => apply({ ...settings, mainHotkey: c ?? EMPTY_COMBO })}
          />
        </div>

        <div className="flex items-center gap-3 rounded-lg border border-black/10 bg-white p-3.5 dark:border-white/[.08] dark:bg-white/[.04]">
          <span className="flex flex-1 flex-col gap-[3px]">
            <span className="text-[13px] font-medium leading-tight text-paper-text dark:text-ink-text">
              {t("hotkeys.quick", { n: "" }).replace(/\s*—\s*$/, "")}
            </span>
            <select
              value={quickId}
              onChange={(e) => setQuickId(e.target.value)}
              className="w-fit rounded-md bg-amber-deep/[.13] px-2 py-0.5 text-[11px] font-medium text-amber-ink outline-none dark:bg-amber/[.16] dark:text-amber-soft"
            >
              {settings.styles.map((s) => (
                <option key={s.id} value={s.id}>
                  {s.name}
                </option>
              ))}
            </select>
          </span>
          <HotkeyField
            lang={lang}
            allowEmpty
            value={quick?.hotkey ?? null}
            onChange={(c) =>
              apply({ ...settings, styles: settings.styles.map((s) => (s.id === quickId ? { ...s, hotkey: c } : s)) })
            }
          />
        </div>

        <div className="flex items-center gap-3 rounded-lg border border-black/10 bg-white p-3.5 dark:border-white/[.08] dark:bg-white/[.04]">
          <span className="flex flex-1 flex-col gap-[3px]">
            <span className="text-[13px] font-medium leading-tight text-paper-text dark:text-ink-text">{t("hotkeys.undo")}</span>
            <span className="text-[11px] leading-tight text-paper-dim dark:text-ink-text/50">откат последней вставки</span>
          </span>
          <HotkeyField
            lang={lang}
            allowEmpty
            value={comboOrNull(settings.undoHotkey)}
            onChange={(c) => apply({ ...settings, undoHotkey: c ?? EMPTY_COMBO })}
          />
        </div>
      </div>
      <div className="rounded-md border border-black/[.07] bg-black/[.035] px-3 py-2.5 text-[11.5px] leading-snug text-paper-mid dark:border-white/[.07] dark:bg-white/[.03] dark:text-ink-text/65">
        {t("wiz.hotkeys.rest")}
      </div>
      <div className="mt-auto flex items-center gap-2.5">
        <button type="button" onClick={onBack} className="text-[12.5px] text-paper-dim hover:underline dark:text-ink-text/55">
          {t("common.back")}
        </button>
        <span className="ml-auto">
          <Button kind="primary" onClick={onNext}>
            {t("common.next")}
          </Button>
        </span>
      </div>
    </>
  );
}

function StepTest({
  t,
  settings,
  apply,
  onBack,
}: {
  t: T;
  settings: Settings;
  apply: (s: Settings) => void;
  onBack: () => void;
}) {
  const [hookOk, setHookOk] = useState(false);
  const [textOk, setTextOk] = useState(false);
  const [shotMs, setShotMs] = useState<number | null>(null);
  const area = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    void commands.diagnostics().then((d) => setHookOk(d.hookInstalled));
    area.current?.focus();
    const uns = [
      events.onText(() => setTextOk(true)),
      events.onScreenshot((s) => setShotMs(s.captureMs + s.encodeMs)),
    ];
    return () => {
      for (const u of uns) void u.then((f) => f());
    };
  }, []);

  return (
    <>
      <Title
        caption={t("wiz.step", { n: 3 })}
        heading={t("wiz.test.title")}
        // Хоткей вставляется в середину фразы, поэтому строку режем по {n}
        hint={(() => {
          const [before, after] = t("wiz.test.hint").split("{n}");
          // Справа отступ не нужен, если дальше идёт знак препинания
          const tail = /^[.,;:!?]/.test(after) ? "" : " mr-1";
          return (
            <>
              {before.trimEnd()}
              <span className={`ml-1 inline-flex align-middle${tail}`}>
                <KeyCaps combo={comboLabel(settings.mainHotkey)} />
              </span>
              {after}
            </>
          );
        })()}
      />
      <textarea
        ref={area}
        defaultValue={t("wiz.test.placeholder")}
        className="min-h-[74px] resize-none rounded-lg border border-black/[.13] bg-white p-3 text-[13px] leading-relaxed text-paper-text outline-none focus:border-amber-deep dark:border-white/10 dark:bg-white/[.05] dark:text-ink-text dark:focus:border-amber"
      />
      <div className="flex flex-col gap-2">
        <Check ok={hookOk} label={t("wiz.check.hook")} />
        <Check ok={textOk} label={textOk ? t("wiz.check.text") : t("wiz.check.pending")} />
        <Check
          ok={shotMs !== null}
          label={`${t("wiz.check.shot")}${shotMs !== null ? ` ${shotMs.toFixed(0)} мс` : ""}`}
        />
      </div>
      <div className="mt-auto flex items-center gap-2.5">
        <button type="button" onClick={onBack} className="text-[12.5px] text-paper-dim hover:underline dark:text-ink-text/55">
          {t("common.back")}
        </button>
        <span className="flex items-center gap-2">
          <Toggle size="sm" checked={settings.autostart} onChange={(v) => apply({ ...settings, autostart: v })} />
          <span className="text-[12.5px] text-paper-mid dark:text-ink-text/70">{t("wiz.autostart")}</span>
        </span>
        <span className="ml-auto">
          <Button kind="primary" onClick={finish}>
            {t("wiz.finish")}
          </Button>
        </span>
      </div>
    </>
  );
}

function Check({ ok, label }: { ok: boolean; label: string }) {
  return (
    <div className="flex items-center gap-2.5">
      <span
        className={`flex h-[15px] w-[15px] flex-none items-center justify-center rounded font-mono text-[9px] font-medium text-white ${
          ok ? "bg-ok" : "bg-black/15 dark:bg-white/20"
        }`}
      >
        {ok ? "✓" : ""}
      </span>
      <span className={`text-[12.5px] ${ok ? "text-paper-text dark:text-ink-text/90" : "text-paper-dim dark:text-ink-text/50"}`}>
        {label}
      </span>
    </div>
  );
}
