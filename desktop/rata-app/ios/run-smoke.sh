#!/usr/bin/env bash
# run-smoke.sh SIMULATOR OUT NAME — launch RATA (a build with the `smoke`
# feature, already installed on the booted simulator SIMULATOR), wait for
# its self-test's smoke.json, and keep it, the app's output and a
# screenshot in OUT as NAME.smoke.json, NAME.stdout.log, NAME.stderr.log
# and NAME.png. Exit 0 when every check passed and RATA is still running.
#
# What the app is given at launch goes in SIMCTL_CHILD_ variables, which
# simctl hands to it without the prefix:
#   SIMCTL_CHILD_RATA_SMOKE_LICENCE  a licence to store (src-tauri/src/smoke.rs)
#   SIMCTL_CHILD_RATA_SMOKE_VIEW     a view to open: inbox, set (Settings)…
#
# Used by .github/workflows/ios.yml, and on any Mac with Xcode:
#   xcrun simctl boot "iPhone 16"; xcrun simctl install booted RATA.app
#   desktop/rata-app/ios/run-smoke.sh booted /tmp/rata-smoke first-screen
set -uo pipefail
sim="${1:?give the simulator (a UDID, or booted)}"
out="${2:?give a folder for what the run leaves}"
name="${3:?give the run a name}"
bundle="${RATA_BUNDLE:-org.mailrata.desktop}"
wait_for="${RATA_SMOKE_WAIT:-180}"
mkdir -p "$out"

running() {
  xcrun simctl spawn "$sim" launchctl list 2>/dev/null \
    | awk -v app="UIKitApplication:$bundle" 'index($3, app) == 1 && $1 ~ /^[0-9]+$/ { found = 1 } END { exit !found }'
}

xcrun simctl terminate "$sim" "$bundle" >/dev/null 2>&1 || true
container="$(xcrun simctl get_app_container "$sim" "$bundle" data)" \
  || { echo "$name: RATA is not installed on $sim"; exit 1; }
smoke="$container/Library/Application Support/$bundle/smoke.json"
rm -f "$smoke"

echo "$name: launching RATA"
xcrun simctl launch --stdout="$out/$name.stdout.log" --stderr="$out/$name.stderr.log" \
  "$sim" "$bundle" || { echo "$name: RATA did not launch"; exit 1; }

started=$(date +%s)
while [ ! -f "$smoke" ]; do
  if ! running; then
    sleep 2
    if ! running && [ ! -f "$smoke" ]; then
      echo "$name: RATA stopped before its self-test finished. Its last words:"
      tail -n 40 "$out/$name.stderr.log" "$out/$name.stdout.log" 2>/dev/null
      xcrun simctl io "$sim" screenshot "$out/$name.png" >/dev/null 2>&1
      exit 1
    fi
  fi
  if [ $(( $(date +%s) - started )) -ge "$wait_for" ]; then
    echo "$name: no smoke.json after ${wait_for} s"
    xcrun simctl io "$sim" screenshot "$out/$name.png" >/dev/null 2>&1
    exit 1
  fi
  sleep 3
done
echo "$name: smoke.json after $(( $(date +%s) - started )) s"

cp "$smoke" "$out/$name.smoke.json"
xcrun simctl io "$sim" screenshot "$out/$name.png"
cat "$out/$name.smoke.json"

python3 - "$out/$name.smoke.json" "$name" <<'PY'
import json, sys
doc = json.load(open(sys.argv[1]))
for c in doc.get('checks', []):
    print(f"{sys.argv[2]}: {'pass' if c.get('ok') else 'FAIL'}: {c.get('name')}")
sys.exit(0 if doc.get('ok') is True else 1)
PY
verdict=$?
if ! running; then
  echo "$name: RATA is no longer running"
  verdict=1
fi
exit "$verdict"
