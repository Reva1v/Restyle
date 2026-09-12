import { createPortal } from "react-dom";
import { useEffect, useRef, useState } from "react";
import { commands } from "../lib/ipc";
import type { T } from "../lib/i18n";

/**
 * Примитивы из макета: шапка окна без системных декораций, тумблер, клавиша,
 * карточка-строка, сегментированный переключатель, поле с отложенной записью.
 * Светлая и тёмная темы живут в одних и тех же классах через `dark:`.
 */

/** Своя строка заголовка: перетаскивание + свернуть/закрыть (макет, все окна). */
export function TitleBar({ title, minimize = true, t }: { title: string; minimize?: boolean; t: T }) {
  return (
    <div
      data-tauri-drag-region
      className="flex h-[38px] flex-none items-center border-b border-black/[.07] bg-paper-bar pl-4 dark:border-white/[.06] dark:bg-ink-700"
    >
      <span data-tauri-drag-region className="text-xs font-medium text-[#33363c] dark:text-ink-text/85">
        {title}
      </span>
      <div className="ml-auto flex h-full">
        {minimize && (
          <button
            type="button"
            title={t("app.minimize")}
            onClick={() => void commands.winMinimize()}
            className="flex w-11 items-center justify-center font-mono text-xs text-[#585c63] transition-colors hover:bg-black/[.06] dark:text-ink-text/50 dark:hover:bg-white/[.08]"
          >
            –
          </button>
        )}
        <button
          type="button"
          title={t("app.close")}
          onClick={() => void commands.winClose()}
          className="flex w-11 items-center justify-center font-mono text-xs text-[#585c63] transition-colors hover:bg-[#e81123] hover:text-white dark:text-ink-text/50"
        >
          ✕
        </button>
      </div>
    </div>
  );
}

export function Toggle({
  checked,
  onChange,
  disabled = false,
  size = "md",
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  disabled?: boolean;
  size?: "md" | "sm";
}) {
  const w = size === "md" ? "w-10 h-[22px]" : "w-7 h-4";
  const k = size === "md" ? "w-4 h-4" : "w-3 h-3";
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={`${w} flex flex-none items-center rounded-full px-[3px] transition-colors disabled:opacity-40 ${
        checked ? "justify-end bg-amber-deep dark:bg-amber" : "justify-start bg-black/[.16] dark:bg-white/20"
      }`}
    >
      <span className={`${k} rounded-full bg-white transition-transform dark:bg-white`} />
    </button>
  );
}

/** Клавиша: моношрифт в рамке. `accent` — подсвеченная (Enter в готовом HUD). */
export function KeyCap({ children, accent = false }: { children: React.ReactNode; accent?: boolean }) {
  return (
    <span
      className={`rounded px-[6px] py-[3px] font-mono text-[10px] font-medium leading-none ${
        accent
          ? "bg-amber-deep text-white dark:bg-amber dark:text-amber-on"
          : "border border-black/10 bg-black/[.06] text-[#33363c] dark:border-transparent dark:bg-white/[.08] dark:text-ink-text/75"
      }`}
    >
      {children}
    </span>
  );
}

/** Крупная клавиша для списка хоткеев в настройках. */
export function KeyCaps({ combo }: { combo: string }) {
  if (combo === "—") return <span className="font-mono text-xs text-paper-dim dark:text-ink-text/45">—</span>;
  return (
    <span className="flex gap-1">
      {combo.split(" + ").map((part, i) => (
        <span
          key={`${part}-${i}`}
          className="rounded-[5px] border border-black/[.12] bg-paper-line px-2 py-1.5 font-mono text-[11.5px] font-medium leading-none text-[#33363c] dark:border-white/10 dark:bg-white/[.07] dark:text-ink-text/85"
        >
          {part}
        </span>
      ))}
    </span>
  );
}

/** Белая карточка-строка настройки: заголовок, пояснение, контрол справа. */
export function Row({
  title,
  hint,
  warn,
  children,
  accent = false,
}: {
  title: React.ReactNode;
  hint?: React.ReactNode;
  warn?: string | null;
  children?: React.ReactNode;
  accent?: boolean;
}) {
  return (
    <div
      className={`flex items-center gap-3.5 rounded-lg border bg-white p-3.5 dark:bg-white/[.04] ${
        accent
          ? "border-amber-deep shadow-[0_0_0_2px_rgb(var(--amber-deep)/.13)] dark:border-amber"
          : warn
            ? "border-danger/55"
            : "border-black/10 dark:border-white/[.08]"
      }`}
    >
      <div className="flex min-w-0 flex-1 flex-col gap-[3px]">
        <span className="text-[13px] font-medium leading-tight text-paper-text dark:text-ink-text">{title}</span>
        {hint && <span className="text-[11.5px] leading-snug text-paper-dim dark:text-ink-text/55">{hint}</span>}
        {warn && <span className="text-[11.5px] leading-snug text-danger-ink dark:text-danger">{warn}</span>}
      </div>
      {children}
    </div>
  );
}

