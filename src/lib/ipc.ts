import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { ShowPayload } from "../bindings/ShowPayload";
import type { Settings } from "../bindings/Settings";
import type { HotkeyCombo } from "../bindings/HotkeyCombo";
import type { Style } from "../bindings/Style";
import type { TextPayload } from "../bindings/TextPayload";
import type { ScreenshotPayload } from "../bindings/ScreenshotPayload";
import type { RewriteStart } from "../bindings/RewriteStart";
import type { RewriteChunk } from "../bindings/RewriteChunk";
import type { RewriteDone } from "../bindings/RewriteDone";
import type { RewriteError } from "../bindings/RewriteError";
import type { HistoryItem } from "../bindings/HistoryItem";

// Типы данных генерируются ts-rs в src/bindings/ (cargo test export_bindings).
// Этот файл — только тонкая обёртка invoke/listen.
export type { ShowPayload, Settings, HotkeyCombo, Style, TextPayload, ScreenshotPayload, HistoryItem };
export type { RewriteStart, RewriteChunk, RewriteDone, RewriteError };

export const events = {
  onShow: (cb: (p: ShowPayload) => void): Promise<UnlistenFn> =>
    listen<ShowPayload>("overlay:show", (e) => cb(e.payload)),
  onHide: (cb: () => void): Promise<UnlistenFn> =>
    listen("overlay:hide", () => cb()),
  /** захваченный исходный текст (может прийти позже overlay:show) */
  onText: (cb: (t: TextPayload) => void): Promise<UnlistenFn> =>
    listen<TextPayload>("overlay:text", (e) => cb(e.payload)),
  /** метаданные готового скриншота (байты остаются на бэкенде) */
  onScreenshot: (cb: (s: ScreenshotPayload) => void): Promise<UnlistenFn> =>
    listen<ScreenshotPayload>("overlay:screenshot", (e) => cb(e.payload)),
  onRewriteStart: (cb: (p: RewriteStart) => void): Promise<UnlistenFn> =>
    listen<RewriteStart>("rewrite:start", (e) => cb(e.payload)),
  onRewriteChunk: (cb: (p: RewriteChunk) => void): Promise<UnlistenFn> =>
    listen<RewriteChunk>("rewrite:chunk", (e) => cb(e.payload)),
  onRewriteDone: (cb: (p: RewriteDone) => void): Promise<UnlistenFn> =>
    listen<RewriteDone>("rewrite:done", (e) => cb(e.payload)),
  onRewriteError: (cb: (p: RewriteError) => void): Promise<UnlistenFn> =>
    listen<RewriteError>("rewrite:error", (e) => cb(e.payload)),
  /** Enter нажат до конца генерации: вставим по готовности */
  onPendingPaste: (cb: (gen: number) => void): Promise<UnlistenFn> =>
    listen<number>("rewrite:pending_paste", (e) => cb(e.payload)),
  /** vk-код клавиши оверлея (Enter, Tab, R, стрелки, 1-9); Esc обрабатывает бэкенд */
  onKey: (cb: (vk: number) => void): Promise<UnlistenFn> =>
    listen<number>("overlay:key", (e) => cb(e.payload)),
  onSettingsChanged: (cb: (s: Settings) => void): Promise<UnlistenFn> =>
    listen<Settings>("settings-changed", (e) => cb(e.payload)),
};

export const commands = {
  /** ack для замера задержки показа: вызывается после первого кадра */
  panelShown: () => invoke("panel_shown"),
  /** подписки оформлены; бэкенд повторит overlay:show, если панель уже видима */
  frontendReady: () => invoke("frontend_ready"),
  hideOverlay: () => invoke("hide_overlay"),
  /** выбрать стиль — запускает (пере)генерацию */
  selectStyle: (styleId: string) => invoke("select_style", { styleId }),
  regenerate: () => invoke("regenerate"),
  /** Enter: вставить результат (или дождаться его) */
  pasteResult: () => invoke("paste_result"),
  undoLast: () => invoke("undo_last"),
  /** ключ уходит в keyring и назад не возвращается */
  setApiKey: (key: string): Promise<void> => invoke("set_api_key", { key }),
  hasApiKey: (): Promise<boolean> => invoke("has_api_key"),
  clearApiKey: (): Promise<void> => invoke("clear_api_key"),
  /** пауза хука на время захвата новой комбинации в настройках */
  setHotkeyCapture: (on: boolean) => invoke("set_hotkey_capture", { on }),
  getSettings: (): Promise<Settings> => invoke("get_settings"),
  /** бэкенд нормализует (id стилей, пустые строки, комбо без модификатора) и
   *  возвращает то, что реально сохранено */
  saveSettings: (settings: Settings): Promise<Settings> => invoke("save_settings", { settings }),
  /** пары действий с одинаковой комбинацией: ["main", "style:formal"] */
  hotkeyConflicts: (settings: Settings): Promise<[string, string][]> =>
    invoke("hotkey_conflicts", { settings }),
  /** встроенные стили как в коробке — для кнопки «вернуть» */
  defaultStyles: (): Promise<Style[]> => invoke("default_styles"),
  /** последние переписывания, новые первыми */
  getHistory: (): Promise<HistoryItem[]> => invoke("get_history"),
  /** положить результат записи в буфер обмена */
  copyHistory: (index: number) => invoke("copy_history", { index }),
  clearHistory: () => invoke("clear_history"),
  /** перекрасить акриловый тинт окна оверлея под тёмную/светлую тему */
  setOverlayTint: (dark: boolean) => invoke("set_overlay_tint", { dark }),
};

export const VK = {
  ENTER: 0x0d,
  TAB: 0x09,
  R: 0x52,
  LEFT: 0x25,
  UP: 0x26,
  RIGHT: 0x27,
  DOWN: 0x28,
} as const;
