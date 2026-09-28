#!/bin/bash
# smoke-installed.sh — install the installers this runner just built, the way
# a customer would, launch each one with no licence and no mailbox, and check
# that it stays up and opens its window under the right title. Run by
# release.yml after the build and before anything is kept; the Windows half is
# smoke-installed.ps1.
#
#   VERSION=0.1.38 EXPECT_TITLE="RATA 0.1.38 beta" BUNDLE=src-tauri/target \
#     ./harness/smoke-installed.sh
#
# Linux: installs the .deb with apt (sudo), launches /usr/bin/rata-app under
# Xvfb, then does the same with the AppImage (extracted and run, since the
# runner has no FUSE). Needs Xvfb and xdotool. macOS: mounts the .dmg, copies
# RATA.app out, runs the program inside it, and reads the title through
# System Events when the runner allows that (see mac_title).
#
# "Stays up" is the check that matters: the window is built in the app's
# setup (main.rs), and a setup that fails ends the process, so a process
# alive after UP seconds has built its window. The log is kept and failed on
# a panic.
set -u

VERSION="${VERSION:?set VERSION to the version being released, as in tauri.conf.json}"
EXPECT="${EXPECT_TITLE:?set EXPECT_TITLE to the window title the build must show}"
BUNDLE="${BUNDLE:-src-tauri/target}"
UP="${UP:-15}"          # seconds the process must stay alive
WAIT="${WAIT:-20}"      # seconds to wait for the window to appear
WORK="$(mktemp -d "${RUNNER_TEMP:-/tmp}/rata-smoke.XXXXXX")"
FAILED=0

say() { printf '%s\n' "$*"; }
fail() { say "::error::$*"; FAILED=1; }

# The one installer of this version and kind in the bundle directory, as an
# absolute path in FOUND (apt takes a file only as a path). A cached target/
# could hold an older version's, so the version is part of the name looked
# for, and two matches are a question rather than a guess.
one() {
  local hits
  FOUND=""
  hits=$(find "$BUNDLE" -path "*/release/bundle/$1/*" -name "$2" -type f 2>/dev/null)
  if [ -z "$hits" ]; then fail "No $2 under $BUNDLE/**/release/bundle/$1."; return 1; fi
  if [ "$(printf '%s\n' "$hits" | wc -l)" -ne 1 ]; then fail "More than one $2: $(printf '%s ' "$hits" | tr '\n' ' ')"; return 1; fi
  FOUND="$(cd "$(dirname "$hits")" && pwd)/$(basename "$hits")"
}

# A panic is the Rust side giving up; "could not start" is main.rs's own
# last words. Anything else in the log (WebKit's chatter about GPUs under
# Xvfb, say) is shown but is not a failure.
clean_log() {
  say "::group::Log of $1"; cat "$2"; say "::endgroup::"
  if grep -qE "panicked at|RATA could not start" "$2"; then
    fail "$1 wrote a panic to its log:"; grep -E -A3 "panicked at|RATA could not start" "$2" | head -20
  fi
}

# ---------------------------------------------------------------- Linux

