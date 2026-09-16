#!/usr/bin/env bash
# Proves backup.sh works without touching a real database.
#
# A backup script that has never been run is a belief. This runs the real one
# end to end against a stubbed pg_dumpall and a stubbed rclone, with real gzip
# and real gpg — so the encryption, the size guard, the upload check and the
# restore are all genuinely exercised. The test that matters is the last one in
# the first block: the canary row has to survive encrypt, ship and decrypt.
#
#   bash infrastructure/backup/selftest.sh
#
# Needs gpg and gzip. Nothing else, and no network.
set -u
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
S="$HERE/backup.sh"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
mkdir -p "$WORK"/{bin,stage,remote,compose}
cp "$HERE/stubs/pg-dumpall-stub" "$WORK/bin/docker"
chmod +x "$WORK/bin/docker"

export PATH="$WORK/bin:$PATH"
export BACKUP_PASSPHRASE='test-passphrase-not-a-real-one'
export RCLONE_REMOTE="remote:$WORK/remote"
export COMPOSE_DIR="$WORK/compose"
export STAGE="$WORK/stage"
export KEEP_DAYS=14

# Restore a working rclone every run: the last case deliberately breaks it, and
# leaving it broken meant the next run started poisoned — which is what made
# the script look guilty when the harness was at fault.
good_rclone(){ cat > "$WORK/bin/rclone" <<'R'
#!/usr/bin/env bash
case "$1" in
  copy) cp "$2" "${3#*:}/" ;;
  lsf)  p="${2#*:}"; [ -f "$p" ] && basename "$p" || exit 1 ;;
esac
R
chmod +x "$WORK/bin/rclone"; }
good_rclone
pass=0; fail=0
ck(){ if [ "$2" = "$3" ]; then echo "  PASS  $1"; pass=$((pass+1)); else echo "  FAIL  $1 (got '$2' want '$3')"; fail=$((fail+1)); fi; }

echo "— a normal night —"
rm -f "$WORK"/stage/*.gpg "$WORK"/remote/*.gpg
out=$(bash "$S" 2>&1); rc=$?
ck "the script exits clean" "$rc" "0"
n=$(ls "$WORK"/remote/*.sql.gz.gpg 2>/dev/null | wc -l)
ck "one encrypted dump reached the remote" "$n" "1"
f=$(ls "$WORK"/remote/*.sql.gz.gpg | head -1)
ck "nothing readable landed on disk" "$(grep -ac CANARY "$f" 2>/dev/null; true)" "0"

echo "— and it restores, which is the only thing that matters —"
gpg --batch --quiet --passphrase "$BACKUP_PASSPHRASE" -d "$f" 2>/dev/null | gunzip > "$WORK/restored.sql"
ck "the canary row survived encrypt -> ship -> decrypt" "$(grep -c CANARY "$WORK/restored.sql")" "1"
ck "and the rows came back" "$(grep -c '@example.com' "$WORK/restored.sql")" "150"
ck "with the clean/if-exists guard a restore needs" "$(grep -c 'DROP DATABASE IF EXISTS' "$WORK/restored.sql")" "1"

good_rclone
echo "— a dump that came back suspiciously small is a failure, not a success —"
rm -f "$WORK"/stage/*.gpg "$WORK"/remote/*.gpg
out=$(TINY=1 bash "$S" 2>&1); rc=$?
ck "it refuses to call that a backup" "$([ $rc -ne 0 ] && echo refused || echo accepted)" "refused"
ck "and says so" "$(echo "$out" | grep -ci 'refusing')" "1"
ck "nothing was shipped" "$(ls "$WORK"/remote/*.gpg 2>/dev/null | wc -l)" "0"

echo "— an upload that silently did not arrive —"
rm -f "$WORK"/stage/*.gpg "$WORK"/remote/*.gpg
cat > "$WORK/bin/rclone" <<'R'
#!/usr/bin/env bash
case "$1" in copy) exit 0 ;; lsf) exit 1 ;; esac
R
chmod +x "$WORK/bin/rclone"
out=$(bash "$S" 2>&1); rc=$?
ck "a copy that reports success but lands nothing is caught" "$([ $rc -ne 0 ] && echo caught || echo missed)" "caught"
ck "and names the problem" "$(echo "$out" | grep -ci 'not there')" "1"

echo; echo "$pass passed, $fail failed"; [ "$fail" -eq 0 ]
