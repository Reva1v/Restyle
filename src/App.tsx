import { useEffect, useRef, useState } from "react";
import {
  commands,
  events,
  VK,
  type Settings,
  type ScreenshotPayload,
  type ShowPayload,
  type TextPayload,
} from "./lib/ipc";

type Gen = {
  state: "idle" | "streaming" | "done" | "error";
  text: string;
  message?: string;
  ms?: string;
  pendingPaste?: boolean;
};

/**
 * Оверлей — функциональная заглушка (фазы 1–4). Окно не имеет фокуса
 * (WS_EX_NOACTIVATE), поэтому клавиатура приходит событием `overlay:key`
 * из LL-хука, а не через DOM. Esc обрабатывает бэкенд (скрытие + abort).
 * Выбор стиля (клик, Tab, стрелки, 1-9) → select_style → генерация.
 */
export default function App() {
  const [visible, setVisible] = useState(false);
  const [payload, setPayload] = useState<ShowPayload | null>(null);
  const [text, setText] = useState<TextPayload | null>(null);
  const [shot, setShot] = useState<ScreenshotPayload | null>(null);
  const [styles, setStyles] = useState<Settings["styles"]>([]);
  const [selected, setSelected] = useState<number | null>(null);
  const [gen, setGen] = useState<Gen>({ state: "idle", text: "" });
  const genRef = useRef(0);
  const stylesRef = useRef(styles);
  stylesRef.current = styles;

  // Выбор стиля из клавиш/кликов: обновить подсветку и запустить генерацию
  function choose(i: number) {
    const s = stylesRef.current[i];
    if (!s) return;
    setSelected(i);
    void commands.selectStyle(s.id);
  }

  useEffect(() => {
    void commands.getSettings().then((s) => setStyles(s.styles));
    const uns = [
      events.onSettingsChanged((s) => setStyles(s.styles)),
      events.onShow((p) => {
        genRef.current = p.gen;
        setPayload(p);
        setText(null);
        setShot(null);
        setGen({ state: "idle", text: "" });
        setSelected(p.styleId ? Math.max(0, stylesRef.current.findIndex((s) => s.id === p.styleId)) : null);
        setVisible(true);
        // ack после первого отрисованного кадра — замер задержки на бэкенде
        requestAnimationFrame(() => requestAnimationFrame(() => void commands.panelShown()));
      }),
      events.onText((t) => setText(t)),
      events.onScreenshot((s) => setShot(s)),
      events.onHide(() => setVisible(false)),
      events.onRewriteStart((p) => {
        if (p.gen !== genRef.current) return;
        setGen({ state: "streaming", text: "" });
      }),
      events.onRewriteChunk((p) => {
        if (p.gen !== genRef.current) return;
        setGen((g) => ({ state: "streaming", text: g.text + p.text }));
      }),
      events.onRewriteDone((p) => {
        if (p.gen !== genRef.current) return;
        setGen({
          state: "done",
          text: p.text,
          ms: `первый чанк ${p.firstChunkMs.toFixed(0)} мс · всего ${p.elapsedMs.toFixed(0)} мс`,
        });
      }),
      events.onPendingPaste((g) => {
        if (g !== genRef.current) return;
        setGen((cur) => ({ ...cur, pendingPaste: true }));
      }),
      events.onRewriteError((p) => {
        if (p.gen !== genRef.current) return;
        setGen((g) => ({ state: "error", text: g.text, message: p.message }));
      }),
      events.onKey((vk) => {
        const n = stylesRef.current.length;
        if (n === 0) return;
        if (vk === VK.R) {
          void commands.regenerate();
          return;
        }
        if (vk === VK.ENTER) {
          void commands.pasteResult();
          return;
        }
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
    // после оформления подписок — сообщить бэкенду (догоняем пропущенный show)
    void Promise.all(uns).then(() => commands.frontendReady());
    return () => {
      for (const u of uns) void u.then((f) => f());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  if (!visible) return null;

  if (payload?.toast) {
    return (
      <div className="flex h-screen items-center px-4 text-sm text-neutral-100">
        <span className="rounded bg-black/40 px-3 py-1.5">{payload.toast}</span>
      </div>
    );
  }

  const sourceLabel = text
    ? `${text.source === "uia" ? "UIA" : "клипборд"}${text.selectionOnly ? " · выделение" : ""}${
        text.uiaWritable ? " · запись UIA" : ""
      } · ${text.elapsedMs.toFixed(0)} мс`
    : payload?.capturing
      ? "читаю текст…"
      : "";

  const shotLabel = shot
    ? `скриншот ${shot.width}×${shot.height}, ${(shot.bytes / 1024).toFixed(0)} КБ, ${shot.captureMs.toFixed(0)}+${shot.encodeMs.toFixed(0)} мс`
    : payload?.screenshot === "pending"
      ? "скриншот: кодирую…"
      : payload?.screenshot === "excluded"
        ? "без скриншота: исключение"
        : payload?.screenshot === "failed"
          ? "без скриншота: захват не удался"
          : "без скриншота";

  return (
    <div className="flex h-screen flex-col gap-3 p-4 text-sm text-neutral-100">
      <div className="flex items-center justify-between">
        <span className="font-semibold">
          Restyle <span className="font-normal text-neutral-400">· {payload?.targetExe || "?"}</span>
        </span>
        <span className="text-xs text-neutral-400">{shotLabel}</span>
      </div>

      <div className="rounded bg-black/30 px-2 py-1">
        <div className="line-clamp-2 whitespace-pre-wrap break-words text-neutral-200">
          {text ? text.text : payload?.capturing ? "…" : "Нет текста"}
        </div>
        <div className="mt-1 text-xs text-neutral-500">
          {sourceLabel}
          {text ? ` · ${text.text.length} симв.` : ""}
        </div>
      </div>

      <div className="flex flex-wrap gap-1.5">
        {styles.map((s, i) => (
          <button
            key={s.id}
            type="button"
            onClick={() => choose(i)}
            className={`rounded px-2 py-1 text-xs ${
              i === selected ? "bg-blue-600 text-white" : "bg-white/10 text-neutral-200"
            }`}
          >
            {i + 1}. {s.name}
          </button>
        ))}
      </div>

      <div className="flex-1 overflow-auto whitespace-pre-wrap break-words rounded bg-black/30 p-2 text-neutral-100">
        {gen.state === "idle" && <span className="text-neutral-500">Выбери стиль — результат появится здесь</span>}
        {gen.text}
        {gen.state === "streaming" && <span className="animate-pulse text-neutral-400">▍</span>}
        {gen.state === "error" && <div className="mt-1 text-red-400">{gen.message}</div>}
      </div>

      <div className="flex items-center justify-between text-xs text-neutral-400">
        <span>
          {gen.pendingPaste && gen.state === "streaming"
            ? "вставлю по готовности…"
            : gen.state === "done"
              ? `${gen.ms} · Enter — вставить`
              : "Enter — вставить · Esc — отмена · R — заново · Tab/↑↓/1-9 — стиль"}
        </span>
        <button type="button" className="rounded bg-white/10 px-2 py-0.5" onClick={() => void commands.hideOverlay()}>
          Закрыть
        </button>
      </div>
    </div>
  );
}