/**
 * Строка настройки, которая целиком переключает тумблер: попадать мышью в
 * сам тумблер (28×17 px) неудобно, а строка — цель во всю ширину окна.
 */
export function ToggleRow({
  title,
  hint,
  checked,
  onChange,
  children,
  disabled = false,
}: {
  title: React.ReactNode;
  hint?: React.ReactNode;
  checked: boolean;
  onChange: (v: boolean) => void;
  /** Дополнительный контент слева от тумблера (например «150 мс»). */
  children?: React.ReactNode;
  disabled?: boolean;
}) {
  return (
    <div
      role="switch"
      aria-checked={checked}
      aria-disabled={disabled}
      tabIndex={disabled ? -1 : 0}
      onClick={() => !disabled && onChange(!checked)}
      onKeyDown={(e) => {
        if (disabled) return;
        if (e.key === " " || e.key === "Enter") {
          e.preventDefault();
          onChange(!checked);
        }
      }}
      className={`flex items-center gap-3.5 rounded-lg border border-black/10 bg-white p-3.5 text-left transition-colors dark:border-white/[.08] dark:bg-white/[.04] ${
        disabled
          ? "opacity-50"
          : "cursor-pointer hover:border-black/20 dark:hover:border-white/20"
      }`}
    >
      <div className="flex min-w-0 flex-1 flex-col gap-[3px]">
        <span className="text-[13px] font-medium leading-tight text-paper-text dark:text-ink-text">{title}</span>
        {hint && <span className="text-[11.5px] leading-snug text-paper-dim dark:text-ink-text/55">{hint}</span>}
      </div>
      {children}
      <Toggle checked={checked} onChange={onChange} disabled={disabled} />
    </div>
  );
}

/**
 * Выпадающий список в стиле макета. Свой, а не нативный `<select>`: системный
 * список WebView2 не красится под тему и выглядит чужеродно. Список рисуется
 * порталом в `body` с `position: fixed` — иначе его обрезал бы скроллер раздела
 * настроек; у нижнего края окна раскрывается вверх. Клавиатура: ↑↓, Enter, Esc.
 */
