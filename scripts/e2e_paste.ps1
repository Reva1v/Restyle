# E2E фазы 5: вставка/undo/Esc в Блокноте без клавиатуры (демо-переменные).
# Запуск: pwsh -File scripts/e2e_paste.ps1   (нужен Vite на :1430 и debug-сборка)
param([string[]]$Cases = @("p_esc", "p_uia", "p_undo", "p_clip", "p_early"))
$Cases = @($Cases | ForEach-Object { $_ -split ',' } | ForEach-Object { $_.Trim() }) # pwsh -File отдаёт список одной строкой
$ErrorActionPreference = "Continue"
Add-Type -Namespace W -Name U -MemberDefinition '[DllImport("user32.dll")] public static extern bool ShowWindowAsync(IntPtr h, int c); [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);'
Add-Type -AssemblyName System.Windows.Forms, System.Drawing, UIAutomationClient, UIAutomationTypes
$exe = "C:\Users\Reva1v\WebstormProjects\Restyle\src-tauri\target\debug\restyle.exe"
$ORIGINAL = "Привет, это тестовый текст для Restyle.`r`nВторая строка с числами 42 и ссылкой https://example.com"

function Get-NotepadWindow {
  Get-Process Notepad -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 -and $_.MainWindowTitle -like "*restyle_test*" } | Select-Object -First 1
}
function Get-NotepadEdit($np) {
  $root = [System.Windows.Automation.AutomationElement]::FromHandle($np.MainWindowHandle)
  $cond = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::IsTextPatternAvailableProperty, $true)
  $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $cond)
}
function Get-NotepadText($np) {
  $el = Get-NotepadEdit $np
  if (-not $el) { return "<no edit element>" }
  $tp = $el.GetCurrentPattern([System.Windows.Automation.TextPattern]::Pattern)
  $tp.DocumentRange.GetText(-1)
}
function Set-NotepadText($np, $text) {
  $el = Get-NotepadEdit $np
  $vp = $el.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern)
  $vp.SetValue($text)
}
function Activate($np) {
  for ($i = 0; $i -lt 4; $i++) { [W.U]::ShowWindowAsync($np.MainWindowHandle, 9) | Out-Null; [System.Windows.Forms.SendKeys]::SendWait('%'); $ok = [W.U]::SetForegroundWindow($np.MainWindowHandle); Start-Sleep -Milliseconds 300 }
  [System.Windows.Forms.SendKeys]::SendWait('{ESC}{ESC}{END}')
  $ok
}
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

function Run-Case($name, $envs, $waitFor, $seconds) {
  if ($Cases -notcontains $name) { return }
  $np = Ensure-Notepad
  Set-NotepadText $np $ORIGINAL
  Set-Clipboard -Value "MARKER-$name"
  $log = "$env:TEMP\restyle_$name.log"; $err = "$env:TEMP\restyle_$name.err"; Clear-Content $log, $err -ErrorAction SilentlyContinue
  foreach ($k in "RESTYLE_DEMO", "RESTYLE_DEMO_HIDE", "RESTYLE_DEMO_PASTE", "RESTYLE_DEMO_UNDO", "RESTYLE_FORCE_CLIPBOARD", "RESTYLE_GEMINI_BASE_URL") { Remove-Item "Env:$k" -ErrorAction SilentlyContinue }
  foreach ($kv in $envs.GetEnumerator()) { Set-Item "Env:$($kv.Key)" $kv.Value }
  Start-Process -FilePath $exe -RedirectStandardOutput $log -RedirectStandardError $err
  Start-Sleep -Milliseconds 1000
  $ok = Activate $np
  $deadline = (Get-Date).AddSeconds($seconds)
  while ((Get-Date) -lt $deadline) { if (Select-String -Path $log, $err -Pattern $waitFor -Quiet) { break }; Start-Sleep -Milliseconds 300 }
  Start-Sleep -Milliseconds 1200
  "=== $name (activated=$ok)"
  Get-Content $log; Get-Content $err
  "--- notepad text after: " + ((Get-NotepadText $np) -replace "`r`n|`r|`n", " ⏎ ")
  "--- clipboard after: " + (Get-Clipboard -Raw)
  Get-Process restyle -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep -Milliseconds 500
}

Run-Case "p_esc"   @{ RESTYLE_DEMO = "4000:formal"; RESTYLE_DEMO_HIDE = "6000" } "generation aborted|overlay:hide|rewrite done" 20
Run-Case "p_uia"   @{ RESTYLE_DEMO = "4000:formal"; RESTYLE_DEMO_PASTE = "9000" } "pasted via|paste failed|Не удалось" 25
Run-Case "p_undo"  @{ RESTYLE_DEMO = "4000:formal"; RESTYLE_DEMO_PASTE = "9000"; RESTYLE_DEMO_UNDO = "3000" } "undo:|undo failed" 30
Run-Case "p_clip"  @{ RESTYLE_DEMO = "4000:formal"; RESTYLE_DEMO_PASTE = "9000"; RESTYLE_FORCE_CLIPBOARD = "1" } "pasted via|paste failed" 25
Run-Case "p_early" @{ RESTYLE_DEMO = "4000:formal"; RESTYLE_DEMO_PASTE = "300" } "pasted via|paste failed" 25
"--- history file exists (should be False): " + (Test-Path "$env:APPDATA\dev.reva1v.restyle\history.json")
