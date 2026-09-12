# OpenWolf

@.wolf/OPENWOLF.md

This project uses OpenWolf for context management. Read and follow .wolf/OPENWOLF.md every session. Check .wolf/cerebrum.md before generating code. Check .wolf/anatomy.md before reading files.


# CLAUDE.md

# Restyle — проектный контекст

Windows-приложение (10 20H2+ / 11): пользователь печатает текст в любом поле,
жмёт хоткей — приложение забирает текст (UIA / клипборд), делает снимок экрана
как контекст, отправляет в Gemini с выбранным стилем и подставляет результат на
место исходного. Полное ТЗ — в стартовом промпте сессии; этот файл фиксирует
принятые решения. Соседний проект-донор: HoldMix
(тот же стек; оттуда взяты hotkey-поток, окно без кражи фокуса, трей, настройки).

## Стек (зафиксирован, не менять молча)

- Tauri v2, Rust (крейт `windows` 0.58; не `winapi`, не `windows-sys` как основной)
- React 18 + TypeScript + Vite, Tailwind CSS 3, Motion (анимации оверлея,
  разделов настроек, шагов мастера), шрифты IBM Plex Sans/Mono локально
  (`@fontsource`, без обращений в сеть)
- `tokio` (control-цикл, захват, AI-клиент — таски на рантайме Tauri),
  `reqwest` (stream/SSE), `serde`, `image` (только jpeg), `base64`.
  Захват экрана — свой BitBlt/GetDIBits через `windows` (`screenshot.rs`):
  `xcap` отвергнут по замеру (PrintWindow ≈ 110 мс на окно 2560×1400)
- `keyring` — API-ключи (Gemini и DeepL) только в Windows Credential Manager,
  никогда в JSON/фронте
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
   (`ComboSet` — несколько именованных комбинаций: `main`, `undo`, `layout`, `style:<id>`;
   двойное нажатие модификатора — `[VK_DOUBLE_TAP, VK]`, срабатывает на втором отпускании и не глотается)
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
   `<config>/system_prompt.txt`. `RESTYLE_GEMINI_BASE_URL` — прокси/мок (в релизе только localhost, см. `ai::base_url_override`;
   `RESTYLE_DUMP_SCREENSHOT` — только debug)
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
10. **Регистр и раскладка** (`textfx.rs`, без модели). Кнопка «Aa Регистр ▾» в панели шлёт `open_format_menu(left, top, bottom, current)`: список показывает ОКНО МЕНЮ ТРЕЯ (`MenuMode::Format`, событие `menu:format`) под кнопкой или над ней по рабочей области — панель не растёт и не двигается; клик мимо закрывает его тем же хуком мыши. Пункт шлёт `select_style("format:<sentence|lower|upper|title|toggle>")`; `maybe_start_generation` считает регистр на месте → `rewrite:start/done` + `Generated`. UIA-захват отдаёт и выделение (`Captured.selection`: текст, начало в символах, смещения UIA): `textfx::apply_to` меняет только выделение и вклеивает его в текст поля, а если длина не изменилась — `PasteOptions.caret` возвращает выделение/каретку (`uia::select_range`) после вставки. Хоткей `layoutHotkey` (combo `layout`, дефолт «Ctrl, Ctrl») — `fix_layout`: захват → «ghbdtn» ↔ «привет» (направление по большинству букв; украинская — если установлена только она или язык интерфейса `uk`) → `WM_INPUTLANGCHANGEREQUEST` → вставка без панели. Двойное нажатие модификатора: `HotkeyCombo.double`, в хук уходит `[VK_DOUBLE_TAP, VK]`, `ComboSet` ловит два тапа (≤300 мс каждый, ≤400 мс между) и отдаёт `Edge::Double` на втором отпускании — не глотается.
7. **Перевод** (`translate.rs`) — стиль с `kind = translate` идёт не в модель,
   а в DeepL: `POST /v2/translate` с `preserve_formatting`, ключ из keyring
   (`deepl-api-key.Restyle`). Хост выбирается по ключу: у бесплатных аккаунтов
   он оканчивается на `:fx` → `api-free.deepl.com`. Нет ключа или
   `translator = "gemini"` — переводит модель по инструкции стиля (приложение
   не ломается). `post_style` — id обычного стиля: перевод уходит в модель и
   получает его инструкцию, так «перевести на английский и сделать формальным»
   — один стиль. `GET /v2/usage` показывает остаток квоты в настройках.
   Мок: `RESTYLE_DEEPL_BASE_URL`.
