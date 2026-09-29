#!/bin/bash
# live-check.sh [BASE] — the public half of the live smoke test (LAUNCH.md §6,
# MVP-PLAN card D5). Run it after every deploy of rata-next. It needs only
# curl and python3, sends no credentials, and prints one PASS/FAIL line per
# check, then exits non-zero if any failed.
#
#   ./scripts/live-check.sh                 # checks https://mailrata.org
#   ./scripts/live-check.sh https://staging.example
#
# What it proves: the deploy is the current code (the routes the old deploy
# lacked answer), the app can talk to it cross-origin (the OPTIONS preflight
# from the app's own origins is allowed, and from a stranger's is not), the
# security headers are on, /api/config publishes no service-role key, and the
# landing page's screenshots are there. It cannot sign up, pay or link a
# mailbox: those are the owner's steps in LAUNCH.md §6.
set -u
BASE="${1:-https://mailrata.org}"
BASE="${BASE%/}"
fails=0
pass() { printf '  PASS  %s\n' "$1"; }
fail() { printf '  FAIL  %s\n' "$1"; fails=$((fails + 1)); }
check() { if [ "$1" = 0 ]; then pass "$2"; else fail "$2"; fi; }

# status URL [curl args...] -> prints the HTTP status
status() { local url="$1" s; shift; s=$(curl -s -o /dev/null -w '%{http_code}' --max-time 20 "$@" "$url" 2>/dev/null); printf '%s' "${s:-000}"; }
# header URL NAME [curl args...] -> prints the header's value, lowercase name match
header() { local url="$1" name="$2"; shift 2; curl -sSI --max-time 20 "$@" "$url" 2>/dev/null | tr -d '\r' | awk -v n="$name" 'BEGIN{IGNORECASE=1} tolower($0) ~ "^" tolower(n) ":" {sub(/^[^:]*:[ \t]*/, ""); print; exit}'; }

echo "Checking $BASE"
# Wait for the server to answer at all (a fresh `npm start` takes a few seconds).
curl -s -o /dev/null --retry 20 --retry-delay 1 --retry-connrefused --retry-all-errors --max-time 10 "$BASE/" || true

echo "- the deploy is the current code -"
s=$(status "$BASE/api/health"); [ "$s" = 200 ] || [ "$s" = 503 ]; check $? "/api/health answers ($s; 503 means up but not ready)"
h=$(curl -sS --max-time 20 "$BASE/api/health" 2>/dev/null)
python3 - "$h" <<'EOF'; check $? "/api/health is RATA's own JSON (ok, ready, service)"
import json, sys
try:
    d = json.loads(sys.argv[1])
    sys.exit(0 if d.get("service") == "rata" and "ok" in d and "ready" in d else 1)
except Exception:
    sys.exit(1)
EOF
python3 - "$h" <<'EOF'; check $? "/api/health says ready (else it lists what is failing)"
import json, sys
try:
    d = json.loads(sys.argv[1])
    if d.get("ready"): sys.exit(0)
    print("        failing:", ", ".join(d.get("failing", [])) or "(not told; use HEALTH_TOKEN for detail)")
    sys.exit(1)
except Exception:
    sys.exit(1)
EOF
s=$(status "$BASE/api/licence/renew" -X POST -H 'Content-Type: application/json' --data '{}'); [ "$s" != 404 ] && [ "$s" != 000 ]; check $? "/api/licence/renew exists ($s)"
s=$(status "$BASE/api/ai" -X POST -H 'Content-Type: application/json' --data '{}'); [ "$s" != 404 ] && [ "$s" != 000 ]; check $? "/api/ai exists ($s)"
s=$(status "$BASE/shots/app-light.webp"); [ "$s" = 200 ]; check $? "landing page screenshot is served ($s)"
s=$(status "$BASE/account"); [ "$s" = 200 ]; check $? "/account is served ($s)"

