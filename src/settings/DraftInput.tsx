import { useEffect, useRef, useState } from "react";

/**
 * Текстовое поле, которое отдаёт значение наверх по потере фокуса (или Ctrl+Enter
 * / Enter для однострочного). Пока поле в фокусе, приходящие снаружи значения
 * игнорируются — иначе ответ бэкенда на предыдущее сохранение затирал бы
 * набираемый текст. Сохранение на каждый символ недопустимо: оно пересобирает
 * ComboSet в hotkey-потоке и пишет store на диск.
 */
export default function DraftInput({
  value,
  onCommit,
  multiline = false,
  rows = 2,
  className = "",
  placeholder,
}: {
  value: string;
  onCommit: (v: string) => void;
  multiline?: boolean;
  rows?: number;
  className?: string;
  placeholder?: string;
}) {
  const [draft, setDraft] = useState(value);
  const focused = useRef(false);

  useEffect(() => {
    if (!focused.current) setDraft(value);
  }, [value]);

  const commit = () => {
    focused.current = false;
    if (draft !== value) onCommit(draft);
  };

  const base =
    "rounded border border-neutral-600 bg-neutral-800 px-2 py-1 text-sm text-neutral-100 " + className;
  const common = {
    value: draft,
    placeholder,
    onFocus: () => {
      focused.current = true;
    },
    onBlur: commit,
    className: base,
  };

  if (multiline) {
    return (
      <textarea
        {...common}
        rows={rows}
        onChange={(e) => setDraft(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) e.currentTarget.blur();
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
      onChange={(e) => setDraft(e.target.value)}
      onKeyDown={(e) => {
        if (e.key === "Enter") e.currentTarget.blur();
        if (e.key === "Escape") {
          setDraft(value);
          e.currentTarget.blur();
        }
      }}
    />
  );
}
