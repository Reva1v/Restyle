import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import {
  commands,
  events,
  VK,
  type ScreenshotPayload,
  type Settings,
  type ShowPayload,
  type TextPayload,
} from "./lib/ipc";
import { useAppShell } from "./lib/theme";
import { comboLabel } from "./lib/hotkey";
import { FMT_KEYS, parseFmt, type Fmt, type T } from "./lib/i18n";

type Gen = {
  state: "idle" | "streaming" | "done" | "error";
  text: string;
  message?: string;
  elapsed?: number;
  pendingPaste?: boolean;
};

/** До скольких чипов работают цифры 1–9; остальные — мышью и Tab. */
const CHIPS_NUMBERED = 9;


/**
 * Оверлей — HUD из макета (разделы 01/02/07): шапка «RESTYLE · исходник ·
 * метка скриншота», ряд чипов, блок результата, подсказки клавиш. Ширина 520,
 * высота по содержимому: после каждой перерисовки замер уходит в бэкенд
 * (`resize_overlay`) — иначе акрил окна торчал бы пустым прямоугольником.
 *
 * Базовые классы — светлая тема, `dark:` — тёмная (тема ставится атрибутом
 * `data-theme` на <html>). Окно без фокуса (WS_EX_NOACTIVATE), поэтому
 * клавиатура приходит событием `overlay:key` из LL-хука, а не через DOM.
 */
