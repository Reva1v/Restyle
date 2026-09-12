# CLAUDE.md

# Restyle — проектный контекст

Windows-приложение (10 20H2+ / 11): пользователь печатает текст в любом поле,
жмёт хоткей — приложение забирает текст (UIA / клипборд), делает снимок экрана
как контекст, отправляет в Gemini с выбранным стилем и подставляет результат на
место исходного. Полное ТЗ — в стартовом промпте сессии; этот файл фиксирует
принятые решения. Соседний проект-донор: `C:\Users\Reva1v\IdeaProjects\HoldMix`
(тот же стек; оттуда взяты hotkey-поток, окно без кражи фокуса, трей, настройки).

## Стек (зафиксирован, не менять молча)

- Tauri v2, Rust (крейт `windows` 0.58; не `winapi`, не `windows-sys` как основной)
- React 18 + TypeScript + Vite, Tailwind CSS 3, Motion
- `tokio` (control-цикл, захват, AI-клиент — таски на рантайме Tauri),
  `reqwest` (stream/SSE), `serde`, `image` (только jpeg), `base64`.
  Захват экрана — свой BitBlt/GetDIBits через `windows` (`screenshot.rs`):
  `xcap` отвергнут по замеру (PrintWindow ≈ 110 мс на окно 2560×1400)
- `keyring` — API-ключ только в Windows Credential Manager, никогда в JSON/фронте
- `tauri-plugin-store` (settings.json, ключ `settings`), `tauri-plugin-autostart`,
  `tauri-plugin-single-instance`, `window-vibrancy`
- Генерация TS-типов из Rust: `ts-rs` → `src/bindings/` (руками не править)
- Пакетный менеджер: pnpm

## Команды

```
pnpm install                 # зависимости фронтенда
pnpm tauri dev               # дев-режим (Vite :1430 + cargo)
pnpm tauri build             # релизная сборка + MSI (WiX)
pnpm build                   # только фронтенд (tsc + vite build)
cargo test                   # юнит-тесты Rust (из src-tauri/); заодно экспорт ts-rs биндингов
cargo clippy --all-targets   # линт Rust
RESTYLE_DEMO=1 pnpm tauri dev  # показать оверлей через 1.5 с без хоткея
```

Git-коммиты — всегда на английском (глобальное правило пользователя).

## Архитектура (утверждена)

Один процесс; между компонентами — каналы, не мьютексы вокруг WinAPI-хендлов.
`HWND`, COM-указатели и клипборд трогаются только из потока-владельца.

1. **Hotkey-поток** (`hotkey/mod.rs`) — `SetWindowsHookExW(WH_KEYBOARD_LL)` +
   свой `GetMessageW`-loop. Колбэк: чистый автомат `hotkey/combo.rs`
   (`ComboSet` — несколько именованных комбинаций: `main`, `undo`, `style:<id>`)
   + `UnboundedSender::send`. `LLKHF_INJECTED` отсекается (свои SendInput).
   Сработавшая комбинация ГЛОТАЕТСЯ (`return 1`); при видимом оверлее глотаются
   Enter/Esc/Tab/R/стрелки/1-9 и уходят как `PanelKey`. Точное совпадение
   модификаторов (Ctrl+Alt+Shift+R не запускает Ctrl+Alt+R). `SUSPENDED` — пауза
   на время захвата хоткея в настройках. Переустановка по `PostThreadMessageW`;
   watchdog из control-цикла раз в 2 с (`GetLastInputInfo` vs последний тик хука).
2. **Control-цикл** (`control.rs`) — tokio-таск: state-машина hidden/visible,
   позиция у курсора (`GetCursorPos` + `MonitorFromPoint` + `GetDpiForMonitor`,
   обрезка — чистая `window::place_panel`), показ/скрытие через
   `run_on_main_thread` → `ShowWindow(SW_SHOWNA)` / `SetWindowPos(SWP_NOACTIVATE)`.
   Эмит `overlay:show` / `overlay:hide` / `overlay:key`. Фазы 2–4: отсюда
   параллельно стартуют захват текста, захват экрана, AI-запрос.
