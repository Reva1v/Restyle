import { useEffect, useRef, useState } from "react";
import type { HotkeyCombo } from "../bindings/HotkeyCombo";
import { commands } from "../lib/ipc";
import { codeToVk, comboLabel } from "../lib/hotkey";

/**
 * Поле хоткея с захватом реального нажатия (схема из HoldMix):
 * клик — режим захвата, зажатые клавиши накапливаются, отпускание первой
 * фиксирует комбинацию. Esc — отмена, Backspace/Delete — очистить.
 * На время захвата LL-хук приостанавливается (иначе текущая комбинация
 * откроет оверлей), поэтому снятие режима обязано пережить и размонтирование
 * (строку стиля можно удалить прямо во время захвата) — иначе хук остался бы
 * выключенным до перезапуска.
 *
 * Захват в один момент времени ведёт только одно поле: их на экране много
 * (у каждого стиля своё), а слушатели у всех глобальные.
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
}: {
  value: HotkeyCombo | null;
  onChange: (c: HotkeyCombo | null) => void;
  allowEmpty?: boolean;
}) {
  const [capturing, setCapturing] = useState(false);
  const [hint, setHint] = useState("");
  const capRef = useRef({ ctrl: false, alt: false, shift: false, win: false, keys: [] as number[] });
  // стабильная ссылка — ею поле опознаёт себя в общем «кто сейчас захватывает»
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

    const onDown = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === "Escape") {
        setCapturing(false);
        return;
      }
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
        setHint("Нужен модификатор: Ctrl / Alt / Shift / Win");
        setCapturing(false);
        return;
      }
      // фиксируем на первом отпускании, если есть хоть одна обычная клавиша
      // или два модификатора (чисто модификаторные комбо)
      if (c.keys.length === 0 && mods < 2) return;
      setCapturing(false);
      onChange({
        ctrl: c.ctrl, alt: c.alt, shift: c.shift, win: c.win,
        key: c.keys[0] ?? 0,
        extraKeys: c.keys.slice(1),
      });
    };
    window.addEventListener("keydown", onDown, true);
    window.addEventListener("keyup", onUp, true);
    return () => {
      window.removeEventListener("keydown", onDown, true);
      window.removeEventListener("keyup", onUp, true);
      endCapture(self); // в т.ч. при размонтировании прямо во время захвата
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [capturing]);

  return (
    <span className="inline-flex flex-col items-end gap-0.5">
      <button
        type="button"
        onClick={() => setCapturing((v) => !v)}
        className={`min-w-40 rounded border px-3 py-1.5 text-left font-mono text-sm ${
          capturing
            ? "border-blue-500 bg-blue-950 text-blue-200"
            : "border-neutral-600 bg-neutral-800 text-neutral-100"
        }`}
      >
        {capturing ? "Нажмите комбинацию…" : comboLabel(value)}
      </button>
      {hint && <span className="text-xs text-amber-400">{hint}</span>}
    </span>
  );
}
