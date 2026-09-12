# E2E фазы 6: окно настроек, кастомный стиль с быстрым хоткеем, история, трей.
# Запуск: pwsh -File scripts/e2e_settings.ps1   (нужна debug-сборка и Vite :1430;
# AI — мок, живой ключ не расходуется)
#
# Скрипт подменяет settings.json тестовым и ВСЕГДА возвращает исходный в finally
# (бэкап — с отметкой времени, чтобы повторный запуск не затёр настоящий файл).
$ErrorActionPreference = "Continue"
Add-Type -AssemblyName System.Windows.Forms, System.Drawing, UIAutomationClient, UIAutomationTypes
Add-Type -Namespace W -Name U -MemberDefinition '[DllImport("user32.dll")] public static extern bool ShowWindowAsync(IntPtr h, int c); [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h); [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out System.Drawing.Rectangle r); [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y); [DllImport("user32.dll")] public static extern void mouse_event(uint f, int dx, int dy, int d, int e);'

$root = Split-Path $PSScriptRoot -Parent
$exe = Join-Path $root "src-tauri\target\debug\restyle.exe"
$cfg = "$env:APPDATA\dev.reva1v.restyle"
$settingsPath = "$cfg\settings.json"
$histPath = "$cfg\history.json"
$backup = "$env:TEMP\restyle_settings_$(Get-Date -Format yyyyMMdd_HHmmss).json"
$out = "$env:TEMP\restyle_phase6"
New-Item -ItemType Directory -Force $out | Out-Null
$ORIGINAL = "привет это тестовый текст для фазы шесть"
$mock = $null

function Stop-Restyle { Get-Process restyle -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep -Milliseconds 400 }
function Get-NotepadWindow {
  Get-Process Notepad -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 -and $_.MainWindowTitle -like "*restyle_test*" } | Select-Object -First 1
}
function Get-NotepadEdit($np) {
  $r = [System.Windows.Automation.AutomationElement]::FromHandle($np.MainWindowHandle)
  $c = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::IsTextPatternAvailableProperty, $true)
  $r.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $c)
}
function Get-NotepadText($np) { (Get-NotepadEdit $np).GetCurrentPattern([System.Windows.Automation.TextPattern]::Pattern).DocumentRange.GetText(-1) }
function Set-NotepadText($np, $t) { (Get-NotepadEdit $np).GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($t) }
function Ensure-Notepad {
  $np = Get-NotepadWindow
  if (-not $np) {
    $f = "$env:TEMP\restyle_test.txt"
    if (-not (Test-Path $f)) { [IO.File]::WriteAllText($f, $ORIGINAL, (New-Object Text.UTF8Encoding $false)) }
    Start-Process notepad.exe $f
    for ($i = 0; $i -lt 20 -and -not $np; $i++) { Start-Sleep -Milliseconds 500; $np = Get-NotepadWindow }
  }
  if (-not $np) { throw "Блокнот с restyle_test.txt не найден" }
  $np
}
function Activate($np) {
  for ($i = 0; $i -lt 3; $i++) { [W.U]::ShowWindowAsync($np.MainWindowHandle, 9) | Out-Null; [System.Windows.Forms.SendKeys]::SendWait('%'); [W.U]::SetForegroundWindow($np.MainWindowHandle) | Out-Null; Start-Sleep -Milliseconds 250 }
  [System.Windows.Forms.SendKeys]::SendWait('{ESC}{ESC}{END}')
}
function Shot($hwnd, $path) {
  $r = New-Object System.Drawing.Rectangle
  [W.U]::GetWindowRect($hwnd, [ref]$r) | Out-Null   # Rectangle здесь = left/top/right/bottom
  $w = $r.Width - $r.X; $h = $r.Height - $r.Y
  $bmp = New-Object System.Drawing.Bitmap $w, $h
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen($r.X, $r.Y, 0, 0, (New-Object System.Drawing.Size $w, $h))
  $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
  $g.Dispose(); $bmp.Dispose()
  "  снимок: $path ($w x $h)"
}
# UIA-дерево WebView2 появляется не сразу после показа окна — ищем с повторами.
function Find-Button($root, $nameLike) {
  for ($i = 0; $i -lt 8; $i++) {
    $all = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
    $b = $all | Where-Object { $_.Current.ControlType.ProgrammaticName -eq "ControlType.Button" -and $_.Current.Name -like $nameLike } | Select-Object -First 1
    # Write-Host, а не вывод в конвейер: иначе функция вернёт строку вместе с элементом
    if ($b) { Write-Host ("жму кнопку: " + $b.Current.Name); return $b }
    Start-Sleep -Milliseconds 700
  }
  $null
}
function Find-ByClass($root, $type, $classLike) {
  $all = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
  $all | Where-Object { $_.Current.ControlType.ProgrammaticName -eq $type -and $_.Current.ClassName -like $classLike } | Select-Object -First 1
}