export default function App() {
  const { t } = useAppShell();
  const [visible, setVisible] = useState(false);
  const [payload, setPayload] = useState<ShowPayload | null>(null);
  const [text, setText] = useState<TextPayload | null>(null);
  const [shot, setShot] = useState<ScreenshotPayload | null>(null);
  const [styles, setStyles] = useState<Settings["styles"]>([]);
  const [selected, setSelected] = useState<number | null>(null);
  const [gen, setGen] = useState<Gen>({ state: "idle", text: "" });
  /** Длительность заполнения кольца удержания; null — кольца нет. */
  const [ring, setRing] = useState<number | null>(null);
  /** Выбранный регистр (вместо стиля); открыт ли список регистров. */
  const [fmt, setFmt] = useState<Fmt | null>(null);
  const [fmtOpen, setFmtOpen] = useState(false);
  const fmtBtnRef = useRef<HTMLButtonElement>(null);
  const genRef = useRef(0);
  /** Номер запроса внутри сессии: чанки и done отменённой генерации игнорируются. */
  const reqRef = useRef(0);
  const stylesRef = useRef(styles);
  stylesRef.current = styles;
  const rootRef = useRef<HTMLDivElement>(null);

  function choose(i: number) {
    const s = stylesRef.current[i];
    if (!s) return;
    setSelected(i);
    void commands.selectStyle(s.id);
  }

  // Список регистров — отдельное окно (окно меню трея): панель не растёт и не
  // двигается, а бэкенд ставит список под кнопкой или над ней по месту на экране.
  function toggleFmt() {
    const b = fmtBtnRef.current;
    if (!b) return;
    const r = b.getBoundingClientRect();
    void commands.openFormatMenu(r.left, r.top, r.bottom, fmt ?? "");
  }

  useEffect(() => {
    void commands.getSettings().then((s) => setStyles(s.styles));
    const uns = [
      events.onSettingsChanged((s) => setStyles(s.styles)),
      events.onRing((p) => {
        genRef.current = p.gen;
        setRing(p.fillMs);
        setVisible(true);
      }),
      events.onShow((p) => {
        genRef.current = p.gen;
        setRing(null);
        setFmt(null);
        setPayload(p);
        setText(null);
        setShot(null);
        setGen({ state: "idle", text: "" });
        setSelected(p.styleId ? Math.max(0, stylesRef.current.findIndex((s) => s.id === p.styleId)) : null);
        setVisible(true);
        requestAnimationFrame(() => requestAnimationFrame(() => void commands.panelShown()));
      }),
      events.onText((x) => setText(x)),
      events.onScreenshot((s) => setShot(s)),
      events.onHide(() => {
        setVisible(false);
        setRing(null);
      }),
      events.onRewriteStart((p) => {
        if (p.gen !== genRef.current) return;
        reqRef.current = p.req;
        // Генерацию мог запустить и бэкенд (быстрый стиль, повтор) — подсветку
        // берём из события, а не только из собственного клика.
        const f = parseFmt(p.styleId);
        setFmt(f);
        if (f) setSelected(null);
        const i = stylesRef.current.findIndex((s) => s.id === p.styleId);
        if (i >= 0) setSelected(i);
        setGen({ state: "streaming", text: "" });
      }),
      events.onRewriteChunk((p) => {
        if (p.gen !== genRef.current || p.req !== reqRef.current) return;
        // `...g`: флаг pendingPaste («вставлю по готовности») чанк не сбрасывает.
        setGen((g) => ({ ...g, state: "streaming", text: g.text + p.text }));
      }),
      events.onRewriteDone((p) => {
        if (p.gen !== genRef.current || p.req !== reqRef.current) return;
        setGen({ state: "done", text: p.text, elapsed: p.elapsedMs });
      }),
      events.onPendingPaste((g) => {
        if (g !== genRef.current) return;
        setGen((cur) => ({ ...cur, pendingPaste: true }));
      }),
      events.onRewriteError((p) => {
        if (p.gen !== genRef.current || p.req !== reqRef.current) return;
        setGen((g) => ({ state: "error", text: g.text, message: p.message }));
      }),
      events.onMenuFormat(() => setFmtOpen(true)),
      events.onMenuHide(() => setFmtOpen(false)),
      events.onKey((vk) => {
        const n = stylesRef.current.length;
        if (n === 0) return;
        if (vk === VK.R) return void commands.regenerate();
        if (vk === VK.ENTER) return void commands.pasteResult();
        setSelected((cur) => {
          let next = cur;
          if (vk === VK.DOWN || vk === VK.RIGHT || vk === VK.TAB) next = cur === null ? 0 : (cur + 1) % n;
          else if (vk === VK.UP || vk === VK.LEFT) next = cur === null ? n - 1 : (cur - 1 + n) % n;
          else if (vk >= 0x31 && vk <= 0x39) next = Math.min(vk - 0x31, n - 1);
          if (next !== null && next !== cur) queueMicrotask(() => choose(next));
          return next;
        });
      }),
    ];
    void Promise.all(uns).then(() => commands.frontendReady());
    return () => {
      for (const u of uns) void u.then((f) => f());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Высота содержимого → размер окна: чипы, блок результата и стриминг меняют
  // её на лету, поэтому слушаем ResizeObserver, а не меряем один раз.
  useLayoutEffect(() => {
    if (!visible || !rootRef.current) return;
    const el = rootRef.current;
    const report = () => void commands.resizeOverlay(el.getBoundingClientRect().height);
    report();
    const ro = new ResizeObserver(report);
    ro.observe(el);
    return () => ro.disconnect();
    // `ring` тоже в зависимостях: кольцо → панель проходит без overlay:hide,
    // и без перезапуска наблюдатель остался бы на отсоединённом div кольца —
    // панель не сообщала бы высоту и оставалась 148 px с обрезанным результатом.
  }, [visible, ring]);

  if (!visible) return null;

  // Кольцо: окно уже показано, панели ещё нет. Заполняется ровно за то время,
  // которое осталось до открытия панели.
  if (ring !== null) {
    // Окно оверлея без акрила и прозрачное: всё вне кружка просто не рисуется.
    return (
      <div ref={rootRef} className="h-[46px] w-[46px]">
        <motion.div
          initial={{ scale: 0.8, opacity: 0 }}
          animate={{ scale: 1, opacity: 1 }}
          transition={{ duration: 0.12 }}
          className="relative flex h-full w-full items-center justify-center rounded-full border border-black/[.13] bg-[#faf8f5] dark:border-white/[.11] dark:bg-[#181a1e]"
        >
          <svg viewBox="0 0 36 36" className="absolute left-1/2 top-1/2 h-[36px] w-[36px] -translate-x-1/2 -translate-y-1/2 -rotate-90">
            <circle cx="18" cy="18" r="15" fill="none" strokeWidth="2.5" className="stroke-black/[.10] dark:stroke-white/[.12]" />
            <motion.circle
              cx="18"
              cy="18"
              r="15"
              fill="none"
              strokeWidth="2.5"
              strokeLinecap="round"
              className="stroke-amber-deep dark:stroke-amber"
              strokeDasharray={2 * Math.PI * 15}
              initial={{ strokeDashoffset: 2 * Math.PI * 15 }}
              animate={{ strokeDashoffset: 0 }}
              transition={{ duration: ring / 1000, ease: "linear" }}
            />
          </svg>
          <span className="font-mono text-[12px] font-semibold text-amber-ink dark:text-amber">R</span>
        </motion.div>
      </div>
    );
  }

  const quick = !!payload?.styleId && gen.state !== "idle" && selected !== null;
  const style = selected !== null ? styles[selected] : null;
  // Показываем все стили: панель растёт по содержимому, и прятать их за
  // «Ещё N» незачем — места снизу всё равно достаточно.
  const chipIdx = styles.map((_, i) => i);

  return (
    <div ref={rootRef} className={payload?.toast ? "inline-block" : undefined}>
      {payload?.toast ? (
        <Toast text={payload.toast} />
      ) : (
        <motion.div
          initial={{ opacity: 0, y: -6, scale: 0.985 }}
          animate={{ opacity: 1, y: 0, scale: 1 }}
          transition={{ duration: 0.14, ease: [0.22, 1, 0.36, 1] }}
          className="overflow-hidden rounded-hud border border-black/[.13] bg-[#faf8f5] text-paper-text dark:border-white/[.11] dark:bg-[#181a1e] dark:text-ink-text"
        >
          {quick ? (
            <QuickHeader t={t} styleName={style?.name ?? ""} hotkey={comboLabel(style?.hotkey)} gen={gen} />
          ) : (
            <Header t={t} payload={payload} text={text} shot={shot} />
          )}

          {!quick && (
            <div className="flex flex-wrap gap-[7px] p-3.5">
              {chipIdx.map((i) => (
                <Chip
                  key={styles[i].id}
                  index={i < CHIPS_NUMBERED ? i + 1 : null}
                  name={styles[i].name}
                  active={i === selected}
                  onClick={() => choose(i)}
                />
              ))}
              <FormatButton
                btnRef={fmtBtnRef}
                label={fmt ? t(FMT_KEYS[fmt]) : t("fmt.button")}
                active={fmt !== null}
                open={fmtOpen}
                disabled={!text}
                onClick={toggleFmt}
              />
            </div>
          )}

          <AnimatePresence initial={false}>
            {gen.state !== "idle" && (
              <motion.div
                key="result"
                initial={{ opacity: 0, height: 0 }}
                animate={{ opacity: 1, height: "auto" }}
                exit={{ opacity: 0, height: 0 }}
                transition={{ duration: 0.16, ease: "easeOut" }}
                className="overflow-hidden"
              >
                <Result t={t} gen={gen} styleName={fmt ? t(FMT_KEYS[fmt]) : (style?.name ?? "")} quick={quick} />
              </motion.div>
            )}
          </AnimatePresence>

          {!quick && <Footer t={t} gen={gen} />}
        </motion.div>
      )}
    </div>
  );
}

function Header({
  t,
  payload,
  text,
  shot,
}: {
  t: T;
  payload: ShowPayload | null;
  text: TextPayload | null;
  shot: ScreenshotPayload | null;
}) {
  const withShot = payload?.screenshot === "ready" || payload?.screenshot === "pending" || !!shot;
  const selectionOnly = text?.selectionOnly ?? false;
  const badge = selectionOnly ? t("hud.selectionOnly") : withShot ? t("hud.withShot") : t("hud.noShot");
  const source = text ? text.text : payload?.capturing ? t("hud.reading") : t("hud.noText");
  const accent = withShot && !selectionOnly;
  return (
    <div className="flex items-center gap-2.5 border-b border-black/[.08] px-3.5 py-[11px] dark:border-white/[.07]">
      <span className="flex-none font-mono text-[9.5px] font-semibold leading-none tracking-[.1em] text-amber-mid dark:text-amber">
        RESTYLE
      </span>
      <span className="min-w-0 flex-1 truncate text-[12.5px] leading-tight text-[#585c63] dark:text-ink-text/60" title={text?.text}>
        {source}
      </span>
      <span
        className={`flex flex-none items-center gap-[5px] rounded-[5px] px-[7px] py-[3px] font-mono text-[10px] font-medium leading-none ${
          accent
            ? "bg-amber-deep/[.13] text-amber-ink dark:bg-amber/[.14] dark:text-amber"
            : "bg-black/[.06] text-paper-dim dark:bg-white/[.06] dark:text-ink-text/50"
        }`}
      >
        {accent && <span className="h-[5px] w-[5px] rounded-full bg-current" />}
        {badge}
      </span>
    </div>
  );
}

/** Шапка быстрого стиля: чип с хоткеем, подпись, время (макет, состояние C). */
function QuickHeader({ t, styleName, hotkey, gen }: { t: T; styleName: string; hotkey: string; gen: Gen }) {
  return (
    <>
      <div className="flex items-center gap-2.5 px-3.5 py-[11px]">
        <span className="flex-none font-mono text-[9.5px] font-semibold leading-none tracking-[.1em] text-amber-mid dark:text-amber">
          RESTYLE
        </span>
        <span className="flex flex-none items-center gap-[7px] rounded-md border border-amber-deep bg-amber-deep/[.13] px-2 py-1 text-xs font-medium text-amber-ink dark:border-amber/45 dark:bg-amber/[.16] dark:text-amber-soft">
          {hotkey !== "—" && <span className="font-mono text-[9.5px] opacity-80">{hotkey}</span>}
          {styleName}
        </span>
        <span className="min-w-0 flex-1 truncate text-xs leading-tight text-paper-dim dark:text-ink-text/55">
          {t("hud.noConfirm")}
        </span>
        {gen.elapsed !== undefined && (
          <span className="flex-none font-mono text-[10.5px] leading-none text-paper-dim dark:text-ink-text/55">
            {t("hud.secs", { n: (gen.elapsed / 1000).toFixed(1) })}
          </span>
        )}
      </div>
      {gen.state === "streaming" && (
        <div className="h-[2px] bg-black/[.06] dark:bg-white/[.06]">
          <motion.div
            className="h-[2px] bg-amber-deep dark:bg-amber"
            initial={{ width: "8%" }}
            animate={{ width: "92%" }}
            transition={{ duration: 6, ease: "easeOut" }}
          />
        </div>
      )}
    </>
  );
}

function Chip({
  index,
  name,
  active,
  onClick,
}: {
  /** Цифра-ускоритель; у стилей после девятого её нет. */
  index: number | null;
  name: string;
  active: boolean;
  onClick: () => void;
}) {
  return (
    <motion.button
      type="button"
      onClick={onClick}
      whileTap={{ scale: 0.97 }}
      className={`flex items-center gap-[7px] rounded-[7px] border px-2.5 py-[7px] text-[12.5px] font-medium leading-none transition-colors ${
        active
          ? "border-amber-deep bg-amber-deep/[.13] text-amber-ink dark:border-amber/45 dark:bg-amber/[.16] dark:text-amber-soft"
          : "border-black/[.12] bg-white text-[#33363c] hover:bg-black/[.03] dark:border-white/[.08] dark:bg-white/5 dark:text-ink-text/80 dark:hover:bg-white/[.09]"
      }`}
    >
      {index !== null && (
        <span className={`font-mono text-[10px] ${active ? "opacity-80" : "text-paper-soft dark:text-ink-text/55"}`}>
          {index}
        </span>
      )}
      {name}
    </motion.button>
  );
}

/** Кнопка «Aa Регистр ▾» в ряду чипов: открывает список регистров. */
function FormatButton({
  btnRef,
  label,
  active,
  open,
  disabled,
  onClick,
}: {
  btnRef: React.RefObject<HTMLButtonElement>;
  label: string;
  active: boolean;
  open: boolean;
  disabled: boolean;
  onClick: () => void;
}) {
  return (
    <button
      ref={btnRef}
      type="button"
      disabled={disabled}
      onClick={onClick}
      className={`flex cursor-pointer items-center gap-[7px] rounded-[7px] border px-2.5 py-[7px] text-[12.5px] font-medium leading-none transition-colors disabled:cursor-default disabled:opacity-50 ${
        active || open
          ? "border-amber-deep bg-amber-deep/[.13] text-amber-ink dark:border-amber/45 dark:bg-amber/[.16] dark:text-amber-soft"
          : "border-dashed border-black/[.18] bg-white text-[#33363c] hover:bg-black/[.03] dark:border-white/[.16] dark:bg-white/5 dark:text-ink-text/80 dark:hover:bg-white/[.09]"
      }`}
    >
      <span className={`font-mono text-[10px] ${active || open ? "opacity-80" : "text-paper-soft dark:text-ink-text/55"}`}>Aa</span>
      {label}
      <span className={`text-[9px] transition-transform ${open ? "rotate-180" : ""}`}>▾</span>
    </button>
  );
}

function Result({ t, gen, styleName, quick }: { t: T; gen: Gen; styleName: string; quick: boolean }) {
  const done = gen.state === "done";
  const err = gen.state === "error";
  if (quick) {
    return (
      <div className="px-3.5 pb-[13px] pt-3">
        <p className="m-0 whitespace-pre-wrap break-words text-[13.5px] leading-[1.6] text-paper-text dark:text-ink-text">
          {gen.text}
          {gen.state === "streaming" && <Caret />}
        </p>
        {err && <p className="mt-2 text-xs text-danger-ink dark:text-danger">{gen.message}</p>}
      </div>
    );
  }
  return (
    <div
      className={`mx-3.5 mb-3 flex flex-col gap-[9px] rounded-[9px] border p-[13px] ${
        err
          ? "border-danger/40 bg-danger/[.08]"
          : done
            ? "border-amber-deep/[.28] bg-amber-deep/[.08] dark:border-amber/25 dark:bg-amber/[.07]"
            : "border-black/[.08] bg-black/[.03] dark:border-white/[.06] dark:bg-white/[.04]"
      }`}
    >
      <div className="flex items-center gap-2">
        {gen.state === "streaming" && <span className="h-1.5 w-1.5 animate-pulse2 rounded-full bg-amber-deep dark:bg-amber" />}
        <span className="font-mono text-[10px] font-medium uppercase leading-none tracking-[.08em] text-amber-mid dark:text-amber/85">
          {styleName}
          {done ? ` · ${t("hud.done")}` : ""}
        </span>
        {done && (
          <span className="ml-auto font-mono text-[10px] leading-none text-paper-dim dark:text-ink-text/55">
            {t("hud.chars", { n: gen.text.length })} · {t("hud.secs", { n: ((gen.elapsed ?? 0) / 1000).toFixed(1) })}
          </span>
        )}
      </div>
      <p className="m-0 max-h-[260px] overflow-auto whitespace-pre-wrap break-words text-[13.5px] leading-[1.6] text-paper-text dark:text-ink-text">
        {gen.text || (gen.state === "idle" ? t("hud.pickStyle") : "")}
        {gen.state === "streaming" && <Caret />}
      </p>
      {err && <p className="m-0 text-xs text-danger-ink dark:text-danger">{gen.message}</p>}
    </div>
  );
}

function Caret() {
  return <span className="ml-[3px] inline-block h-[15px] w-[7px] animate-caret bg-amber-deep/75 align-[-3px] dark:bg-amber/75" />;
}

function Footer({ t, gen }: { t: T; gen: Gen }) {
  const streaming = gen.state === "streaming";
  const done = gen.state === "done";
  return (
    <div className="flex items-center gap-3.5 border-t border-black/[.08] bg-black/[.02] px-3.5 py-[9px] dark:border-white/[.07] dark:bg-white/[.02]">
      {streaming ? (
        <Hint keyCap="Esc" label={gen.pendingPaste ? t("hud.pendingPaste") : t("hud.cancelRequest")} />
      ) : (
        <>
          <Hint keyCap="Enter" label={t("hud.insert")} accent={done} />
          {done ? <Hint keyCap="R" label={t("hud.again")} /> : <Hint keyCap="1–9" label={t("hud.style")} />}
          <Hint keyCap="Tab" label={done ? t("hud.otherStyle") : t("hud.next")} />
          <Hint keyCap="Esc" label={t("hud.cancel")} />
        </>
      )}
    </div>
  );
}

function Hint({ keyCap, label, accent = false }: { keyCap: string; label: string; accent?: boolean }) {
  return (
    <span
      className={`flex items-center gap-1.5 text-[11px] leading-none ${
        accent ? "text-paper-text dark:text-ink-text/70" : "text-paper-dim dark:text-ink-text/55"
      }`}
    >
      <span
        className={`rounded px-1.5 py-[3px] font-mono text-[10px] font-medium leading-none ${
          accent
            ? "bg-amber-deep text-white dark:bg-amber dark:text-amber-on"
            : "border border-black/10 bg-black/[.06] text-[#33363c] dark:border-transparent dark:bg-white/[.08] dark:text-ink-text/75"
        }`}
      >
        {keyCap}
      </span>
      {label}
    </span>
  );
}

/**
 * Тост (макет, состояние D): иконка, заголовок, пояснение через « — ».
 * Ширина — по тексту: окно оверлея подкрашено акрилом, и всё, что карточка
 * не закрыла, пользователь видит серой полосой.
 */
function Toast({ text }: { text: string }) {
  const ref = useRef<HTMLDivElement>(null);
  const error = /ключ|key|соединени|connection|лимит|limit|не удалось|failed|отменена|cancel/i.test(text);
  const undo = /вернул|restored|повернув|заменён|replaced|скопирован|copied/i.test(text);
  const [title, ...rest] = text.split(" — ");
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    // Вверх: дробную ширину окно округлит вниз и срежет правый край карточки.
    void commands.resizeToast(Math.ceil(r.width) + 1, Math.ceil(r.height) + 1);
  }, [text]);
  return (
    <motion.div
      ref={ref}
      initial={{ opacity: 0, y: -6 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.14 }}
      className={`inline-flex max-w-[460px] items-center gap-[11px] rounded-[10px] border bg-[#faf8f5] px-3.5 py-3 dark:bg-[#181a1e] ${
        error ? "border-danger/35" : "border-black/[.12] dark:border-white/[.11]"
      }`}
    >
      <span
        className={`flex h-[26px] w-[26px] flex-none items-center justify-center rounded-lg font-mono text-[13px] font-medium ${
          error
            ? "bg-danger/[.14] text-danger-ink dark:text-danger"
            : undo
              ? "bg-amber-deep/[.14] text-amber-ink dark:bg-amber/[.14] dark:text-amber"
              : "bg-black/[.06] text-paper-dim dark:bg-white/[.07] dark:text-ink-text/50"
        }`}
      >
        {error ? "!" : undo ? "↺" : "—"}
      </span>
      <div className="flex min-w-0 flex-col gap-0.5">
        <span className="truncate text-[13px] font-medium leading-tight text-paper-text dark:text-ink-text">{title}</span>
        {rest.length > 0 && (
          <span className="truncate text-[11.5px] leading-tight text-paper-dim dark:text-ink-text/55">{rest.join(" — ")}</span>
        )}
      </div>
    </motion.div>
  );
}