echo "- the app may call it, strangers may not -"
for o in tauri://localhost http://tauri.localhost https://tauri.localhost; do
  for p in /api/ai /api/licence/renew; do
    a=$(header "$BASE$p" access-control-allow-origin -X OPTIONS -H "Origin: $o" -H 'Access-Control-Request-Method: POST' -H 'Access-Control-Request-Headers: content-type')
    [ "$a" = "$o" ]; check $? "preflight from $o on $p is allowed"
  done
done
a=$(header "$BASE/api/ai" access-control-allow-origin -X OPTIONS -H 'Origin: https://evil.example' -H 'Access-Control-Request-Method: POST')
[ -z "$a" ]; check $? "preflight from a stranger's origin gets no permission"

echo "- the config endpoint -"
# /api/config is a JavaScript file (window.RATA_CONFIG = {...};), not JSON.
c=$(curl -sS --max-time 20 "$BASE/api/config" 2>/dev/null)
cfg=$(python3 - "$c" <<'EOF2'
import json, re, sys
m = re.search(r"window\.RATA_CONFIG\s*=\s*(\{.*?\})\s*;", sys.argv[1], re.S)
if not m: sys.exit(1)
try:
    json.loads(m.group(1)); print(m.group(1))
except Exception:
    sys.exit(1)
EOF2
)
[ -n "$cfg" ]; check $? "/api/config is RATA's own window.RATA_CONFIG"
python3 - "$cfg" <<'EOF2'; check $? "/api/config carries a Supabase URL and anon key (empty means the variables are not set)"
import json, sys
try:
    d = json.loads(sys.argv[1] or "{}")
    sys.exit(0 if d.get("supabaseUrl") and d.get("supabaseKey") else 1)
except Exception:
    sys.exit(1)
EOF2
python3 - "$cfg" <<'EOF2'; check $? "/api/config publishes no service-role key"
import json, sys, base64
try:
    d = json.loads(sys.argv[1] or "{}")
except Exception:
    sys.exit(1)
for v in d.values():
    if isinstance(v, str) and v.count(".") == 2:
        try:
            body = v.split(".")[1]
            body += "=" * (-len(body) % 4)
            claims = json.loads(base64.urlsafe_b64decode(body))
            if claims.get("role") == "service_role": sys.exit(1)
        except Exception:
            pass
sys.exit(0)
EOF2
python3 - "$cfg" <<'EOF2'; check $? "/api/config publishes no googleClientId, stripeMonthly or stripeAnnual (old deploy keys)"
import json, sys
try:
    d = json.loads(sys.argv[1] or "{}")
    sys.exit(1 if any(k in d for k in ("googleClientId", "stripeMonthly", "stripeAnnual")) else 0)
except Exception:
    sys.exit(1)
EOF2

echo "- security headers -"
for p in / /api/config; do
  for pair in "strict-transport-security:max-age" "x-content-type-options:nosniff" "x-frame-options:DENY" "referrer-policy:strict-origin-when-cross-origin"; do
    name="${pair%%:*}"; want="${pair#*:}"
    v=$(header "$BASE$p" "$name")
    case "$v" in *"$want"*) check 0 "$p sends $name";; *) check 1 "$p sends $name (got: ${v:-nothing})";; esac
  done
done

echo "- the landing page -"
body=$(curl -sS --max-time 20 "$BASE/" 2>/dev/null)
printf '%s' "$body" | grep -q 'Your mail, on your machine'; check $? "landing page is the current one"
! printf '%s' "$body" | grep -qiE 'slack|discord|texts and email|[$]8'; check $? "landing page sells nothing removed (no Slack, Discord, texts, \$8)"
! printf '%s' "$body" | grep -qE 'fonts\.(googleapis|gstatic)\.com'; check $? "landing page loads no Google font"

echo
if [ "$fails" = 0 ]; then echo "All checks passed."; else echo "$fails check(s) failed."; fi
[ "$fails" = 0 ]