# Wait for a window titled exactly $EXPECT from process $1, keep it running
# until it has been up UP seconds, and report. xdotool reads _NET_WM_NAME
# through Xlib, so this is the same title a tester reads off the title bar.
linux_watch() {
  local what="$1" pid="$2" log="$3" start=$SECONDS title="" wid="" ids w n h geo
  while [ $((SECONDS - start)) -lt "$WAIT" ]; do
    kill -0 "$pid" 2>/dev/null || break
    ids=$(DISPLAY=:99 xdotool search --onlyvisible --name '.' 2>/dev/null || true)
    for w in $ids; do
      n=$(DISPLAY=:99 xdotool getwindowname "$w" 2>/dev/null || true)
      case "$n" in RATA*) title="$n"; wid="$w"; break ;; esac
    done
    [ -n "$title" ] && break
    sleep 1
  done
  while kill -0 "$pid" 2>/dev/null && [ $((SECONDS - start)) -lt "$UP" ]; do sleep 1; done
  if ! kill -0 "$pid" 2>/dev/null; then
    wait "$pid"; fail "$what exited within ${UP} s of starting (exit $?)."
  else
    say "$what is still running after $((SECONDS - start)) s."
  fi
  if [ -z "$title" ]; then fail "$what showed no window named RATA… within ${WAIT} s."
  elif [ "$title" != "$EXPECT" ]; then fail "$what's window is titled \"$title\", not \"$EXPECT\"."
  else say "$what's window: \"$title\"."
    # fit_to_screen (main.rs): the screen here is a 1366×768 laptop's, and
    # the configured 1280×860 must have been shrunk to fit it.
    geo=$(DISPLAY=:99 xdotool getwindowgeometry --shell "$wid" 2>/dev/null | grep -E '^(WIDTH|HEIGHT)=' | tr '\n' ' ')
    say "$what's window size: ${geo:-unknown}"
    case "$geo" in *HEIGHT=*) h=${geo##*HEIGHT=}; h=${h%% *}; [ "$h" -le 768 ] || fail "$what's window is ${h} px tall on a 768 px screen: fit_to_screen did not shrink it." ;; esac
  fi
  kill "$pid" 2>/dev/null; sleep 1; kill -9 "$pid" 2>/dev/null; wait "$pid" 2>/dev/null
  clean_log "$what" "$log"
}

linux() {
  local deb img
  # A fresh home for each launch: a first launch, with no app data directory
  # yet (the app makes its own) and no ~/.config/user-dirs.dirs. Without that
  # file Tauri has no Downloads folder, which matters to saving
  # (commands::downloads) and must not matter to starting.
  fresh_home() {
    export HOME="$WORK/home-$1"; mkdir -p "$HOME"
    export XDG_RUNTIME_DIR="$WORK/run-$1"; mkdir -p "$XDG_RUNTIME_DIR"; chmod 700 "$XDG_RUNTIME_DIR"
    unset XDG_CONFIG_HOME XDG_DATA_HOME XDG_CACHE_HOME
  }

  Xvfb :99 -screen 0 1366x768x24 -nolisten tcp >"$WORK/xvfb.log" 2>&1 &
  local xvfb=$!
  sleep 2
  if ! kill -0 "$xvfb" 2>/dev/null; then fail "Xvfb did not start."; cat "$WORK/xvfb.log"; return; fi

  if one deb "RATA_${VERSION}_*.deb"; then
    deb="$FOUND"
    say "Installing $deb"
    if sudo apt-get install -y --no-install-recommends "$deb"; then
      if [ "$(command -v rata-app)" != /usr/bin/rata-app ] || ! dpkg -S /usr/bin/rata-app >/dev/null 2>&1; then
        fail "Installing the .deb did not put rata-app at /usr/bin/rata-app."
      else
        say "$(dpkg -s rata 2>/dev/null | grep -E '^(Package|Version):' | tr '\n' ' ')installed as $(dpkg -S /usr/bin/rata-app)"
        ( fresh_home deb; DISPLAY=:99 exec /usr/bin/rata-app >"$WORK/deb.log" 2>&1 ) &
        linux_watch "The installed .deb" $! "$WORK/deb.log"
      fi
    else
      fail "apt could not install $deb."
    fi
  fi

  if one appimage "RATA_${VERSION}_*.AppImage"; then
    img="$FOUND"
    cp "$img" "$WORK/RATA.AppImage"; chmod +x "$WORK/RATA.AppImage"
    ( fresh_home appimage; cd "$WORK" && APPIMAGE_EXTRACT_AND_RUN=1 DISPLAY=:99 exec ./RATA.AppImage >"$WORK/appimage.log" 2>&1 ) &
    linux_watch "The AppImage" $! "$WORK/appimage.log"
  fi

  kill "$xvfb" 2>/dev/null
}

# ---------------------------------------------------------------- macOS