8. **Ключ** (`secrets.rs`) — `keyring` 4 (v1 API, Windows Credential Manager,
   target `gemini-api-key.Restyle`). Команды `set_api_key`/`has_api_key`/
   `clear_api_key`; значение во фронтенд не возвращается никогда.
9. **Окна и трей** (`tray.rs`, `src/settings/`, `src/history/`, `src/welcome/`) —
   окна настроек/истории/мастера создаются ТОЛЬКО из не-главного потока:
   `WebviewWindowBuilder::build()` из главного потока встаёт намертво (окно
   создаёт событийный цикл, которого мы же и ждём), вебвью остаётся белым —
   поэтому команды `open_*_window` объявлены `async`, а обработчик меню трея
   уводит вызов в `std::thread::spawn`. Меню трея — СВОЁ ОКНО (`menu.html`), а не системное меню: статичное меню
   Tauri v2 при пересборке подменю дублировало пункты и выглядело чужеродно.
   Окно создаётся при старте и живёт скрытым (поднимать webview по клику —
   сотни миллисекунд), фокус не забирает (`WS_EX_NOACTIVATE`), иначе «вернуть
   предыдущий текст» вставляло бы в само меню. Клик мимо ловит LL-хук мыши,
   который ставится только на время показа меню (`WM_APP_MENU` в hotkey-поток),
   Esc — клавиатурным хуком. Пункт истории кладёт результат в буфер
   (`ControlMsg::CopyHistory`) — HWND из записи давно мог протухнуть.
   ВАЖНО: окно, которого нет в `capabilities/default.json`, не может слушать
   события (`listen`) — свои команды работают, а `overlay:*`/`settings-changed`
   молча не приходят. Там перечислены все окна. Окно настроек —
   `SettingsApp.tsx`: тумблеры и хоткеи сохраняются сразу, текстовые поля по
   потере фокуса (`DraftInput`), каждое сохранение пересобирает `ComboSet` и
   пишет store. `HotkeyField` ведёт захват строго в одном поле и снимает
   `set_hotkey_capture(false)` даже при размонтировании; комбинация без
   модификатора не принимается (иначе хук глотал бы обычную клавишу во всех
   программах). Закрытие окна крестиком React не размонтирует, поэтому паузу
   снимает и бэкенд — обработчик `WindowEvent::Destroyed | CloseRequested`
   в `open_settings`; переходы `set_suspended` пишутся в лог (залипший `true`
   иначе никак не увидеть: хоткеи просто перестают работать).

Иконка — тёмная плашка `#181a1e` с янтарной `R` (`scripts/make_icons.py`,
IBM Plex Sans 700 из `node_modules`): на тёмной панели задач плашка сливается с
фоном и видна чистая янтарная буква, на светлой — тёмный значок. Каждый размер
рисуется отдельно (даунскейл 256→16 мылит букву); трею отдаётся PNG ровно того
размера, который просит `GetSystemMetrics(SM_CXSMICON)` (16 / 20 / 24 / 32),
файлы вшиты через `include_bytes!`. `icon.ico` — все восемь размеров; в PIL
базовой картинкой при сборке ICO обязана быть самая большая, иначе остальные
молча выбрасываются.

