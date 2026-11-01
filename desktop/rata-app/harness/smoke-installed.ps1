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
  [int] $Wait = 20,   # seconds to wait for the window to appear
  [int] $Again = 15   # seconds a second launch has to hand over and exit
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

# The app keeps its mailbox list under %LOCALAPPDATA%\org.mailrata.desktop
# (since 0.1.52; review R13), moving one an older build left under
# %APPDATA%\org.mailrata.desktop, and makes its folder itself on the first
# write; both have to exist.
if (-not $env:APPDATA -or -not (Test-Path $env:APPDATA)) { Fail "This runner user has no %APPDATA% folder." }
if (-not $env:LOCALAPPDATA -or -not (Test-Path $env:LOCALAPPDATA)) { Fail "This runner user has no %LOCALAPPDATA% folder." }

# stdout and stderr to files: the program has no console (windows_subsystem),
# but a panic still writes to stderr.
$work = Join-Path $env:RUNNER_TEMP 'rata-smoke'
New-Item -ItemType Directory -Force -Path $work | Out-Null
$out = Join-Path $work 'out.log'; $err = Join-Path $work 'err.log'
$out2 = Join-Path $work 'again-out.log'; $err2 = Join-Path $work 'again-err.log'
$app = Start-Process -FilePath $exe -PassThru -RedirectStandardOutput $out -RedirectStandardError $err
# Held now, or .NET forgets the exit code of a process that has already gone.
$null = $app.Handle
$clock = [Diagnostics.Stopwatch]::StartNew()

# The title of the process's main window. The window is built in setup
# (lib.rs), and the webview draws inside it from msedgewebview2.exe, which
# owns no top-level window of its own. Single instance's own window,
# "org.mailrata.desktop-siw", is made earlier, in the plugin's setup, unowned
# and marked visible so that it gets messages; .NET can take it for the main
# window until RATA's is up. So only a title beginning RATA ends the wait,
# as on Linux, and any other is shown if nothing better comes.
$title = ''
$other = ''
while ($clock.Elapsed.TotalSeconds -lt $Wait -and -not $app.HasExited) {
  $app.Refresh()
  $now = $app.MainWindowTitle
  if ($now -like 'RATA*') { $title = $now; break }
  if ($now) { $other = $now }
  Start-Sleep -Milliseconds 500
}
while ($clock.Elapsed.TotalSeconds -lt $Up -and -not $app.HasExited) { Start-Sleep -Milliseconds 500 }

if ($app.HasExited) {
  Fail "RATA exited within $Up s of starting (exit $($app.ExitCode))."
} else {
  Write-Output ("RATA is still running after {0:N0} s." -f $clock.Elapsed.TotalSeconds)
}
if (-not $title) {
  if ($other) { Write-Output "The last title read from RATA's process was `"$other`"." }
  $shown = Get-Process | Where-Object MainWindowTitle | ForEach-Object { "$($_.ProcessName): $($_.MainWindowTitle)" }
  Fail "RATA showed no window titled RATA... within $Wait s. Windows with titles on this runner: $($shown -join '; ')"
} elseif ($title -ne $Expect) {
  Fail "RATA's window is titled `"$title`", not `"$Expect`"."
} else {
  Write-Output "RATA's window: `"$title`"."
}

# Single instance (lib.rs, review R5): RATA started again while it is open
# finds the first (a named mutex and a hidden window, per sign-in session),
# hands over to it and exits 0 at once, and the first stays up; the list's
# own lock stops any copy that gets past that. A second copy still running
# is two RATAs writing one mailbox list.
if (-not $app.HasExited) {
  $second = Start-Process -FilePath $exe -PassThru -RedirectStandardOutput $out2 -RedirectStandardError $err2
  $null = $second.Handle
  if (-not $second.WaitForExit($Again * 1000)) {
    Fail "RATA, launched again, was still running $Again s later beside the first: single instance did not stop it."
    Stop-Process -Id $second.Id -Force -ErrorAction SilentlyContinue
    $second.WaitForExit(10000) | Out-Null
  } elseif ($second.ExitCode -ne 0) {
    Fail "RATA, launched again, exited $($second.ExitCode), not 0."
  } else {
    Write-Output "RATA, launched again, handed over to the first and exited 0."
  }
  if ($app.HasExited) { Fail "RATA exited when it was launched again (exit $($app.ExitCode))." }
}

if (-not $app.HasExited) { Stop-Process -Id $app.Id -Force -ErrorAction SilentlyContinue; $app.WaitForExit(10000) | Out-Null }
# WebView2 children go with it, or a later step finds files still in use.
Get-Process msedgewebview2 -ErrorAction SilentlyContinue |
  Where-Object { $_.Path -and $_.CommandLine -match 'org\.mailrata\.desktop' } |
  Stop-Process -Force -ErrorAction SilentlyContinue

foreach ($log in $out, $err, $out2, $err2) {
  Write-Output "::group::$(Split-Path -Leaf $log)"
  if (Test-Path $log) { Get-Content $log }
  Write-Output "::endgroup::"
}
$text = (@($out, $err, $out2, $err2) | Where-Object { Test-Path $_ } | ForEach-Object { Get-Content -Raw $_ }) -join "`n"
if ($text -match 'panicked at|RATA could not start') { Fail "RATA wrote a panic to its log." }

if ($failed) { Write-Output "The installed app failed its launch check."; exit 1 }
Write-Output "The installed app starts and shows `"$Expect`"."
exit 0
