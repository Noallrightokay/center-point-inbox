#!/usr/bin/env bash
# Build the clickable demo: the real interface (app.html and bridge.js, as
# the desktop app ships them) over a fake Rust side with sample mail
# (demo-backend.js). Nothing in it sends mail or opens a mailbox.
#
#   desktop/rata-app/demo/build.sh [out-dir]      (default: demo/out)
#   desktop/rata-app/demo/build.sh --single [out-file]
#                                     (default: demo/out/RATA-demo.html)
#
# The first form writes a folder of static files: serve it with any web
# server (python3 -m http.server --directory <out-dir>) or publish
# rata-demo.html with the rest of the folder as its files. The second
# builds that folder in a temporary place and folds it into one HTML file
# (single.py) that opens by double-click, offline, with no server.
# demo-backend.js says what the demo holds; demo.css styles its label.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
app="$here/.."

if [ "${1:-}" = "--single" ]; then
  file="${2:-$here/out/RATA-demo.html}"
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT
  "$here/build.sh" "$tmp/demo" >/dev/null
  mkdir -p "$(dirname "$file")"
  python3 "$here/single.py" "$tmp/demo" "$file"
  echo "demo built in $file ($(wc -c <"$file" | tr -d ' ') bytes)"
  exit 0
fi

out="${1:-$here/out}"

"$app/sync-ui.sh" >/dev/null
rm -rf "$out"
mkdir -p "$out"
cp -r "$app/ui/fonts" "$app/ui/icons" "$app/ui/vendor" "$out/"
cp "$app/ui/bridge.js" "$app/ui/bridge.css" "$app/ui/config.js" "$out/"
cp "$here/demo-backend.js" "$out/"

python3 - "$app/ui/app.html" "$here/demo.css" "$out" <<'PY'
import glob, sys
page, css, out = sys.argv[1:]
s = open(page, encoding='utf-8').read()

def swap(a, b):
    global s
    if a not in s:
        sys.exit('app.html changed: cannot find ' + a[:60])
    s = s.replace(a, b, 1)

swap('<title>RATA — Your mail, on your machine.</title>', '<title>RATA Demo</title>')
swap('<link rel="manifest" href="manifest.json">\n', '')
# The demo ships no sw.js, so registering one only logged a 404.
import re
s, n = re.subn(r"if\('serviceWorker' in navigator\)\{[^\n]*?register\('sw\.js'\)[^\n]*\}\n", '', s)
if n != 1 or 'sw.js' in s:
    sys.exit('app.html changed: cannot find its one service worker registration')
# The fake backend must be in place before bridge.js looks for Tauri.
anchor = '<script src="config.js"></script>'
swap(anchor, '<style>\n' + open(css, encoding='utf-8').read() + '</style>\n'
     '<script src="demo-backend.js"></script>\n' + anchor)
open(out + '/rata-demo.html', 'w', encoding='utf-8').write(s)

# Some hosts refuse text files holding raw control characters or U+FFFD.
# The vendored libraries carry both, always inside string literals, where
# the escape means the same thing. A byte after a backslash would not, so
# that stops the build rather than guessing.
for f in glob.glob(out + '/vendor/*.js'):
    t = open(f, encoding='utf-8').read()
    o, changed = [], False
    for i, c in enumerate(t):
        if (ord(c) < 32 and c not in '\t\n\r') or c == '�':
            k, n = i - 1, 0
            while k >= 0 and t[k] == '\\':
                n += 1
                k -= 1
            if n % 2:
                sys.exit(f'{f}: a control character after a backslash at {i}; escape it by hand')
            o.append('\\ufffd' if c == '�' else '\\x%02x' % ord(c))
            changed = True
        else:
            o.append(c)
    if changed:
        open(f, 'w', encoding='utf-8').write(''.join(o))
PY

for f in "$out"/vendor/*.js "$out"/*.js; do node --check "$f" >/dev/null 2>&1 || { echo "$f no longer parses" >&2; exit 1; }; done
echo "demo built in $out"