Окно оверлея создаётся один раз при старте и прячется. Конфигурация:
`decorations: false`, `transparent: true`, `alwaysOnTop: true`, `skipTaskbar: true`,
`visible: false`, `focus: false`, `resizable: false`, `shadow: false`; после
создания через сырой HWND добавляются `WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE`.
Оверлей никогда не забирает фокус — вставка идёт в поле, где остался курсор.
Акрила у оверлея НЕТ (у меню трея есть): DWM рисует его на всё окно, и всё,
не закрытое CSS, видно серым; `SetWindowRgn` композиция WebView2 игнорирует.
Системная рамка и скругление сняты (`window::clear_dwm_frame`), углы — CSS.
Логический размер 520×148 на старте (`window::PANEL_W/H` = tauri.conf.json), высоту
присылает фронтенд по содержимому (`resize_overlay`).

## Контракт IPC

Источник правды — `src-tauri/src/ipc.rs` и `settings.rs`
(`#[serde(rename_all = "camelCase")]`, `#[derive(TS)]`, экспорт в `src/bindings/`).

События backend → frontend: `overlay:show` (`ShowPayload {x, y, dpiScale, styleId?,
toast?, capturing, targetExe}`), `overlay:hide`, `overlay:key` (vk),
`overlay:text` (`TextPayload {text, source, selectionOnly, uiaWritable, elapsedMs}`),
`settings-changed` (`Settings`). Control-цикл ждёт захват до 250 мс, потом
показывает оверлей с `capturing: true`, текст догоняет событием. Тост
(«Нет текста», «Поле пароля») — тот же оверлей с `toast`, прячется через 1.4 с.

`overlay:ring` (`RingPayload {gen, x, y, dpiScale, fillMs}`) — кольцо
удержания у курсора; `menu:show` (масштаб экрана) / `menu:hide` — своё меню трея;
`menu:format {scale, current}` — то же окно показывает список регистров.

События `rewrite:start {gen, styleId, withScreenshot}` / `rewrite:chunk {gen, text}` /
`rewrite:done {gen, text, elapsedMs, firstChunkMs}` / `rewrite:error {gen, message}`;
`gen` (u64 → TS number) есть и в `overlay:show` — фронт игнорирует чужие поколения.

Команды: `panel_shown` (ack первого кадра — замер задержки), `frontend_ready`,
`hide_overlay`, `select_style(styleId)`, `regenerate`, `paste_result`, `undo_last`, `set_hotkey_capture(on)`,
`get_settings`, `save_settings` (нормализует и возвращает сохранённое),
`hotkey_conflicts(settings)` (пары действий с одинаковой комбинацией; первая
в паре — та, что реально сработает), `default_styles`, `get_history`,
`copy_history(index)`, `clear_history`, `undo_entry(index)`,
`resize_overlay(height)` (высота HUD по содержимому),
`resize_toast(width, height)` (тост — окно ровно по карточке: всё вне
карточки прозрачно, но ловило бы клики),
`list_models` (каталог моделей ключа через ListModels, фильтр по
`generateContent` и по именам — картинки/tts/эмбеддинги отсеиваются),
`running_apps` (видимые окна → exe + заголовок, для списков процессов),
`set_deepl_key`/`has_deepl_key`/`clear_deepl_key`/`test_deepl_key` (проверка +
остаток квоты), `exit_app`, `close_tray_menu`, `resize_menu(height)`, `open_format_menu(left, top, bottom, current)` (список регистров
в окне меню), `open_url(url)` (ShellExecute, белый список github.com / www.deepl.com /
aistudio.google.com),
`open_settings_window`, `open_history_window`, `win_minimize`, `win_close`
(своя шапка окна), `diagnostics` (стоит ли хук), `test_api_key(key)` (проверка
живым запросом, в keyring не пишет), `finish_onboarding`,
`set_api_key(key)`, `has_api_key`, `clear_api_key`.
Демо без клавиатуры: `RESTYLE_DEMO=<мс>[:<style_id>]`, `RESTYLE_DEMO_HIDE=<мс>`,
`RESTYLE_DEMO_PASTE=<мс>`, `RESTYLE_DEMO_UNDO=<мс>` (задержки последовательные),
`RESTYLE_DEMO_RELEASE=<мс>` (отпустить основную комбинацию — развилка
«короткое/долгое»), `RESTYLE_DEMO_MENU=<мс>` (открыть меню трея), `RESTYLE_DEMO_SELECT=<мс>:<style_id>`
(выбрать стиль в открытой панели).
`RESTYLE_LOG_TRAY=1` печатает события значка в трее: системного меню нет,
и «клик не сработал» от «событие не пришло» иначе не отличить (проверено:
Windows шлёт `Click{button_state: Down}` и `Up` и для левой, и для правой).

