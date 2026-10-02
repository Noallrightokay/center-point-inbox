#!/bin/bash
# verify-release.sh TAG — download a published release and check every
# shipped binary: the .deb, the .AppImage, the Windows -setup.exe and both
# macOS .dmg files. In each: the licence public key (PEM or raw), the app id
# org.mailrata.desktop, the window title, and no font CDN
# (fonts.googleapis / fonts.gstatic). Then the .deb is launched under Xvfb
# and its window title read. Nothing else is executed: the AppImage is
# opened as the squashfs it carries after its ELF runtime, never run.
#   PUB_PEM=path/to/licence.pub WORK=/some/scratch/dir ./verify-release.sh v0.1.44
# Needs curl and python3; each check also needs its own tool, and says
# "skipped: <tool> missing" without it rather than passing:
#   .deb       dpkg-deb
#   .AppImage  unsquashfs (squashfs-tools; 7-Zip cannot read its zstd)
#   .exe/.dmg  7z (p7zip-full or 7zip; reads NSIS and HFS+/APFS images)
#   launch     Xvfb, xdotool, and the webview's libraries (ldd finds them all)
# Ends with one summary line. Exit status: 0 every asset checked and passed,
# 1 something failed, 3 nothing failed but something was skipped or missing
# from the release, so a partial run never reads as a pass.
#   ./verify-release.sh --title v1.0.0   prints the window title that release
#                                        must show, and nothing else.
set -u
# The title a version's window carries, derived as release.yml's smoke
# derives it: 0.x is a beta ("RATA 0.1.42 beta"), 1.x and on are not
# ("RATA 1.0.0").
title_for() {
  local v="${1#v}"
  case "$v" in 0.*) echo "RATA $v beta" ;; *) echo "RATA $v" ;; esac
}
if [ "${1:-}" = "--title" ]; then title_for "${2:?give a version or tag}"; exit 0; fi
PUB="${PUB_PEM:?set PUB_PEM to the licence public key (PEM) the release was built with}"
case "$PUB" in /*) ;; *) PUB="$PWD/$PUB" ;; esac
[ -r "$PUB" ] || { echo "PUB_PEM: cannot read $PUB"; exit 1; }
S="${WORK:-$PWD/rata-verify}"
T="${1:?give a release tag, e.g. v0.1.44}"; D="$S/$T"; mkdir -p "$D"; cd "$D" || exit 1
TITLE=$(title_for "$T"); echo "expected title: $TITLE"
REPO=Noallrightokay/center-point-inbox
names=$(curl -sSL "https://api.github.com/repos/$REPO/releases/tags/$T" | python3 -c 'import json,sys; [print(a["name"]) for a in json.load(sys.stdin).get("assets", [])]')
echo "assets: $(echo "$names" | grep -c .)"; echo "$names"
for n in $names; do
  case "$n" in
    *.deb|*setup.exe|*.AppImage|*.dmg)
      [ -f "$n" ] || curl -sSLO "https://github.com/$REPO/releases/download/$T/$n" ;;
  esac
done
KEYB=$(grep -v '^-----' "$PUB" | tr -d '\n'); RAW=$(echo "$KEYB" | base64 -d | tail -c 32 | base64)

PASS=0; FAIL=0; SKIP=0
have() { command -v "$1" >/dev/null 2>&1; }
skip() { echo "$1: skipped: $2"; SKIP=$((SKIP + 1)); }
bad() { echo "$1: FAIL: $2"; FAIL=$((FAIL + 1)); }
# A title the compiler did not keep as one string. An x86-64 build can
# write a short constant straight into memory with 8-byte `movabs`
# immediates (REX.W B8+r), so "RATA 0.1.44 beta" is in the code as
# "RATA 0.1" and ".44 beta", never side by side (v0.1.44's macOS x64
# binary). Counted only when every 8-byte piece of the title (the last one
# taken from the end, overlapping) is such an immediate, all within 64
# bytes of the first: the title in code, not two strings that happen to be
# about. A title that does not split this way fails, never passes.
title_in_code() {
  python3 - "$1" "$2" <<'PY'
import re, sys
d = open(sys.argv[1], 'rb').read()
t = sys.argv[2].encode()
if len(t) < 8:
    print(0)
    sys.exit()
pieces = [t[i:i + 8] for i in range(0, len(t) - 7, 8)]
if len(t) % 8:
    pieces.append(t[-8:])
def at(p):
    return [m.start() for m in re.finditer(rb'[\x48\x49][\xb8-\xbf]' + re.escape(p), d)]
rest = [at(p) for p in pieces[1:]]
print(sum(1 for f in at(pieces[0]) if all(any(abs(g - f) <= 64 for g in r) for r in rest)))
PY
}
# The bundle identifier a macOS app declares (Contents/Info.plist).
bundle_id() {
  python3 - "$1" <<'PY'
import plistlib, sys
try:
    print(plistlib.load(open(sys.argv[1], 'rb')).get('CFBundleIdentifier', ''))
except Exception:
    print('')
PY
}
# One binary: print what was found, then whether that is a pass. The key
# counts as present in either form (PEM body or raw 32 bytes in base64). A
# third argument is a macOS Info.plist, whose CFBundleIdentifier must be
# the app id too.
judge() {
  local label="$1" b="$2" plist="${3:-}" pem raw id title code="" fonts bundle=""
  if [ ! -f "$b" ]; then bad "$label" "no binary at $b"; return; fi
  pem=$(grep -ac "$KEYB" "$b"); raw=$(grep -ac "$RAW" "$b")
  id=$(grep -ac org.mailrata.desktop "$b"); title=$(grep -acF "$TITLE" "$b")
  [ "$title" -ge 1 ] || code=$(title_in_code "$b" "$TITLE")
  fonts=$(grep -acE 'fonts\.(googleapis|gstatic)' "$b")
  [ -z "$plist" ] || bundle=$(bundle_id "$plist")
  echo "$label: ${b#"$D"/} key-pem=$pem key-raw=$raw id=$id title=$title${code:+ title-in-code=$code} fonts=$fonts${plist:+ bundle-id=${bundle:-none}}"
  local why=""
  [ $((pem + raw)) -ge 1 ] || why="$why; no licence key"
  [ "$id" -ge 1 ] || why="$why; no org.mailrata.desktop"
  [ -z "$plist" ] || [ "$bundle" = org.mailrata.desktop ] || why="$why; Info.plist bundle id is ${bundle:-missing}"
  [ "$title" -ge 1 ] || [ "${code:-0}" -ge 1 ] || why="$why; no \"$TITLE\""
  [ "$fonts" -eq 0 ] || why="$why; a font CDN address"
  if [ -z "$why" ]; then echo "$label: pass"; PASS=$((PASS + 1)); else bad "$label" "${why#; }"; fi
}
# The one file of a kind in the release, or nothing.
only() { local f; for f in $1; do [ -f "$f" ] && { echo "$f"; return; }; done; }

# .deb
deb=$(only '*.deb')
if [ -z "$deb" ]; then skip ".deb" "missing from the release"
elif ! have dpkg-deb; then skip "$deb" "dpkg-deb missing"
else
  rm -rf "${D:?}/deb"; mkdir -p deb
  if dpkg-deb -x "$deb" deb; then judge "$deb" "$D/deb/usr/bin/rata-app"; else bad "$deb" "dpkg-deb could not unpack it"; fi
fi

# .AppImage: an ELF runtime with a squashfs appended where the ELF ends
# (what the runtime's own --appimage-offset prints). That offset is tried
# first; then every "hsqs" magic in the file, since the runtime itself
# carries the string. Never executed.
app=$(only '*.AppImage')
if [ -z "$app" ]; then skip ".AppImage" "missing from the release"
elif ! have unsquashfs; then skip "$app" "unsquashfs missing (apt-get install squashfs-tools)"
else
  off=""
  for o in $(python3 - "$app" <<'PY'
import struct, sys
d = open(sys.argv[1], 'rb').read()
seen = []
if d[:4] == b'\x7fELF':
    if d[4] == 2:
        fmt = '<' if d[5] == 1 else '>'
        shoff, = struct.unpack_from(fmt + 'Q', d, 0x28)
        size, num = struct.unpack_from(fmt + 'HH', d, 0x3A)
    else:
        fmt = '<' if d[5] == 1 else '>'
        shoff, = struct.unpack_from(fmt + 'I', d, 0x20)
        size, num = struct.unpack_from(fmt + 'HH', d, 0x2E)
    seen.append(shoff + size * num)
i = d.find(b'hsqs')
while i != -1 and len(seen) < 64:
    if i not in seen: seen.append(i)
    i = d.find(b'hsqs', i + 1)
print(' '.join(map(str, seen)))
PY
  ); do
    if unsquashfs -s -o "$o" "$app" >/dev/null 2>&1; then off="$o"; break; fi
  done
  if [ -z "$off" ]; then bad "$app" "no squashfs found after its ELF runtime"
  else
    echo "$app: squashfs at offset $off"
    rm -rf "${D:?}/appimage"
    if unsquashfs -q -n -o "$off" -d appimage "$app" usr/bin/rata-app >/dev/null 2>&1; then
      judge "$app" "$D/appimage/usr/bin/rata-app"
    else bad "$app" "unsquashfs could not extract usr/bin/rata-app"; fi
  fi
fi

# Windows: 7-Zip opens the NSIS installer.
exe=$(only '*setup.exe')
if [ -z "$exe" ]; then skip "setup.exe" "missing from the release"
elif ! have 7z; then skip "$exe" "7z missing"
else
  rm -rf "${D:?}/exe"
  if 7z x -y -oexe "$exe" >/dev/null 2>&1; then judge "$exe" "$D/exe/rata-app.exe"; else bad "$exe" "7z could not unpack it"; fi
fi

# macOS: each .dmg (one per architecture), opened with 7-Zip, which reads
# the disk image and the HFS+ or APFS volume inside; the binary is the one
# under RATA.app/Contents/MacOS.
for dmg in *.dmg; do
  [ -f "$dmg" ] || continue
  if ! have 7z; then skip "$dmg" "7z missing"; continue; fi
  dir="dmg-${dmg%.dmg}"; rm -rf "${D:?}/$dir"
  # Its exit status is not the verdict: 7-Zip can complain about an image's
  # other (empty) partitions. What matters is whether the app came out.
  7z x -y -o"$dir" "$dmg" >/dev/null 2>&1
  bin=$(find "$dir" -path '*.app/Contents/MacOS/*' -type f 2>/dev/null | head -n 1)
  if [ -z "$bin" ]; then bad "$dmg" "7z found no .app/Contents/MacOS binary in it"
  else judge "$dmg" "$D/$bin" "$D/${bin%/MacOS/*}/Info.plist"; fi
done
for arch in aarch64 x64; do
  echo "$names" | grep -q "_${arch}\.dmg$" || skip "${arch}.dmg" "missing from the release"
done

# Launch the .deb's binary under Xvfb and read its window title.
if [ ! -x "$D/deb/usr/bin/rata-app" ]; then skip "launch" "no unpacked .deb binary"
elif ! have Xvfb; then skip "launch" "Xvfb missing"
elif ! have xdotool; then skip "launch" "xdotool missing"
elif have ldd && ldd "$D/deb/usr/bin/rata-app" 2>/dev/null | grep -q 'not found'; then
  skip "launch" "libraries missing: $(ldd "$D/deb/usr/bin/rata-app" | awk '/not found/ {print $1}' | tr '\n' ' ')"
else
  export HOME="$D/home"; mkdir -p "$HOME"
  Xvfb :77 -screen 0 1280x800x24 >/dev/null 2>&1 & X=$!; sleep 2
  DISPLAY=:77 "$D/deb/usr/bin/rata-app" >/dev/null 2>&1 & A=$!
  # A cold machine can take 20 s or more to draw the first window; wait for
  # it, up to a minute, rather than a fixed time.
  for _ in $(seq 1 60); do
    DISPLAY=:77 xdotool search --name RATA >/dev/null 2>&1 && break
    sleep 1
  done
  sleep 2
  seen=""
  for w in $(DISPLAY=:77 xdotool search --name RATA 2>/dev/null); do
    n=$(DISPLAY=:77 xdotool getwindowname "$w")
    if [ "$n" = "$TITLE" ]; then echo "window: $n (as expected)"; seen=yes; else echo "window: $n (expected: $TITLE)"; fi
  done
  if [ -n "$seen" ]; then echo "launch: pass"; PASS=$((PASS + 1)); else bad "launch" "no window titled \"$TITLE\""; fi
  kill $A $X 2>/dev/null
fi

echo "summary: $PASS passed, $FAIL failed, $SKIP skipped"
[ "$FAIL" -eq 0 ] || exit 1
[ "$SKIP" -eq 0 ] || exit 3
exit 0
