import { useEffect, useRef, useState } from "react";
import type { HotkeyCombo } from "../bindings/HotkeyCombo";
import { commands } from "../lib/ipc";
import { EMPTY_COMBO, codeToVk, comboLabel } from "../lib/hotkey";
import { KeyCaps } from "../ui/kit";
import { resolveLang, translator } from "../lib/i18n";

/**
 * Поле хоткея с захватом реального нажатия (схема из HoldMix, вид — из макета:
 * клавиши капсами, режим захвата — пунктирная рамка с «Жду нажатие…»).
 * Клик — режим захвата, зажатые клавиши накапливаются, отпускание первой
 * фиксирует комбинацию. Esc — отмена, Backspace/Delete — очистить.
 *
 * На время захвата LL-хук приостанавливается, и снятие паузы обязано пережить
 * размонтирование (строку стиля можно удалить прямо во время захвата) — иначе
 * хук остался бы выключенным до перезапуска. Закрытие окна крестиком React не
 * размонтирует: там паузу снимает бэкенд по WindowEvent::Destroyed.
 *
 * Захват в один момент времени ведёт только одно поле: их на экране много,
 * а слушатели у всех глобальные.
 */
let activeStop: (() => void) | null = null;

function beginCapture(stop: () => void) {
  if (activeStop && activeStop !== stop) activeStop();
  activeStop = stop;
  void commands.setHotkeyCapture(true);
}

function endCapture(stop: () => void) {
  if (activeStop !== stop) return;
  activeStop = null;
  void commands.setHotkeyCapture(false);
}