3. **Capture-поток** (`capture/`) — STA + свой message loop; владеет
   `IUIAutomation`, message-only окном-слушателем клипборда и элементом
   последнего чтения (thread-local, для `ValuePattern.SetValue`). Запросы —
   `mpsc` + `PostThreadMessageW`, ответ — `tokio::oneshot`. Цепочка:
   `uia.rs` (select-only → только `TextPattern.GetSelection`; иначе
   `ValuePattern.CurrentValue` для Edit/Document, затем `TextPattern.DocumentRange`;
   поле пароля → отказ) → `clipboard.rs` (снапшот всех HGLOBAL-форматов →
   Ctrl+A/Ctrl+C через `input.rs` → `WM_CLIPBOARDUPDATE` ≤ 300 мс →
   `VK_RIGHT` снимает выделение → восстановление снапшота СРАЗУ, не после
   вставки) → «Нет текста». `input::mask_menu_activation` (Ctrl down/up при
   зажатом Alt/Win, приём AutoHotkey) вызывается control-циклом сразу после
   срабатывания комбинации — иначе отпускание Alt открывает меню окна.
   `process.rs` — exe окна с фокусом для списков «только выделение»/исключений.
   Отладка: `RESTYLE_FORCE_CLIPBOARD=1` пропускает UIA; `RESTYLE_DEMO=<мс>`.
4. **Скриншот** (`screenshot.rs`) — из control-цикла: настройки
   (`screenshotEnabled`, исключения по exe) → `spawn_blocking(capture)`:
   DWM-границы окна с фокусом (или монитор под курсором), BitBlt|CAPTUREBLT →
   BGRA. Ждём сырой кадр ≤ 150 мс ДО показа оверлея (иначе он в кадре;
   при повторном хоткее оверлей прячется и ждёт 40 мс). Кодирование
   (thumbnail до 1280 px, swap B/R, JPEG q70, base64) — отдельный
   blocking-таск → `ControlMsg::ScreenshotReady{gen}` → `Session.shot`
   (`Shot::None(reason) | Encoding | Ready(Encoded)`), фронту — только
   метаданные `overlay:screenshot`. Байты JPEG живут в памяти сессии.
   Отладка: `RESTYLE_DUMP_SCREENSHOT=<путь>` пишет JPEG на диск.
5. **AI-клиент** (`ai/`) — трейт `Provider` (`rewrite(api_key, req, chunks)`
   → boxed future; отмена = drop future / `JoinHandle::abort`, reqwest рвёт
   стрим — проверено моком), `ai/gemini.rs`: `streamGenerateContent?alt=sse`,
   инкрементальный `SseParser` (CRLF-устойчивый), `extract_text`
   (блокировки → `AiError::Blocked`), минимум размышлений через
   `thinking_config(model)`: 2.5 → `thinkingBudget: 0`, 3.x → `thinkingLevel:
   minimal`; на 400 со словом «thinking» запрос повторяется без него
   (3.8-flash не принимает minimal). Таймауты: 20 с на весь запрос, 6 с connect. Ошибки →
   `AiError::user_message()` (401/403/400 API_KEY_INVALID → «Проверь API-ключ»,
   429 → «Лимит запросов», сеть → «Нет соединения»). Системный промпт —
   `prompts/system.txt` (include_str!), override без пересборки —
   `<config>/system_prompt.txt`. `RESTYLE_GEMINI_BASE_URL` — прокси/мок
   (`scripts/mock_gemini.py`: режимы по ключу `bad-key`/`rate`/`slow`).
   Генерация в control-цикле: `Session.wanted_style` + `maybe_start_generation`
   ждёт текст и решённый скриншот (Encoding → ждём `ScreenshotReady`), затем
   `tokio::spawn` таска с ключом из keyring; хэндл в `Session.generation`.
   Быстрый стиль (`style:<id>`) стартует сам; `select_style`/`regenerate` —
   команды фронта. Модели (замер 12.09.2026 по живому ключу): `gemini-2.5-*`
   недоступны новым ключам (404), дефолт `gemini-3.5-flash-lite` (первый чанк
   ≈0,7 с), пресет «точнее» `gemini-3.6-flash` (≈2,9 с при minimal thinking;
   без thinkingConfig ≈6,6 с). Константы `MODEL_LITE`/`MODEL_FLASH` в settings.rs.
