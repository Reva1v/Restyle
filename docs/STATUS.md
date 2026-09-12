# Restyle — статус проекта (12.09.2026)

Завершены фазы 1–5 из 7. Фаза 6 (окно настроек, кастомные стили, быстрые
хоткеи, история в трее) и фаза 7 (дизайн-макет, анимации, MSI) не начаты.
Архитектура, контракт IPC и принятые решения — в `CLAUDE.md`.

## Что работает

- Хоткей Ctrl+Alt+R → оверлей у курсора без кражи фокуса; Esc/Enter/R/Tab/↑↓/1-9
  через LL-хук; трей; автозапуск; single instance; настройки в
  `tauri-plugin-store`.
- Захват текста: UIA (TextPattern/ValuePattern, select-only по списку процессов,
  поля паролей не читаются) → клипборд (Ctrl+A/Ctrl+C, WM_CLIPBOARDUPDATE ≤ 300 мс,
  снапшот всех HGLOBAL-форматов, восстановление сразу после чтения).
- Скриншот окна с фокусом/монитора: BitBlt → 1280 px → JPEG q70 → base64;
  исключения по процессам; глобальный выключатель.
- Gemini SSE-стриминг, трейт `Provider`, системный промпт в
  `src-tauri/prompts/system.txt` (override `<config>/system_prompt.txt`),
  ключ в Credential Manager, отмена по Esc рвёт HTTP-стрим.
- Вставка: UIA `SetValue` или клипборд + Ctrl+A/Ctrl+V с восстановлением буфера
  через 150 мс; ранний Enter вставляет по готовности; `autoPaste` для быстрых
  стилей; Undo (Ctrl+Alt+Z / трей) с историей 20 пар и опцией на диск.

## Замеры (dev opt-level 1 / release, Блокнот 2560×1400)

| Метрика | Значение |
|---|---|
| Хоткей → отрисованный оверлей с текстом и снятым кадром | 38–46 мс |
| Захват текста UIA / клипборд | 10 мс / 10–18 мс |
| Скриншот захват + кодирование | 16 + 41 мс (release 23 + 39) |
| Первый чанк gemini-3.5-flash-lite (со скриншотом) | 0,9–1,5 с |
| Первый чанк gemini-3.6-flash | 0,8–2,9 с |

## Как проверять

```
pnpm install && pnpm tauri dev          # приложение
cd src-tauri && cargo test               # 44 юнит-теста + экспорт ts-rs
python scripts/mock_gemini.py 8765       # мок SSE (ключи bad-key / rate / slow)
pwsh -File scripts/e2e_paste.ps1         # E2E фазы 5 в Блокноте (нужны Vite и debug-сборка)
```

Демо-переменные для запуска без клавиатуры: `RESTYLE_DEMO=<мс>[:<style>]`,
`RESTYLE_DEMO_HIDE`, `RESTYLE_DEMO_PASTE`, `RESTYLE_DEMO_UNDO`,
`RESTYLE_FORCE_CLIPBOARD=1`, `RESTYLE_DUMP_SCREENSHOT=<путь>`,
`RESTYLE_GEMINI_BASE_URL=<url>`.

## Отклонения от ТЗ (осознанные)

- `xcap` заменён на собственный BitBlt: PrintWindow давал ~110 мс на окно.
- Клипборд восстанавливается сразу после чтения, а не после вставки.
- Дефолтная модель `gemini-3.5-flash-lite`: `gemini-2.5-*` недоступны новым
  ключам (404), у 3.6-flash первый чанк заметно медленнее.
- Undo в режиме «только выделение» не вставляет, а кладёт исходник в буфер.

## Что проверить руками (у автоматики не было доступа)

Telegram Desktop, Chrome (textarea и contenteditable), VS Code и Word — источник
текста в подписи оверлея; Ctrl+Alt+R в Блокноте не должен открывать меню File
по отпусканию Alt; Ctrl+Alt+Z после вставки.

## Известные ограничения

- Watchdog хука иногда ложно переустанавливает его при чисто мышиной активности
  (идемпотентно).
- UWP-приложения показываются как `ApplicationFrameHost.exe` в списках процессов.
- Первое обращение UIA к Chrome включает у него accessibility-режим.
