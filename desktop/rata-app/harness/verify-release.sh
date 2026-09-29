#!/bin/bash
# verify-release.sh TAG — download a published release and check the shipped
# binaries: the licence public key (PEM and raw), the app id, the window title,
# no font CDN, and that the .deb launches under Xvfb with the right title.
#   PUB_PEM=path/to/licence.pub WORK=/some/scratch/dir ./verify-release.sh v0.1.38
# Needs curl, python3, dpkg-deb, 7z, Xvfb and xdotool.
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
S="${WORK:-$PWD/rata-verify}"
T="$1"; D="$S/$T"; mkdir -p "$D"; cd "$D" || exit 1
TITLE=$(title_for "$T"); echo "expected title: $TITLE"
names=$(curl -sSL "https://api.github.com/repos/Noallrightokay/center-point-inbox/releases/tags/$T" | python3 -c 'import json,sys; [print(a["name"]) for a in json.load(sys.stdin)["assets"]]')
echo "assets: $(echo "$names" | wc -l)"; echo "$names"
for n in $names; do case "$n" in *.deb|*setup.exe) [ -f "$n" ] || curl -sSLO "https://github.com/Noallrightokay/center-point-inbox/releases/download/$T/$n";; esac; done
KEYB=$(grep -v '^-----' "$PUB" | tr -d '\n'); RAW=$(echo "$KEYB" | base64 -d | tail -c 32 | base64)
rm -rf "${D:?}/deb" "${D:?}/exe"; mkdir -p deb exe
dpkg-deb -x ./*.deb deb; 7z x -y -oexe ./*setup.exe >/dev/null
for b in deb/usr/bin/rata-app exe/rata-app.exe; do
  echo "$b: key-pem=$(grep -ac "$KEYB" "$b") key-raw=$(grep -ac "$RAW" "$b") id=$(grep -ac org.mailrata.desktop "$b") title=$(grep -acF "$TITLE" "$b") fonts=$(grep -acE 'fonts\.(googleapis|gstatic)' "$b")"
done
export HOME="$D/home"; mkdir -p "$HOME"
Xvfb :77 -screen 0 1280x800x24 >/dev/null 2>&1 & X=$!; sleep 2
DISPLAY=:77 deb/usr/bin/rata-app >/dev/null 2>&1 & A=$!; sleep 12
for w in $(DISPLAY=:77 xdotool search --name RATA); do
  n=$(DISPLAY=:77 xdotool getwindowname "$w")
  if [ "$n" = "$TITLE" ]; then echo "window: $n (as expected)"; else echo "window: $n (expected: $TITLE)"; fi
done
kill $A $X 2>/dev/null
