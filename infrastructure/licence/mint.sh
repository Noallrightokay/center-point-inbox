#!/usr/bin/env bash
# Mint a RATA licence key, on the VPS or any machine with OpenSSL 3.
#
#   sudo ./mint.sh check                      is there a signing key, and is it
#                                             the one the released app trusts?
#   sudo ./mint.sh you@example.com pro 30     print a licence for that address
#   sudo ./mint.sh new                        make a signing key (only if none)
#
# The key lives at /etc/rata/licence.key (override with RATA_LICENCE_KEY).
# It is the private half of an Ed25519 pair. Never paste it anywhere, never
# commit it, and keep a copy somewhere safe and separate from the database
# backups (infrastructure/backup/README.md).
#
# A licence is v1.<payload>.<signature>, exactly what rata-next/lib/licence.js
# issues: payload is base64url of {"v":1,"sub":…,"plan":…,"iat":…,"exp":…}
# and the signature is Ed25519 over the payload's base64url text. The app
# checks it offline against the public half compiled into it.
set -euo pipefail

KEY="${RATA_LICENCE_KEY:-/etc/rata/licence.key}"

# The public half compiled into the released installers (checked in v0.1.42
# with desktop/rata-app/harness/verify-release.sh). A licence signed by any
# other key is refused by those installers with a signature error.
SHIPPED="MCowBQYDK2VwAyEAxQq84CRvlV6Ac1NdFWuwh41zgvBqNWT/xV9Ft/9WhZs="

die() { echo "mint.sh: $*" >&2; exit 1; }
b64url() { openssl base64 -A | tr '+/' '-_' | tr -d '='; }
public_of() { openssl pkey -in "$1" -pubout 2>/dev/null | grep -v -- '-----' | tr -d '\n'; }

command -v openssl >/dev/null || die "openssl is not installed (apt install openssl)."
openssl version | grep -q '^OpenSSL 3' || die "needs OpenSSL 3 for Ed25519 signing; found: $(openssl version)"

matches() {
  [ -r "$KEY" ] || return 2
  [ "$(public_of "$KEY")" = "$SHIPPED" ]
}

case "${1:-}" in
  check)
    if [ ! -e "$KEY" ]; then
      echo "No signing key at $KEY."
      echo "If you have the original key file somewhere else, run with RATA_LICENCE_KEY=/path/to/it."
      echo "If it is lost, run: sudo $0 new   (the app then needs a new release; see the note it prints)."
      exit 1
    fi
    [ -r "$KEY" ] || die "cannot read $KEY (run with sudo)."
    if matches; then
      echo "OK: $KEY is the key the released app trusts. Licences minted with it will be accepted."
    else
      echo "MISMATCH: $KEY is a valid key, but not the one the released app trusts."
      echo "Its public half is: $(public_of "$KEY")"
      echo "The app expects:    $SHIPPED"
      echo "Licences minted with it are refused. Either find the original key, or put this key's"
      echo "public half in the RATA_LICENCE_PUBLIC_KEY GitHub secret and release a new build."
      exit 1
    fi
    ;;

  new)
    [ -e "$KEY" ] && die "$KEY already exists; refusing to overwrite a signing key. Move it aside yourself if you really mean to."
    mkdir -p "$(dirname "$KEY")"
    chmod 700 "$(dirname "$KEY")"
    ( umask 077; openssl genpkey -algorithm ed25519 -out "$KEY" )
    chmod 600 "$KEY"
    echo "Made a new signing key at $KEY (mode 600). Back it up somewhere safe now."
    echo
    echo "The released app does not trust it yet. To switch the app over:"
    echo "  1. GitHub → the repository → Settings → Secrets and variables → Actions →"
    echo "     RATA_LICENCE_PUBLIC_KEY → Update, and paste these three lines:"
    echo
    openssl pkey -in "$KEY" -pubout
    echo
    echo "  2. Ask for a new release (a version bump). Installers from that release"
    echo "     accept licences from this key; 0.1.42 and older will not."
    echo "  3. The website's LICENCE_PRIVATE_KEY must be this same key (rata-next/LAUNCH.md §2)."
    ;;

  ""|-h|--help)
    sed -n '2,8p' "$0" | sed 's/^# \{0,1\}//'
    ;;

  *)
    email="$(printf '%s' "$1" | tr '[:upper:]' '[:lower:]' | tr -d '[:space:]')"
    plan="${2:-}"
    days="${3:-30}"
    [[ "$email" =~ ^[a-z0-9._%+-]+@[a-z0-9.-]+\.[a-z]{2,}$ ]] || die "'$1' does not look like an email address."
    case "$plan" in base|pro|enterprise) ;; *) die "plan must be base, pro or enterprise (got '$plan')." ;; esac
    [[ "$days" =~ ^[0-9]+$ ]] && [ "$days" -ge 1 ] && [ "$days" -le 400 ] || die "days must be 1 to 400 (got '$days')."
    [ -e "$KEY" ] || die "no signing key at $KEY. Run: sudo $0 check"
    [ -r "$KEY" ] || die "cannot read $KEY (run with sudo)."
    if ! matches && [ "${RATA_ANY_KEY:-}" != "1" ]; then
      die "$KEY is not the key the released app trusts, so the app would refuse this licence. Run: sudo $0 check"
    fi
    now=$(date +%s)
    payload=$(printf '{"v":1,"sub":"%s","plan":"%s","iat":%d,"exp":%d}' "$email" "$plan" "$now" $((now + days * 86400)))
    body=$(printf '%s' "$payload" | b64url)
    tmp=$(mktemp); trap 'rm -f "$tmp"' EXIT
    printf '%s' "$body" > "$tmp"
    sig=$(openssl pkeyutl -sign -inkey "$KEY" -rawin -in "$tmp" | b64url)
    echo "v1.$body.$sig"
    echo "Licence for $email, plan $plan, valid $days days (until $(date -u -d "@$((now + days * 86400))" '+%Y-%m-%d %H:%M UTC'))." >&2
    echo "Paste the v1. line into RATA as one line. Send it to its owner privately." >&2
    ;;
esac