## Настройки (`settings.rs`)

`Settings { mainHotkey, undoHotkey, layoutHotkey, styles[], model, autostart, theme, accent, autoPaste,
screenshotEnabled, screenshotMode, screenshotExcludedProcesses[],
selectOnlyProcesses[], historyToDisk, historyLimit, language, restoreClipboard,
onboarded, quickStyle, longPress, longPressRingMs, longPressOpenMs, translator }`;
у стиля — `screenshot` (быстрый стиль без кадра не тратит 20–60 мс),
`kind` (`prompt` | `translate`), `targetLang`, `postStyle`.
Долгое нажатие: короткое нажатие основной комбинации применяет `quickStyle`,
удержание `longPressRingMs` показывает кольцо у курсора, `longPressOpenMs` —
панель выбора. Захват текста и кадра стартует по нажатию, а не по развилке,
иначе быстрый путь потерял бы эти миллисекунды. Дефолт: Ctrl+Alt+R / Ctrl+Alt+Z / «Ctrl, Ctrl»,
`gemini-3.5-flash-lite`, 8 встроенных стилей (`builtin: true`; удалять можно, «Вернуть удалённые встроенные» —
в разделе стилей).
Хранение — `tauri-plugin-store` в каталоге конфига приложения.
`Settings::normalize` вызывается и при чтении, и перед сохранением: id стиля
берётся из имени (`slug`, кириллица даёт фолбэк `style`) и уникализируется
суффиксом, `builtin` выставляется по совпадению id со встроенным, пустые/битые
`theme`/`screenshotMode`/`model` заменяются дефолтом, списки процессов чистятся
от дублей и пробелов, комбинации без модификатора отбрасываются (основной
хоткей возвращается к дефолту — без него приложение не открыть).

## Ключи и тесты

Активный ключ приложения — Credential Manager `gemini-api-key.Restyle`
(платный). Бесплатный лежит в `gemini-api-key-free.Restyle` и приложением не
читается; тесты ИИ гонять на нём (перед прогоном скопировать его в активную
запись через `cmdkey`, после — вернуть платный). Ключи в файлы репозитория и
логи не писать. Ключ DeepL — `deepl-api-key.Restyle`, пользовательский;
без него перевод делает модель.

## Дизайн (перенесён в фазе 7)

Макет — проект Claude Design `https://claude.ai/design/p/36b777b1-d89b-4bbc-8bd2-f905b80811ab?file=Restyle+UI.dc.html`
(файл `Restyle UI.dc.html` + `support.js`), читается инструментом `DesignSync`
(`/design-login`). Перенесены все восемь разделов макета.

