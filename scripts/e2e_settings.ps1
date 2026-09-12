# E2E фазы 6: окно настроек, кастомный стиль с быстрым хоткеем, история и трей.
# Запуск: pwsh -File scripts/e2e_settings.ps1   (нужна debug-сборка; AI — мок)
# Настоящий settings.json сохраняется и возвращается на место в конце.
$ErrorActionPreference = "Continue"
Add-Type -AssemblyName System.Windows.Forms, System.Drawing, UIAutomationClient, UIAutomationTypes
Add-Type -Namespace W -Name U -MemberDefinition '[DllImport("user32.dll")] public static extern bool ShowWindowAsync(IntPtr h, int c); [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h); [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out System.Drawing.Rectangle r); [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y); [DllImport("user32.dll")] public static extern void mouse_event(uint f, int dx, int dy, int d, int e);'

$root = Split-Path $PSScriptRoot -Parent
$exe = Join-Path $root "src-tauri\target\debug\restyle.exe"
$cfg = "$env:APPDATA\dev.reva1v.restyle"
$settingsPath = "$cfg\settings.json"
$backup = "$env:TEMP\restyle_settings_backup.json"
$out = "$env:TEMP\restyle_phase6"
New-Item -ItemType Directory -Force $out | Out-Null
$ORIGINAL = "привет это тестовый текст для фазы шесть"

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

Stop-Restyle
if (Test-Path $settingsPath) { Copy-Item $settingsPath $backup -Force; "настройки сохранены в $backup" }

# --- Готовим настройки как из окна: кастомный стиль с быстрым хоткеем,
#     история на диск, монитор вместо окна. Приложение должно их принять.
$json = @'
{
  "mainHotkey": { "ctrl": true, "alt": true, "shift": false, "win": false, "key": 82, "extraKeys": [] },
  "undoHotkey": { "ctrl": true, "alt": true, "shift": false, "win": false, "key": 90, "extraKeys": [] },
  "styles": [
    { "id": "formal", "name": "Formal", "instruction": "Rewrite formally.", "hotkey": null, "builtin": true },
    { "id": "", "name": "Эмодзи-стиль", "instruction": "Add emoji.",
      "hotkey": { "ctrl": true, "alt": true, "shift": true, "win": false, "key": 69, "extraKeys": [] }, "builtin": false }
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

# --- Мок Gemini ---
$mock = Start-Process pwsh -PassThru -WindowStyle Hidden -ArgumentList "-NoProfile", "-Command", "python `"$root\scripts\mock_gemini.py`" 8765"
Start-Sleep -Milliseconds 900

$env:RESTYLE_GEMINI_BASE_URL = "http://127.0.0.1:8765"
$env:RESTYLE_DEMO = "4000:style"          # быстрый стиль кастомного стиля (id = "style")
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

"=== лог приложения"
Get-Content $log; Get-Content $err
"--- текст Блокнота: " + ((Get-NotepadText $np) -replace "`r`n|`r|`n", " ⏎ ")
"--- history.json на диске: " + (Test-Path "$cfg\history.json")
if (Test-Path "$cfg\history.json") { "--- записей: " + (@(Get-Content "$cfg\history.json" -Raw | ConvertFrom-Json).Count) }

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
  Start-Sleep -Milliseconds 1200
  Shot $w.MainWindowHandle "$out\settings_top.png"
  # прокрутить вниз и снять вторую половину: колесо идёт в окно под курсором
  $r = New-Object System.Drawing.Rectangle
  [W.U]::GetWindowRect($w.MainWindowHandle, [ref]$r) | Out-Null
  [W.U]::SetCursorPos([int](($r.X + $r.Width) / 2), [int](($r.Y + $r.Height) / 2)) | Out-Null
  Start-Sleep -Milliseconds 300
  for ($i = 0; $i -lt 25; $i++) { [W.U]::mouse_event(0x0800, 0, 0, -120, 0); Start-Sleep -Milliseconds 40 }
  Start-Sleep -Milliseconds 800
  Shot $w.MainWindowHandle "$out\settings_bottom.png"
} else {
  "!!! окно настроек не появилось"
}
"--- что записано в settings.json (нормализовано приложением):"
Get-Content $settingsPath -Raw

Stop-Restyle
if ($mock) { Stop-Process -Id $mock.Id -Force -ErrorAction SilentlyContinue }
Get-Process python -ErrorAction SilentlyContinue | Where-Object { $_.CommandLine -like "*mock_gemini*" } | Stop-Process -Force -ErrorAction SilentlyContinue
if (Test-Path $backup) { Copy-Item $backup $settingsPath -Force; "настройки возвращены" }
Remove-Item "$cfg\history.json" -ErrorAction SilentlyContinue