6. **Вставка и undo** (control-цикл + capture-поток). Enter → `paste_result` →
   `do_paste`: если результата ещё нет — `paste_on_done` (событие
   `rewrite:pending_paste`), вставка по `Generated`. Проверка, что foreground
   всё тот же HWND (иначе тост «Окно сменилось»). Оверлей прячется, запрос
   `Request::Paste` на capture-поток: `prefer_uia` (текст читали через UIA,
   элемент writable, не select-only) → `uia::write` (`ValuePattern.SetValue`
   в элемент последнего чтения); иначе клипборд: снапшот → `set_text` →
   `release_modifiers` → Ctrl+A (не в select-only) → Ctrl+V → 150 мс →
   восстановление снапшота. Переводы строк возвращаются в стиль исходника
   (`apply_line_ending`). Быстрый стиль + `autoPaste` → вставка без Enter.
   `history.rs`: последние 20 `HistoryEntry` в памяти, `historyToDisk` →
   `<config>/history.json` (при выключении файл удаляется). Undo (хоткей
   Ctrl+Alt+Z, трей «Вернуть предыдущий текст», команда `undo_last`) — последняя
   запись: в то же окно (проверка HWND) вставляется исходник; повторный undo —
   снова результат (`showing_original`). Для select-only записей текст только
   кладётся в буфер с тостом (выделение не восстановить).
   E2E: `scripts/e2e_paste.ps1` (Блокнот, живой Gemini, демо-переменные
   `RESTYLE_DEMO_PASTE`/`RESTYLE_DEMO_UNDO`; проверяет текст Блокнота через UIA
   и маркер в буфере).
7. **Ключ** (`secrets.rs`) — `keyring` 4 (v1 API, Windows Credential Manager,
   target `gemini-api-key.Restyle`). Команды `set_api_key`/`has_api_key`/
   `clear_api_key`; значение во фронтенд не возвращается никогда.

Окно оверлея создаётся один раз при старте и прячется. Конфигурация:
`decorations: false`, `transparent: true`, `alwaysOnTop: true`, `skipTaskbar: true`,
`visible: false`, `focus: false`, `resizable: false`, `shadow: false`; после
создания через сырой HWND добавляются `WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE`.
Оверлей никогда не забирает фокус — вставка идёт в поле, где остался курсор.
Логический размер 480×400 (`window::PANEL_W/H` = tauri.conf.json).

## Контракт IPC

Источник правды — `src-tauri/src/ipc.rs` и `settings.rs`
(`#[serde(rename_all = "camelCase")]`, `#[derive(TS)]`, экспорт в `src/bindings/`).

События backend → frontend: `overlay:show` (`ShowPayload {x, y, dpiScale, styleId?,
toast?, capturing, targetExe}`), `overlay:hide`, `overlay:key` (vk),
`overlay:text` (`TextPayload {text, source, selectionOnly, uiaWritable, elapsedMs}`),
`settings-changed` (`Settings`). Control-цикл ждёт захват до 250 мс, потом
показывает оверлей с `capturing: true`, текст догоняет событием. Тост
(«Нет текста», «Поле пароля») — тот же оверлей с `toast`, прячется через 1.4 с.

События `rewrite:start {gen, styleId, withScreenshot}` / `rewrite:chunk {gen, text}` /
`rewrite:done {gen, text, elapsedMs, firstChunkMs}` / `rewrite:error {gen, message}`;
`gen` (u64 → TS number) есть и в `overlay:show` — фронт игнорирует чужие поколения.

