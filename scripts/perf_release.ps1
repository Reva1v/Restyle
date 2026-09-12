# Замеры критериев приёмки на РЕЛИЗНОЙ сборке (фазы 7–8).
# Запуск: pwsh -File scripts/perf_release.ps1
# Свой settings.json сохраняется и возвращается в finally.
#
# Меряем: задержку «хоткей → отрисованный оверлей» (лог приложения),
# потребление памяти и CPU в простое.
$ErrorActionPreference = "Continue"
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type -Namespace P -Name U -MemberDefinition @'
[DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
[DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
[DllImport("user32.dll")] public static extern bool ShowWindowAsync(IntPtr h, int c);
[DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
[DllImport("user32.dll")] public static extern void mouse_event(uint f, uint x, uint y, uint d, System.UIntPtr e);
'@
$root = Split-Path $PSScriptRoot -Parent
$exe = Join-Path $root "src-tauri\target\release\restyle.exe"
if (-not (Test-Path $exe)) { throw "нет релизной сборки: $exe (pnpm tauri build)" }
$cfg = Join-Path $env:APPDATA "dev.reva1v.restyle"
$settingsPath = Join-Path $cfg "settings.json"
$backup = Join-Path $env:TEMP "restyle_settings_perf_$(Get-Date -Format yyyyMMdd_HHmmss).json"
$out = Join-Path $env:TEMP "restyle_perf"
New-Item -ItemType Directory -Force $out | Out-Null
$TEXT = "слушай я тут посчитал, разбивку по часам сделаю но не до пятницы а до вторника, надо ещё с бухгалтерией свериться"
$mock = $null
$target = $null

Get-Process restyle -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 400
if (Test-Path $settingsPath) { Copy-Item $settingsPath $backup -Force }

try {
  $json = @"
{"settings":{"mainHotkey":{"ctrl":true,"alt":true,"shift":false,"win":false,"key":82,"extraKeys":[]},
"undoHotkey":{"ctrl":true,"alt":true,"shift":false,"win":false,"key":90,"extraKeys":[]},
"model":"mock","autostart":false,"theme":"dark","language":"ru","onboarded":true,
"screenshotEnabled":true,"screenshotMode":"window","historyToDisk":false}}
"@
  New-Item -ItemType Directory -Force $cfg | Out-Null
  [IO.File]::WriteAllText($settingsPath, $json, (New-Object Text.UTF8Encoding $false))

  $mock = Start-Process python -PassThru -WindowStyle Hidden -ArgumentList (Join-Path $root "scripts\mock_gemini.py"), "8765"
  Start-Sleep -Milliseconds 900

  # окно-источник текста
  [IO.File]::WriteAllText((Join-Path $env:TEMP "restyle_shot_source.txt"), $TEXT, (New-Object Text.UTF8Encoding $false))
  $tf = Join-Path $env:TEMP "restyle_textfield.ps1"
  @'
$Text = [IO.File]::ReadAllText((Join-Path $env:TEMP "restyle_shot_source.txt"))
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
$f = New-Object System.Windows.Forms.Form
$f.Text = "Restyle perf target"; $f.StartPosition = "Manual"
$f.Location = New-Object System.Drawing.Point(120, 120)
$f.Size = New-Object System.Drawing.Size(1100, 700)
$t = New-Object System.Windows.Forms.TextBox
$t.Multiline = $true; $t.Dock = "Fill"; $t.Text = $Text
$f.Controls.Add($t)
$f.Add_Shown({ $f.Activate(); $t.Focus() })
[System.Windows.Forms.Application]::Run($f)
'@ | Set-Content -Path $tf -Encoding UTF8
  $target = Start-Process pwsh -PassThru -ArgumentList "-NoProfile", "-File", $tf
  $tw = $null
  for ($i = 0; $i -lt 25 -and -not $tw; $i++) {
    Start-Sleep -Milliseconds 400
    $p = Get-Process -Id $target.Id -ErrorAction SilentlyContinue
    if ($p -and $p.MainWindowHandle -ne 0) { $tw = $p.MainWindowHandle }
  }

  $env:RESTYLE_GEMINI_BASE_URL = "http://127.0.0.1:8765"
  $log = Join-Path $out "perf.log"
  $err = Join-Path $out "perf.err"

  # --- прогон 1: задержка показа, три запуска подряд ---
  "=== задержка «хоткей → отрисованный оверлей» (релиз, 3 запуска)"
  for ($run = 1; $run -le 3; $run++) {
    $env:RESTYLE_DEMO = "2500"
    $env:RESTYLE_DEMO_HIDE = "1200"
    $runLog = Join-Path $out "perf_$run.log"
    Start-Process -FilePath $exe -RedirectStandardOutput $runLog -RedirectStandardError $err
    Start-Sleep -Milliseconds 1200
    # без фокуса на поле захват уходит в таймаут и меряется не то:
    # оверлей показывается с «capturing», и задержка вырастает до порога
    $ok = $false
    for ($f = 0; $f -lt 10 -and -not $ok; $f++) {
      [P.U]::ShowWindowAsync($tw, 9) | Out-Null
      [System.Windows.Forms.SendKeys]::SendWait('%')
      [P.U]::SetForegroundWindow($tw) | Out-Null
      Start-Sleep -Milliseconds 150
      $ok = ([P.U]::GetForegroundWindow() -eq $tw)
    }
    if (-not $ok) { "  $run| !!! окно не в фокусе — замер недостоверен" }
    # Alt из SendKeys уводит клавиатурный фокус на саму форму, и UIA видит
    # не поле, а окно («нет текста»). Возвращаем фокус кликом внутрь поля.
    [P.U]::SetCursorPos(700, 320) | Out-Null
    Start-Sleep -Milliseconds 120
    [P.U]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
    [P.U]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 200
    Start-Sleep -Seconds 6
    Select-String -Path $runLog -Pattern "show latency|captured \d|screenshot \d" | ForEach-Object { "  $run| " + $_.Line }
    if ($run -lt 3) { Get-Process restyle -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep -Milliseconds 600 }
  }
  Remove-Item Env:RESTYLE_DEMO, Env:RESTYLE_DEMO_HIDE -ErrorAction SilentlyContinue

  # --- прогон 2: простой (CPU/RAM) ---
  # Считаем всё дерево: вебвью живут в отдельных msedgewebview2.exe
  # (с фазы 8 их два — оверлей и меню трея), и пользователь видит в
  # диспетчере именно группу процессов.
  function AppTree() {
    $all = Get-CimInstance Win32_Process | Select-Object ProcessId, ParentProcessId, Name
    $tree = @{}
    $queue = @(Get-Process restyle -ErrorAction SilentlyContinue | ForEach-Object { [int]$_.Id })
    while ($queue.Count -gt 0) {
      # ключи строго [int]: Get-Process даёт Int32, CIM — UInt32,
      # и хеш-таблица считает такие ключи разными
      $id = [int]$queue[0]; $queue = @($queue | Select-Object -Skip 1)
      if ($tree.ContainsKey($id)) { continue }
      $p = $all | Where-Object { [int]$_.ProcessId -eq $id }
      if (-not $p) { continue }
      $tree[$id] = $p
      $queue += @($all | Where-Object { [int]$_.ParentProcessId -eq $id } | ForEach-Object { [int]$_.ProcessId })
    }
    $tree
  }
  function TreeCpu($tree) {
    $sum = [TimeSpan]::Zero
    foreach ($id in $tree.Keys) {
      $p = Get-Process -Id $id -ErrorAction SilentlyContinue
      if ($p) { $sum += $p.TotalProcessorTime }
    }
    $sum
  }

  $tree = AppTree
  if ($tree.Count -gt 0) {
    $cpu0 = TreeCpu $tree
    $t0 = Get-Date
    Start-Sleep -Seconds 20
    $cpu1 = TreeCpu $tree
    $span = ((Get-Date) - $t0).TotalSeconds
    $cores = [Environment]::ProcessorCount
    $pct = (($cpu1 - $cpu0).TotalSeconds / $span / $cores) * 100
    "=== простой 20 с (всё дерево процессов)"
    "  CPU дерева: {0:N2} % (норма < 0,5)" -f $pct
    $ws = 0.0
    foreach ($id in $tree.Keys) {
      $p = Get-Process -Id $id -ErrorAction SilentlyContinue
      if ($p) { $ws += $p.WorkingSet64 }
    }
    $raw = Get-CimInstance Win32_PerfRawData_PerfProc_Process | Select-Object IDProcess, WorkingSetPrivate
    $priv = 0.0
    foreach ($r in $raw) { if ($tree.ContainsKey([int]$r.IDProcess)) { $priv += [double]$r.WorkingSetPrivate } }
    $names = $tree.Values | Group-Object Name | ForEach-Object { "{0} x{1}" -f $_.Name, $_.Count }
    "  процессы: " + ($names -join ", ")
    "  RAM приватная (реальная цена): {0:N0} МБ (норма < 120)" -f ($priv / 1MB)
    "  RAM рабочий набор: {0:N0} МБ — общие страницы вебвью считаются в каждом процессе" -f ($ws / 1MB)
    $main = Get-Process restyle -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($main) { "  из них сам restyle.exe: {0:N0} МБ" -f ($main.WorkingSet64 / 1MB) }
  } else {
    "!!! процесс не найден"
  }
}
finally {
  Get-Process restyle -ErrorAction SilentlyContinue | Stop-Process -Force
  if ($target) { Stop-Process -Id $target.Id -Force -ErrorAction SilentlyContinue }
  if ($mock) { Stop-Process -Id $mock.Id -Force -ErrorAction SilentlyContinue }
  Get-CimInstance Win32_Process -Filter "Name='python.exe'" |
    Where-Object { $_.CommandLine -like "*mock_gemini*" } |
    ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
  if (Test-Path $backup) { Copy-Item $backup $settingsPath -Force; "настройки возвращены" }
  else { Remove-Item $settingsPath -ErrorAction SilentlyContinue }
}