export function Select<V extends string>({
  value,
  options,
  onChange,
  className = "",
  disabled = false,
}: {
  value: V;
  options: { value: V; label: string; hint?: string }[];
  onChange: (v: V) => void;
  className?: string;
  disabled?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const [hi, setHi] = useState(0);
  const [pos, setPos] = useState<{ left: number; width: number; top?: number; bottom?: number; maxH: number } | null>(
    null,
  );
  const btnRef = useRef<HTMLButtonElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const hiRef = useRef(0);
  hiRef.current = hi;
  const current = options.find((o) => o.value === value);

  const pick = (v: V) => {
    setOpen(false);
    if (v !== value) onChange(v);
  };

  useEffect(() => {
    if (!open || !btnRef.current) return;
    const r = btnRef.current.getBoundingClientRect();
    const want = Math.min(options.length * 32 + 10, 300);
    const below = window.innerHeight - r.bottom - 10;
    const above = r.top - 10;
    const up = below < want && above > below;
    // Список шире узкой кнопки: у правого края окна выравниваем его по правому
    // краю кнопки и в любом случае не выпускаем за границы окна.
    const width = Math.min(Math.max(r.width, 180), window.innerWidth - 16);
    const left = Math.max(8, Math.min(r.left + width > window.innerWidth - 8 ? r.right - width : r.left, window.innerWidth - width - 8));
    setPos({
      left,
      width,
      ...(up ? { bottom: window.innerHeight - r.top + 4 } : { top: r.bottom + 4 }),
      maxH: Math.max(120, Math.min(300, up ? above : below)),
    });
    setHi(Math.max(0, options.findIndex((o) => o.value === value)));

    const inside = (n: EventTarget | null) =>
      !!n && !!(btnRef.current?.contains(n as Node) || listRef.current?.contains(n as Node));
    const onDown = (e: MouseEvent) => {
      if (!inside(e.target)) setOpen(false);
    };
    const onScroll = (e: Event) => {
      if (!inside(e.target)) setOpen(false);
    };
    const onResize = () => setOpen(false);
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        setOpen(false);
      } else if (e.key === "ArrowDown") {
        e.preventDefault();
        setHi((h) => Math.min(options.length - 1, h + 1));
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        setHi((h) => Math.max(0, h - 1));
      } else if (e.key === "Enter") {
        e.preventDefault();
        const o = options[hiRef.current];
        if (o) pick(o.value);
      }
    };
    window.addEventListener("mousedown", onDown, true);
    window.addEventListener("scroll", onScroll, true);
    window.addEventListener("resize", onResize);
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("mousedown", onDown, true);
      window.removeEventListener("scroll", onScroll, true);
      window.removeEventListener("resize", onResize);
      window.removeEventListener("keydown", onKey, true);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  // Подсвеченный пункт не должен уезжать из видимой части списка.
  useEffect(() => {
    listRef.current?.querySelector<HTMLElement>(`[data-i="${hi}"]`)?.scrollIntoView({ block: "nearest" });
  }, [hi, pos]);

  return (
    <span className={`relative inline-flex items-center ${className}`}>
      <button
        ref={btnRef}
        type="button"
        disabled={disabled}
        onClick={() => setOpen((o) => !o)}
        className={`flex h-full min-h-[32px] w-full cursor-pointer items-center gap-2 rounded-lg border bg-white py-0 pl-2.5 pr-2 text-left text-[12.5px] text-paper-text outline-none transition-colors disabled:cursor-default disabled:opacity-50 dark:bg-ink-700 dark:text-ink-text ${
          open
            ? "border-amber-deep dark:border-amber"
            : "border-black/[.14] hover:border-black/25 dark:border-white/10 dark:hover:border-white/20"
        }`}
      >
        <span className="min-w-0 flex-1 truncate">{current?.label ?? "—"}</span>
        <span className={`flex-none text-[9px] text-paper-dim transition-transform dark:text-ink-text/50 ${open ? "rotate-180" : ""}`}>
          ▼
        </span>
      </button>
      {open &&
        pos &&
        createPortal(
          <div
            ref={listRef}
            style={{ left: pos.left, width: pos.width, top: pos.top, bottom: pos.bottom, maxHeight: pos.maxH }}
            className="fixed z-[1000] overflow-y-auto rounded-lg border border-black/[.12] bg-white p-1 shadow-hudLight dark:border-white/10 dark:bg-[#1f2126] dark:shadow-hud"
          >
            {options.map((o, i) => {
              const selected = o.value === value;
              return (
                <button
                  key={o.value}
                  type="button"
                  data-i={i}
                  onMouseEnter={() => setHi(i)}
                  onClick={() => pick(o.value)}
                  className={`flex w-full cursor-pointer items-center gap-3 rounded-md px-2.5 py-[7px] text-left text-[12.5px] ${
                    i === hi ? "bg-black/[.05] dark:bg-white/[.07]" : ""
                  } ${selected ? "text-amber-ink dark:text-amber" : "text-paper-text dark:text-ink-text/90"}`}
                >
                  <span className="min-w-0 flex-1 truncate">{o.label}</span>
                  {o.hint && (
                    <span className="flex-none font-mono text-[10.5px] text-paper-dim dark:text-ink-text/45">{o.hint}</span>
                  )}
                  {selected && <span className="flex-none text-[11px]">✓</span>}
                </button>
              );
            })}
          </div>,
          document.body,
        )}
    </span>
  );
}

/** Модалка поверх окна: затемнение + карточка. Esc и клик по фону закрывают. */
export function Modal({
  title,
  onClose,
  children,
}: {
  title: string;
  onClose: () => void;
  children: React.ReactNode;
}) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/35 p-6"
      onClick={onClose}
    >
      <div
        onClick={(e) => e.stopPropagation()}
        className="flex max-h-full w-[420px] flex-col overflow-hidden rounded-xl border border-black/10 bg-paper shadow-hudLight dark:border-white/10 dark:bg-ink-800 dark:shadow-hud"
      >
        <div className="flex flex-none items-center border-b border-black/[.07] px-4 py-3 dark:border-white/[.06]">
          <span className="text-[13px] font-semibold text-paper-text dark:text-ink-text">{title}</span>
          <button
            type="button"
            onClick={onClose}
            className="ml-auto font-mono text-xs text-paper-dim hover:text-paper-text dark:text-ink-text/50 dark:hover:text-ink-text"
          >
            ✕
          </button>
        </div>
        {children}
      </div>
    </div>
  );
}

