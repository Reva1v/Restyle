import type { HotkeyCombo } from "../bindings/HotkeyCombo";

/** Человекочитаемое имя VK-кода (латиница/цифры/F-ряд, остальное — hex). */
export function vkName(vk: number): string {
  if (vk >= 0x30 && vk <= 0x39) return String.fromCharCode(vk); // 0-9
  if (vk >= 0x41 && vk <= 0x5a) return String.fromCharCode(vk); // A-Z
  if (vk >= 0x70 && vk <= 0x87) return `F${vk - 0x6f}`;
  const named: Record<number, string> = {
    0x20: "Space", 0x09: "Tab", 0x14: "CapsLock", 0xc0: "~",
    0xbd: "-", 0xbb: "=", 0xdb: "[", 0xdd: "]", 0xba: ";",
    0xde: "'", 0xbc: ",", 0xbe: ".", 0xbf: "/", 0xdc: "\\",
  };
  return named[vk] ?? `0x${vk.toString(16).toUpperCase()}`;
}

export function comboLabel(c: HotkeyCombo | null | undefined): string {
  if (!c) return "—";
  const parts: string[] = [];
  if (c.ctrl) parts.push("Ctrl");
  if (c.alt) parts.push("Alt");
  if (c.shift) parts.push("Shift");
  if (c.win) parts.push("Win");
  for (const k of c.extraKeys) parts.push(vkName(k));
  if (c.key !== 0) parts.push(vkName(c.key));
  return parts.length ? parts.join(" + ") : "—";
}

/** e.code -> VK (основные клавиши; модификаторы обрабатываются отдельно) */
export function codeToVk(e: KeyboardEvent): number | null {
  const code = e.code;
  if (/^Key[A-Z]$/.test(code)) return code.charCodeAt(3);
  if (/^Digit[0-9]$/.test(code)) return 0x30 + Number(code[5]);
  if (/^F([1-9]|1[0-9]|2[0-4])$/.test(code)) return 0x70 + Number(code.slice(1)) - 1;
  const map: Record<string, number> = {
    Space: 0x20, Backquote: 0xc0, Minus: 0xbd, Equal: 0xbb,
    BracketLeft: 0xdb, BracketRight: 0xdd, Semicolon: 0xba,
    Quote: 0xde, Comma: 0xbc, Period: 0xbe, Slash: 0xbf, Backslash: 0xdc,
  };
  return map[code] ?? null;
}

export const EMPTY_COMBO: HotkeyCombo = {
  ctrl: false, alt: false, shift: false, win: false, key: 0, extraKeys: [],
};