Stop-Restyle
if (Test-Path $settingsPath) { Copy-Item $settingsPath $backup -Force; "настройки сохранены в $backup" }

try {
  # --- Настройки как из окна: кастомный стиль с быстрым хоткеем (id придёт из
  #     имени — кириллица не даёт слаг, ждём "style"), история на диск, монитор.
  #     Хоткей стиля намеренно совпадает с основным: окно должно показать
  #     «не сработает — занято: Панель стилей» (проверка hotkey_conflicts).
  $json = @'
{
  "mainHotkey": { "ctrl": true, "alt": true, "shift": false, "win": false, "key": 82, "extraKeys": [] },
  "undoHotkey": { "ctrl": true, "alt": true, "shift": false, "win": false, "key": 90, "extraKeys": [] },
  "styles": [
    { "id": "formal", "name": "Formal", "instruction": "Rewrite formally.", "hotkey": null, "builtin": true },
    { "id": "", "name": "Эмодзи-стиль", "instruction": "Add emoji.",
      "hotkey": { "ctrl": true, "alt": true, "shift": false, "win": false, "key": 82, "extraKeys": [] }, "builtin": false }
  ],
  "model": "mock", "autostart": false, "theme": "dark", "autoPaste": true,
  "screenshotEnabled": true, "screenshotMode": "monitor",
  "screenshotExcludedProcesses": ["KeePass.exe"],
  "selectOnlyProcesses": ["Code.exe"],
  "historyToDisk": true
}
'@
  New-Item -ItemType Directory -Force $cfg | Out-Null
  [IO.File]::WriteAllText($settingsPath, "{`"settings`":$json}", (New-Object Text.UTF8Encoding $false))
  Remove-Item $histPath -ErrorAction SilentlyContinue

  # python запускаем напрямую: обёртка pwsh умирает, а сервер остаётся жить
  $mock = Start-Process python -PassThru -WindowStyle Hidden -ArgumentList "$root\scripts\mock_gemini.py", "8765"
  Start-Sleep -Milliseconds 900

  $env:RESTYLE_GEMINI_BASE_URL = "http://127.0.0.1:8765"
  $env:RESTYLE_DEMO = "4000:style"          # быстрый хоткей кастомного стиля
  $env:RESTYLE_DEMO_PASTE = "9000"
  Remove-Item Env:RESTYLE_DEMO_HIDE, Env:RESTYLE_DEMO_UNDO, Env:RESTYLE_FORCE_CLIPBOARD -ErrorAction SilentlyContinue

  $np = Ensure-Notepad
  Set-NotepadText $np $ORIGINAL
  $log = "$out\run.log"; $err = "$out\run.err"
  Start-Process -FilePath $exe -RedirectStandardOutput $log -RedirectStandardError $err
  Start-Sleep -Milliseconds 1200
  Activate $np
  $deadline = (Get-Date).AddSeconds(25)
  while ((Get-Date) -lt $deadline) { if (Select-String -Path $log, $err -Pattern "pasted via|paste failed" -Quiet) { break }; Start-Sleep -Milliseconds 300 }
  Start-Sleep -Milliseconds 1500

  "=== лог приложения (ждём combos с style:style и подменю истории)"
  Get-Content $log; Get-Content $err
  "--- текст Блокнота: " + ((Get-NotepadText $np) -replace "`r`n|`r|`n", " ⏎ ")
  "--- history.json на диске: " + (Test-Path $histPath)
  if (Test-Path $histPath) { "--- записей: " + (@(Get-Content $histPath -Raw | ConvertFrom-Json).Count) }

  # --- Окно настроек: вторая копия процесса открывает его в живой ---
  "=== окно настроек"
  Start-Process -FilePath $exe -RedirectStandardOutput "$out\second.log" -RedirectStandardError "$out\second.err"
  $w = $null
  for ($i = 0; $i -lt 24 -and -not $w; $i++) {
    Start-Sleep -Milliseconds 500
    $w = Get-Process restyle -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowTitle -like "*настройки*" } | Select-Object -First 1
  }
  if ($w) {
    [W.U]::SetForegroundWindow($w.MainWindowHandle) | Out-Null
    Start-Sleep -Milliseconds 1500
    Shot $w.MainWindowHandle "$out\settings_top.png"
    # колесо мыши идёт в окно под курсором
    $r = New-Object System.Drawing.Rectangle
    [W.U]::GetWindowRect($w.MainWindowHandle, [ref]$r) | Out-Null
    [W.U]::SetCursorPos([int](($r.X + $r.Width) / 2), [int](($r.Y + $r.Height) / 2)) | Out-Null
    Start-Sleep -Milliseconds 300
    for ($i = 0; $i -lt 25; $i++) { [W.U]::mouse_event(0x0800, 0, 0, -120, 0); Start-Sleep -Milliseconds 40 }
    Start-Sleep -Milliseconds 800
    Shot $w.MainWindowHandle "$out\settings_bottom.png"

    $uiRoot = [System.Windows.Automation.AutomationElement]::FromHandle($w.MainWindowHandle)

    # --- «В буфер» у первой записи истории ---
    Set-Clipboard -Value "MARKER-before-copy"
    $btn = Find-Button $uiRoot "*В буфер*"
    if ($btn) {
      $btn.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
      Start-Sleep -Milliseconds 1800
      "--- буфер после «В буфер» (ждём результат переписывания): " + (Get-Clipboard -Raw)
    } else {
      "!!! кнопка «В буфер» не найдена — история пуста?"
    }

    # --- Полный круг сохранения: добавить стиль из окна. Бэкенд должен выдать
    #     ему уникальный id, записать store и перезалить комбинации в хук.
    $add = Find-Button $uiRoot "*Добавить стиль*"
    if ($add) {
      $add.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
      Start-Sleep -Milliseconds 1500
      "--- стили в settings.json после кнопки «Добавить стиль»:"
      (Get-Content $settingsPath -Raw | ConvertFrom-Json).settings.styles |
        ForEach-Object { "    id={0} name={1} builtin={2}" -f $_.id, $_.name, $_.builtin }
    } else {
      "!!! кнопка «Добавить стиль» не найдена"
    }

    # --- Пауза хука не должна пережить закрытие окна: ставим поле хоткея в
    #     режим захвата и закрываем окно крестиком (React не размонтируется).
    # у поля хоткея доступное имя берётся из подписи строки, а не из текста кнопки
    $hk = Find-Button $uiRoot "*Вернуть предыдущий текст*"
    if ($hk) {
      $hk.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
      Start-Sleep -Milliseconds 800
      $win = $uiRoot.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern)
      $win.Close()
      Start-Sleep -Milliseconds 1200
    } else {
      "!!! поле хоткея не найдено"
    }
    "=== лог после закрытия окна (ждём «приостановлен» и «снова слушает»)"
    Get-Content "$out\second.log", $log -ErrorAction SilentlyContinue | Select-String "хук |combos" | ForEach-Object { "  " + $_.Line }
  } else {
    "!!! окно настроек не появилось"
  }
}
finally {
  Stop-Restyle
  if ($mock) { Stop-Process -Id $mock.Id -Force -ErrorAction SilentlyContinue }
  Get-CimInstance Win32_Process -Filter "Name='python.exe'" |
    Where-Object { $_.CommandLine -like "*mock_gemini*" } |
    ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
  Remove-Item $histPath -ErrorAction SilentlyContinue
  if (Test-Path $backup) {
    Copy-Item $backup $settingsPath -Force
    "настройки возвращены из $backup"
  } else {
    Remove-Item $settingsPath -ErrorAction SilentlyContinue
    "своего settings.json не было — тестовый удалён"
  }
}
