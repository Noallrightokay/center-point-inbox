#!/usr/bin/env bash
# Opens one backup made by backup.sh, for a restore or the monthly drill.
#
#   open-backup.sh rata-<stamp>.tar [into-dir]
#
# Writes <into-dir>/rata-<stamp>/{manifest.txt,data.sql,schema.sql}, readable
# by you alone. That is every account's login and billing row in plain text:
# run it on the machine doing the restore, never on a shared one, and delete
# the directory when you are done (the drill's last step).
#
# Keys, from the environment, never from the command line:
#   age backups:  AGE_IDENTITY=/path/to/identity.txt (the private half, from
#                 wherever you keep it; not the VPS)
#   gpg backups:  BACKUP_PASSPHRASE, or it asks for it without echoing
set -Eeuo pipefail
umask 077

TAR="${1:?usage: open-backup.sh rata-<stamp>.tar [into-dir]}"
INTO="${2:-.}"
die() { echo "open-backup: $*" >&2; exit 1; }
[ -f "$TAR" ] || die "no such file: $TAR"

# Only what backup.sh writes: one directory and three encrypted files. A tar
# with anything else in it (an absolute path, a ..) is not ours and is not
# unpacked.
NAME=""
while IFS= read -r m; do
  case "$m" in
    rata-[0-9]*Z/) NAME="${m%/}" ;;
    rata-[0-9]*Z/data.sql.gz.age|rata-[0-9]*Z/data.sql.gz.gpg) ;;
    rata-[0-9]*Z/schema.sql.gz.age|rata-[0-9]*Z/schema.sql.gz.gpg) ;;
    rata-[0-9]*Z/manifest.txt.age|rata-[0-9]*Z/manifest.txt.gpg) ;;
    *) die "unexpected entry in the tar: $m" ;;
  esac
  case "$m" in *..*|/*) die "unexpected entry in the tar: $m" ;; esac
done < <(tar -tf "$TAR")
[ -n "$NAME" ] || NAME="$(tar -tf "$TAR" | head -1 | cut -d/ -f1)"

mkdir -p "$INTO"
[ ! -e "$INTO/$NAME" ] || die "$INTO/$NAME already exists; move it away first"
SEALED="$(mktemp -d)"
trap 'rm -rf "$SEALED"' EXIT
tar -C "$SEALED" --no-same-owner -xf "$TAR"

if [ -f "$SEALED/$NAME/manifest.txt.age" ]; then
  ENC=age
  [ -n "${AGE_IDENTITY:-}" ] || die "this backup is age-encrypted: set AGE_IDENTITY to the identity file"
  decrypt() { age -d -i "$AGE_IDENTITY" "$1"; }
else
  ENC=gpg
  if [ -z "${BACKUP_PASSPHRASE:-}" ]; then
    read -rsp "Backup passphrase: " BACKUP_PASSPHRASE < /dev/tty; echo >&2
  fi
  decrypt() {
    gpg --batch --quiet --pinentry-mode loopback --passphrase-fd 3 -d "$1" \
      3< <(printf '%s' "$BACKUP_PASSPHRASE")
  }
fi

mkdir "$INTO/$NAME"
OUT="$INTO/$NAME"
decrypt "$SEALED/$NAME/manifest.txt.$ENC" > "$OUT/manifest.txt" || die "could not decrypt the manifest (wrong key?)"
decrypt "$SEALED/$NAME/data.sql.gz.$ENC" | gunzip > "$OUT/data.sql" || die "could not decrypt data.sql"
decrypt "$SEALED/$NAME/schema.sql.gz.$ENC" | gunzip > "$OUT/schema.sql" || die "could not decrypt schema.sql"

echo "opened $OUT:"
grep '^table ' "$OUT/manifest.txt" | awk '{printf "  %-32s %s rows\n", $2, $3}'
