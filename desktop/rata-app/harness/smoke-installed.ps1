# smoke-installed.ps1 - install the Windows installer this runner just built
# the way a customer would, launch RATA with no licence and no mailbox, and
# check that it stays up and opens its window under the right title. Run by
# release.yml after the signatures are checked and before anything is kept;
# Linux and macOS have smoke-installed.sh.
#
#   pwsh ./harness/smoke-installed.ps1 -Version 0.1.38 -Expect 'RATA 0.1.38 beta'
#
# The same for a signed and an unsigned build: it runs whatever the bundler
# left in bundle/nsis, and a locally built file carries no mark of the web,
# so SmartScreen has nothing to say either way.
param(
  [Parameter(Mandatory)] [string] $Version,
  [Parameter(Mandatory)] [string] $Expect,
  [string] $Bundle = 'src-tauri/target',
  [int] $Up = 15,     # seconds the process must stay alive
  [int] $Wait = 20    # seconds to wait for the window to appear
)
$ErrorActionPreference = 'Stop'
$failed = $false
function Fail([string] $m) { Write-Output "::error::$m"; $script:failed = $true }

# The one installer of this version. A cached target/ could hold an older
# version's, so the version is part of the name looked for.
$setups = @(Get-ChildItem -Recurse -File -Path $Bundle -Filter "RATA_${Version}_*-setup.exe" -ErrorAction SilentlyContinue |
  Where-Object { $_.FullName -match '[\\/]release[\\/]bundle[\\/]nsis[\\/]' })
if ($setups.Count -ne 1) {
  Write-Output "::error::Expected one RATA_${Version}_*-setup.exe under $Bundle/**/release/bundle/nsis, found $($setups.Count): $($setups.FullName -join ', ')"
  exit 1
}
$setup = $setups[0].FullName

# /S is NSIS's silent install: no pages, the default per-user location
# (%LOCALAPPDATA%\RATA), and no launch at the end (that needs /R too).
Write-Output "Installing $setup silently."
$install = Start-Process -FilePath $setup -ArgumentList '/S' -Wait -PassThru
if ($install.ExitCode -ne 0) { Write-Output "::error::The installer exited with $($install.ExitCode)."; exit 1 }

# Where it went, as the installer recorded it for Add or remove programs,
# which is also what an update and an uninstall go by.
$dir = $null
foreach ($hive in 'HKCU:', 'HKLM:') {
  $key = "$hive\Software\Microsoft\Windows\CurrentVersion\Uninstall\RATA"
  $loc = (Get-ItemProperty -Path $key -Name InstallLocation -ErrorAction SilentlyContinue).InstallLocation
  if ($loc) {
    $dir = $loc.Trim('"')
    Write-Output "Registered in $key as $((Get-ItemProperty -Path $key).DisplayName) $((Get-ItemProperty -Path $key).DisplayVersion), at $dir"
    break
  }
}
if (-not $dir) { Fail "The installer registered no uninstall entry for RATA."; $dir = Join-Path $env:LOCALAPPDATA 'RATA' }
$exe = Join-Path $dir 'rata-app.exe'
if (-not (Test-Path $exe)) { Write-Output "::error::No rata-app.exe where RATA was installed ($dir)."; exit 1 }

# The app keeps its mailbox list under %APPDATA%\org.mailrata.desktop and
# makes that folder itself on the first write; %APPDATA% has to exist.
if (-not $env:APPDATA -or -not (Test-Path $env:APPDATA)) { Fail "This runner user has no %APPDATA% folder." }

# stdout and stderr to files: the program has no console (windows_subsystem),
# but a panic still writes to stderr.
$work = Join-Path $env:RUNNER_TEMP 'rata-smoke'
New-Item -ItemType Directory -Force -Path $work | Out-Null
$out = Join-Path $work 'out.log'; $err = Join-Path $work 'err.log'
$app = Start-Process -FilePath $exe -PassThru -RedirectStandardOutput $out -RedirectStandardError $err
# Held now, or .NET forgets the exit code of a process that has already gone.
$null = $app.Handle
$clock = [Diagnostics.Stopwatch]::StartNew()

# The title of the process's main window. The window is built in setup
# (main.rs), and the webview draws inside it from msedgewebview2.exe, which
# owns no top-level window of its own.
$title = ''
while ($clock.Elapsed.TotalSeconds -lt $Wait -and -not $app.HasExited) {
  $app.Refresh()
  if ($app.MainWindowTitle) { $title = $app.MainWindowTitle; break }
  Start-Sleep -Milliseconds 500
}
while ($clock.Elapsed.TotalSeconds -lt $Up -and -not $app.HasExited) { Start-Sleep -Milliseconds 500 }

if ($app.HasExited) {
  Fail "RATA exited within $Up s of starting (exit $($app.ExitCode))."
} else {
  Write-Output ("RATA is still running after {0:N0} s." -f $clock.Elapsed.TotalSeconds)
}
if (-not $title) {
  $shown = Get-Process | Where-Object MainWindowTitle | ForEach-Object { "$($_.ProcessName): $($_.MainWindowTitle)" }
  Fail "RATA showed no window with a title within $Wait s. Windows with titles on this runner: $($shown -join '; ')"
} elseif ($title -ne $Expect) {
  Fail "RATA's window is titled `"$title`", not `"$Expect`"."
} else {
  Write-Output "RATA's window: `"$title`"."
}

if (-not $app.HasExited) { Stop-Process -Id $app.Id -Force -ErrorAction SilentlyContinue; $app.WaitForExit(10000) | Out-Null }
# WebView2 children go with it, or a later step finds files still in use.
Get-Process msedgewebview2 -ErrorAction SilentlyContinue |
  Where-Object { $_.Path -and $_.CommandLine -match 'org\.mailrata\.desktop' } |
  Stop-Process -Force -ErrorAction SilentlyContinue

foreach ($log in $out, $err) {
  Write-Output "::group::$(Split-Path -Leaf $log)"
  if (Test-Path $log) { Get-Content $log }
  Write-Output "::endgroup::"
}
$text = (@($out, $err) | Where-Object { Test-Path $_ } | ForEach-Object { Get-Content -Raw $_ }) -join "`n"
if ($text -match 'panicked at|RATA could not start') { Fail "RATA wrote a panic to its log." }

if ($failed) { Write-Output "The installed app failed its launch check."; exit 1 }
Write-Output "The installed app starts and shows `"$Expect`"."
exit 0