# The window's title as System Events sees it, or nothing. Reading another
# process's windows needs the Accessibility permission, and asking System
# Events anything may need Automation; a hosted runner may grant neither, and
# a permission prompt on a machine with nobody at it waits forever. So it is
# given 20 s and its failure is reported, not fatal: what a Mac runner can
# always assert is that the process lives (its window is built in setup)
# and that its log is clean.
mac_title() {
  local pid="$1" out="$WORK/osascript.out" t
  osascript -e "tell application \"System Events\" to get name of every window of (first process whose unix id is $pid)" >"$out" 2>&1 &
  t=$!
  for _ in $(seq 20); do kill -0 "$t" 2>/dev/null || break; sleep 1; done
  if kill -0 "$t" 2>/dev/null; then kill "$t" 2>/dev/null; wait "$t" 2>/dev/null; echo "osascript timed out (waiting on a permission prompt?)" >"$out"; return 1; fi
  wait "$t" || return 1
  cat "$out"
}

mac() {
  local dmg mnt app exe pid start title
  one dmg "RATA_${VERSION}_*.dmg" || return
  dmg="$FOUND"
  mnt="$WORK/mnt"; mkdir -p "$mnt"
  say "Mounting $dmg"
  if ! hdiutil attach -nobrowse -noautoopen -readonly -mountpoint "$mnt" "$dmg"; then fail "hdiutil could not mount $dmg."; return; fi
  # Copied out as a customer drags it to Applications; ditto keeps the
  # bundle's signature, links and extended attributes intact.
  mkdir -p "$HOME/Applications"
  app="$HOME/Applications/RATA.app"; rm -rf "$app"
  if [ -d "$mnt/RATA.app" ]; then ditto "$mnt/RATA.app" "$app"; else fail "No RATA.app in $dmg:"; ls -la "$mnt"; fi
  hdiutil detach "$mnt" -quiet || hdiutil detach "$mnt" -force -quiet
  [ -d "$app" ] || return

  exe="$app/Contents/MacOS/$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$app/Contents/Info.plist")"
  say "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$app/Contents/Info.plist") $(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$app/Contents/Info.plist"), $(lipo -archs "$exe"), on a $(uname -m) runner"
  # The Intel build runs on an Apple silicon runner through Rosetta.
  if [ "$(uname -m)" = arm64 ] && ! lipo -archs "$exe" | grep -q arm64 && ! arch -x86_64 /usr/bin/true 2>/dev/null; then
    say "Installing Rosetta to run the Intel build."
    sudo softwareupdate --install-rosetta --agree-to-license || fail "Rosetta could not be installed, so the Intel build cannot be run here."
  fi
  # The app keeps its mailbox list in Application Support (store.rs makes its
  # own folder there on first write).
  mkdir -p "$HOME/Library/Application Support"

  # The program itself rather than `open`, so there is a process to watch
  # and a log to read.
  "$exe" >"$WORK/mac.log" 2>&1 &
  pid=$!; start=$SECONDS
  while kill -0 "$pid" 2>/dev/null && [ $((SECONDS - start)) -lt "$UP" ]; do sleep 1; done
  if ! kill -0 "$pid" 2>/dev/null; then
    wait "$pid"; fail "RATA.app exited within ${UP} s of starting (exit $?)."
  else
    say "RATA.app is still running after $((SECONDS - start)) s."
    if title=$(mac_title "$pid"); then
      # One line per window, comma-separated if several; the app has one.
      case ", $title," in
        *", $EXPECT,"*) say "RATA.app's window: \"$EXPECT\"." ;;
        *) fail "RATA.app's windows are \"$title\"; none is \"$EXPECT\"." ;;
      esac
    else
      say "::warning::The window title could not be read on this runner ($(head -c 300 "$WORK/osascript.out")). Checked instead: RATA.app stayed up ${UP} s, which it does only once its window is built, and its log is clean."
    fi
  fi
  kill "$pid" 2>/dev/null; sleep 1; kill -9 "$pid" 2>/dev/null; wait "$pid" 2>/dev/null
  clean_log "RATA.app" "$WORK/mac.log"
}

case "$(uname -s)" in
  Linux) linux ;;
  Darwin) mac ;;
  *) fail "smoke-installed.sh runs on Linux and macOS; Windows has smoke-installed.ps1." ;;
esac

if [ "$FAILED" -ne 0 ]; then say "The installed app failed its launch check."; exit 1; fi
say "The installed app starts and shows \"$EXPECT\"."