**Токены** (`tailwind.config.ts`): IBM Plex Sans — интерфейс, IBM Plex Mono —
клавиши, хоткеи, имена процессов и id. Акцент выбирается во вкладке «Интерфейс» (`settings.accent`, пресеты
`ACCENTS` в settings.rs и цвета в `src/lib/theme.ts`); классы `amber-*` — это
CSS-переменные «R G B», новый хардкод цвета акцента не писать. По умолчанию янтарный: `#e8a33d` в тёмной теме,
`#c8821f` в светлой (не путается с синим системным выделением Windows).
Поверхности: тёмные `ink.*` (#0d0e10…#23262b), светлые `paper.*` (#f4f2ef…),
ошибка `#f2756a`, успех `#3f8f5e`.

**Темы.** Базовые классы Tailwind — светлая тема, `dark:` — тёмная. Тема ставится
атрибутом `data-theme` на `<html>` (`src/lib/theme.ts`), в конфиге
`darkMode: ["selector", '[data-theme="dark"]']` — именно `selector`, а не
вариант с `:where()`: у `:where()` нулевая специфичность, и светлый класс
перебивал бы тёмный. Акрил окна красится только у меню трея
(`apply_overlay_tint`); у оверлея фон рисует CSS.

**Экраны.** Оверлей — HUD 520 px с динамической высотой; окно настроек 840×620 с
боковой навигацией из шести разделов (модель и ключ, хоткеи, стили, приватность,
вставка и история, интерфейс); окно истории 840×560; мастер первого
запуска 560×540. У всех окон, кроме оверлея, свои шапки (`decorations: false`,
`TitleBar` с `data-tauri-drag-region`).

**Языки** (`src/lib/i18n.ts`): русский, украинский, английский; настройка
`language` со значением `system` берёт язык из `navigator.language`.

## Тесты

Юнит-тесты только на чистую логику: `hotkey/combo.rs`, `window.rs`
(`place_panel`), `settings.rs` (сериализация, список комбинаций),
`capture/text.rs` (фильтр форматов, переводы строк, матчинг процессов),
`ai/gemini.rs` (SSE-парсер, извлечение текста, тело запроса), `ai/mod.rs`
(маппинг статусов), `screenshot.rs` (fit, JPEG, каналы), `history.rs`
(лимит 20, диск, переводы строк, порядок `items`, `preview`), `settings.rs`
(нормализация id и перечислений, конфликты хоткеев, комбо без модификатора),
`textfx.rs` (регистр, раскладка EN↔RU/UK, вклейка выделения), `hotkey/combo.rs`
(двойное нажатие модификатора). Всего 79 тестов.
Замеры на релизе — `scripts/perf_release.ps1`: считает CPU и память по всему
дереву процессов (вебвью живут в отдельных `msedgewebview2.exe`) и КЛИКАЕТ в
поле перед демо-хоткеем: `SendKeys('%')` для разблокировки `SetForegroundWindow`
уводит клавиатурный фокус на саму форму, UIA видит окно вместо поля и замер
превращается в «Нет текста» через 250 мс.
E2E фазы 6 — `scripts/e2e_settings.ps1` (быстрый хоткей стиля, автовставка,
история на диск, содержимое подменю трея из лога, скриншоты окна настроек,
кнопка «В буфер» через UIA); свой `settings.json` скрипт сохраняет и
возвращает в `finally`.
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
6. Окно настроек, кастомные стили, быстрые хоткеи, история — **готово**
   (проверено: стиль с быстрым хоткеем и автовставкой, подменю трея, история
   на диске, «В буфер»; правка стилей и списков процессов из окна)
7. Перенос дизайн-макета, анимации, установщик — **готово** (все 8 разделов
   макета; онбординг, окно истории, скриншот у стиля, три языка; MSI через WiX
   в ru-RU и en-US; замеры на релизе — см. `docs/STATUS.md`)

8. Правки после первого теста: свой значок и меню трея, перевод через DeepL,
   долгое нажатие, секции и порядок стилей, каталог моделей с ключа, выбор
   процессов из запущенных, тосты убраны — **готово**
9. Вторая волна правок: все чипы, удаление встроенных стилей, белый список
   моделей, кольцо без подложки, нет вспышки панели — **готово**
10. Третья волна: вкладка «Интерфейс» с акцентным цветом, регистр в панели,
   исправление раскладки с учётом выделения и возвратом каретки, хоткеи
   «Ctrl, Ctrl», README — **готово**

После каждой фазы — стоп и показ результата.

## Критерии приёмки (сокращённо)

Хоткей → оверлей < 100 мс (с захватом текста и скриншота); CPU idle < 0.5 %,
RAM < 120 МБ; клипборд восстановлен после любого сценария; API-ключ нигде,
кроме keyring; не падает при отказе любого шага; хук переживает снятие по
таймауту.