Команды: `panel_shown` (ack первого кадра — замер задержки), `frontend_ready`,
`hide_overlay`, `select_style(styleId)`, `regenerate`, `paste_result`, `undo_last`, `set_hotkey_capture(on)`,
`get_settings`, `save_settings`, `set_overlay_tint(dark)`, `set_api_key(key)`,
`has_api_key`, `clear_api_key`.
Демо без клавиатуры: `RESTYLE_DEMO=<мс>[:<style_id>]`, `RESTYLE_DEMO_HIDE=<мс>`,
`RESTYLE_DEMO_PASTE=<мс>`, `RESTYLE_DEMO_UNDO=<мс>` (задержки последовательные).

## Настройки (`settings.rs`)

`Settings { mainHotkey, undoHotkey, styles[], model, autostart, theme, autoPaste,
screenshotEnabled, screenshotMode, screenshotExcludedProcesses[],
selectOnlyProcesses[], historyToDisk }`. Дефолт: Ctrl+Alt+R / Ctrl+Alt+Z,
`gemini-3.5-flash-lite`, 7 встроенных стилей (`builtin: true` — не удаляются).
Хранение — `tauri-plugin-store` в каталоге конфига приложения.

## Ключи и тесты

Активный ключ приложения — Credential Manager `gemini-api-key.Restyle`
(платный). Бесплатный лежит в `gemini-api-key-free.Restyle` и приложением не
читается; тесты ИИ гонять на нём (перед прогоном скопировать его в активную
запись через `cmdkey`, после — вернуть платный). Ключи в файлы репозитория и
логи не писать.

## Дизайн (фаза 7, последняя)

Макет — проект Claude Design `https://claude.ai/design/p/36b777b1-d89b-4bbc-8bd2-f905b80811ab?file=Restyle+UI.dc.html`
(файл `Restyle UI.dc.html` + импортируемый `support.js`). Импорт через MCP
`claude_design` (`https://api.anthropic.com/v1/design/mcp`, авторизация
`/design-login`). До фазы 7 UI — функциональная заглушка без стилизации;
переносить один в один, визуал не выдумывать.

## Тесты

Юнит-тесты только на чистую логику: `hotkey/combo.rs`, `window.rs`
(`place_panel`), `settings.rs` (сериализация, список комбинаций),
`capture/text.rs` (фильтр форматов, переводы строк, матчинг процессов),
`ai/gemini.rs` (SSE-парсер, извлечение текста, тело запроса), `ai/mod.rs`
(маппинг статусов), `screenshot.rs` (fit, JPEG, каналы), `history.rs`
(лимит 20, диск, переводы строк).
WinAPI/COM-слой — руками по чек-листу критериев приёмки.

## План работ и статус

1. Скелет: проект, трей, хоткей-поток, оверлей без кражи фокуса, настройки — **готово**
2. Захват текста: UIA + клипборд + фолбэк — **готово** (проверено на Блокноте:
   UIA 12 мс, клипборд 17 мс, текст/картинка в буфере восстанавливаются)
3. Захват экрана с ресайзом и исключениями — **готово** (BitBlt вместо xcap:
   xcap/PrintWindow давал 110 мс на окно; исключения и «без скриншота» проверены)
4. Gemini-клиент со стримингом, системный промпт, стили — **готово** (мок:
   стрим, ошибки, реальный abort; живой API — см. замеры моделей выше)
5. Оверлей с выбором стиля, превью, вставкой, Undo — **готово** (E2E в Блокноте:
   Esc/UIA-вставка/клипборд-вставка/undo/ранний Enter; буфер восстановлен)
6. Окно настроек, кастомные стили, быстрые хоткеи, история
7. Перенос дизайн-макета, анимации, установщик

После каждой фазы — стоп и показ результата.

## Критерии приёмки (сокращённо)

Хоткей → оверлей < 100 мс (с захватом текста и скриншота); CPU idle < 0.5 %,
RAM < 120 МБ; клипборд восстановлен после любого сценария; API-ключ нигде,
кроме keyring; не падает при отказе любого шага; хук переживает снятие по
таймауту.