export function SectionTitle({ title, hint }: { title: string; hint?: string }) {
  return (
    <div className="flex flex-col gap-[3px]">
      <span className="text-sm font-semibold leading-tight text-paper-text dark:text-ink-text">{title}</span>
      {hint && <span className="text-xs leading-snug text-paper-dim dark:text-ink-text/55">{hint}</span>}
    </div>
  );
}

/** Мелкая моно-подпись группы («ЧТО СНИМАЕМ»). */
export function Caption({ children }: { children: React.ReactNode }) {
  return (
    <span className="font-mono text-[11px] font-medium uppercase leading-none tracking-[.06em] text-paper-dim dark:text-ink-text/55">
      {children}
    </span>
  );
}

export function Segmented<V extends string>({
  value,
  options,
  onChange,
}: {
  value: V;
  options: { value: V; label: string }[];
  onChange: (v: V) => void;
}) {
  return (
    <span className="flex gap-1">
      {options.map((o) => (
        <button
          key={o.value}
          type="button"
          onClick={() => onChange(o.value)}
          className={`rounded-md px-3 py-1.5 text-xs transition-colors ${
            o.value === value
              ? "border border-black/[.14] bg-white font-medium text-paper-text dark:border-white/15 dark:bg-white/10 dark:text-ink-text"
              : "text-paper-dim hover:text-paper-text dark:text-ink-text/55 dark:hover:text-ink-text"
          }`}
        >
          {o.label}
        </button>
      ))}
    </span>
  );
}

export function Button({
  children,
  onClick,
  kind = "ghost",
  disabled = false,
  title,
}: {
  children: React.ReactNode;
  onClick?: () => void;
  kind?: "primary" | "ghost" | "danger";
  disabled?: boolean;
  title?: string;
}) {
  const cls =
    kind === "primary"
      ? "bg-paper-text text-paper dark:bg-amber dark:text-amber-on"
      : kind === "danger"
        ? "text-danger-ink dark:text-danger hover:bg-danger/10"
        : "border border-black/[.14] bg-white text-[#33363c] hover:bg-black/[.03] dark:border-white/10 dark:bg-white/[.06] dark:text-ink-text dark:hover:bg-white/10";
  return (
    <button
      type="button"
      title={title}
      disabled={disabled}
      onClick={onClick}
      className={`flex h-[34px] items-center rounded-md px-[15px] text-[12.5px] font-medium leading-none transition-colors disabled:opacity-40 ${cls}`}
    >
      {children}
    </button>
  );
}

/**
 * Поле ввода, отдающее значение по потере фокуса (или Enter). Сохранение на
 * каждый символ пересобирало бы ComboSet в hotkey-потоке и писало store.
 */
export function DraftField({
  value,
  onCommit,
  multiline = false,
  placeholder,
  className = "",
  mono = false,
  password = false,
  autoFocus = false,
  onKeyDown,
}: {
  value: string;
  onCommit: (v: string) => void;
  multiline?: boolean;
  placeholder?: string;
  className?: string;
  mono?: boolean;
  password?: boolean;
  autoFocus?: boolean;
  onKeyDown?: (e: React.KeyboardEvent) => void;
}) {
  const [draft, setDraft] = useState(value);
  const focused = useRef(false);
  useEffect(() => {
    if (!focused.current) setDraft(value);
  }, [value]);

  const base = `rounded-md border border-black/[.13] bg-white px-3 py-2 text-[13px] text-paper-text outline-none transition-colors placeholder:text-paper-dim/70 focus:border-amber-deep focus:shadow-[0_0_0_2px_rgb(var(--amber-deep)/.13)] dark:border-white/10 dark:bg-white/[.05] dark:text-ink-text dark:placeholder:text-ink-text/35 dark:focus:border-amber ${
    mono ? "font-mono" : ""
  } ${className}`;
  const common = {
    value: draft,
    placeholder,
    autoFocus,
    onFocus: () => {
      focused.current = true;
    },
    onBlur: () => {
      focused.current = false;
      if (draft !== value) onCommit(draft);
    },
    className: base,
  };
  if (multiline) {
    return (
      <textarea
        {...common}
        onChange={(e) => setDraft(e.target.value)}
        onKeyDown={(e) => {
          onKeyDown?.(e);
          if (e.key === "Escape") {
            setDraft(value);
            e.currentTarget.blur();
          }
        }}
      />
    );
  }
  return (
    <input
      {...common}
      type={password ? "password" : "text"}
      autoComplete="off"
      onChange={(e) => setDraft(e.target.value)}
      onKeyDown={(e) => {
        onKeyDown?.(e);
        if (e.key === "Enter") e.currentTarget.blur();
        if (e.key === "Escape") {
          setDraft(value);
          e.currentTarget.blur();
        }
      }}
    />
  );
}