export default function HotkeyField({
  value,
  onChange,
  allowEmpty = false,
  lang,
  fill = false,
}: {
  value: HotkeyCombo | null;
  onChange: (c: HotkeyCombo | null) => void;
  allowEmpty?: boolean;
  lang?: string;
  /** Растянуть на всю ширину контейнера (поле «Хоткей» в редакторе стиля). */
  fill?: boolean;
}) {
  const t = translator(resolveLang(lang));
  const [capturing, setCapturing] = useState(false);
  const [hint, setHint] = useState("");
  const capRef = useRef({ ctrl: false, alt: false, shift: false, win: false, keys: [] as number[] });
  /** Прошлый одиночный тап модификатора — для «Ctrl, Ctrl». */
  const tapRef = useRef<{ m: "ctrl" | "alt" | "shift" | "win"; t: number } | null>(null);
  const stopRef = useRef<() => void>(() => {});
  stopRef.current = () => setCapturing(false);
  const selfRef = useRef(() => stopRef.current());

  useEffect(() => {
    const self = selfRef.current;
    if (!capturing) {
      endCapture(self);
      return;
    }
    beginCapture(self);
    setHint("");
    capRef.current = { ctrl: false, alt: false, shift: false, win: false, keys: [] };
    tapRef.current = null;

    const onDown = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === "Escape") return setCapturing(false);
      if (allowEmpty && (e.key === "Backspace" || e.key === "Delete")) {
        setCapturing(false);
        onChange(null);
        return;
      }
      const c = capRef.current;
      if (e.key === "Control") c.ctrl = true;
      else if (e.key === "Alt") c.alt = true;
      else if (e.key === "Shift") c.shift = true;
      else if (e.key === "Meta") c.win = true;
      else {
        const vk = codeToVk(e);
        if (vk !== null && !c.keys.includes(vk)) c.keys.push(vk);
      }
    };
    const onUp = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      const c = capRef.current;
      const mods = [c.ctrl, c.alt, c.shift, c.win].filter(Boolean).length;
      // Без модификатора хук глотал бы обычную клавишу во всех программах.
      if (c.keys.length > 0 && mods === 0) {
        setHint(t("hotkeys.needMod"));
        setCapturing(false);
        return;
      }
      // Один модификатор без клавиш: тап. Два тапа подряд — «Ctrl, Ctrl».
      if (c.keys.length === 0 && mods === 1) {
        const m = c.ctrl ? "ctrl" : c.alt ? "alt" : c.shift ? "shift" : "win";
        const now = performance.now();
        const last = tapRef.current;
        capRef.current = { ctrl: false, alt: false, shift: false, win: false, keys: [] };
        if (last && last.m === m && now - last.t <= 400) {
          setCapturing(false);
          onChange({ ...EMPTY_COMBO, [m]: true, double: true });
          return;
        }
        tapRef.current = { m, t: now };
        return;
      }
      // фиксируем на первом отпускании, если есть обычная клавиша или два модификатора
      if (c.keys.length === 0 && mods < 2) return;
      setCapturing(false);
      onChange({
        ctrl: c.ctrl,
        alt: c.alt,
        shift: c.shift,
        win: c.win,
        key: c.keys[0] ?? 0,
        extraKeys: c.keys.slice(1),
        double: false,
      });
    };
    window.addEventListener("keydown", onDown, true);
    window.addEventListener("keyup", onUp, true);
    return () => {
      window.removeEventListener("keydown", onDown, true);
      window.removeEventListener("keyup", onUp, true);
      endCapture(self);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [capturing]);

  const label = comboLabel(value);
  const empty = !value || label === "—";

  if (capturing) {
    return (
      <span className={`flex items-center gap-1.5 ${fill ? "w-full" : ""}`}>
        <button
          type="button"
          onClick={() => setCapturing(false)}
          className={`flex cursor-pointer items-center gap-2 rounded-md border border-dashed border-amber-deep bg-amber-deep/10 px-3 py-[5px] dark:border-amber dark:bg-amber/10 ${
            fill ? "h-[34px] min-w-0 flex-1" : ""
          }`}
        >
          <span className="h-1.5 w-1.5 flex-none animate-pulse2 rounded-full bg-amber-deep dark:bg-amber" />
          <span className="truncate font-mono text-[11.5px] font-medium text-amber-mid dark:text-amber">{t("common.waiting")}</span>
        </button>
        {/* Убрать назначенный хоткей: мышью, а не только Backspace, о котором никто не знает. */}
        {allowEmpty && !empty && (
          <button
            type="button"
            // mousedown, а не click: иначе потеря фокуса успела бы снять захват раньше
            onMouseDown={(e) => {
              e.preventDefault();
              setCapturing(false);
              onChange(null);
            }}
            className={`flex flex-none cursor-pointer items-center gap-1.5 rounded-md border border-black/[.13] bg-white px-2.5 py-[5px] text-[11.5px] text-danger-ink transition-colors hover:border-danger/60 dark:border-white/10 dark:bg-white/[.05] dark:text-danger ${
              fill ? "h-[34px]" : ""
            }`}
          >
            ✕ {t("hotkey.clear")}
          </button>
        )}
      </span>
    );
  }

  return (
    <span className={`flex flex-col gap-1 ${fill ? "w-full items-stretch" : "items-end"}`}>
      {empty ? (
        // Пустой хоткей — плашка с полосатой обводкой и приглашением: жать можно
        // в любое место, а не выцеливать крошечный «—».
        <button
          type="button"
          onClick={() => setCapturing(true)}
          className={`group flex cursor-pointer rounded-md bg-[repeating-linear-gradient(135deg,rgba(0,0,0,.22)_0_5px,transparent_5px_10px)] p-px hover:bg-[repeating-linear-gradient(135deg,rgb(var(--amber-deep))_0_5px,transparent_5px_10px)] dark:bg-[repeating-linear-gradient(135deg,rgba(255,255,255,.2)_0_5px,transparent_5px_10px)] dark:hover:bg-[repeating-linear-gradient(135deg,rgb(var(--amber))_0_5px,transparent_5px_10px)] ${
            fill ? "h-[34px] w-full" : ""
          }`}
        >
          <span className="flex flex-1 items-center justify-center whitespace-nowrap rounded-[5px] bg-white px-3 py-[5px] text-[11.5px] text-paper-dim transition-colors group-hover:text-amber-ink dark:bg-[#1e2024] dark:text-ink-text/50 dark:group-hover:text-amber">
            {t("hotkey.assign")}
          </span>
        </button>
      ) : (
        // Заданный хоткей: при наведении поверх клавиш — «Нажмите, чтобы изменить».
        <button
          type="button"
          onClick={() => setCapturing(true)}
          className={`group relative flex cursor-pointer items-center ${
            fill
              ? "h-[34px] w-full rounded-md border border-black/[.13] bg-white px-2 dark:border-white/10 dark:bg-white/[.05]"
              : ""
          }`}
        >
          <KeyCaps combo={label} />
          <span
            className={`pointer-events-none absolute flex items-center justify-center whitespace-nowrap rounded-md border border-amber-deep bg-[#faf8f5] px-2.5 text-[11.5px] font-medium text-amber-ink opacity-0 transition-opacity group-hover:opacity-100 dark:border-amber dark:bg-[#23262b] dark:text-amber ${
              fill ? "inset-0" : "-inset-y-[3px] right-[-3px] min-w-[calc(100%+6px)]"
            }`}
          >
            {t("hotkey.change")}
          </span>
        </button>
      )}
      {hint && <span className="text-[11px] text-amber-mid dark:text-amber">{hint}</span>}
    </span>
  );
}
