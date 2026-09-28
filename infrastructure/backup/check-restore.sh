#!/usr/bin/env bash
# Checks a restored database against the backup's manifest: every table the
# dump held must hold exactly as many rows after the restore.
#
#   check-restore.sh rata-<stamp>/manifest.txt
#
# Connects with psql's own environment variables (PGHOST, PGPORT, PGUSER,
# PGDATABASE, and PGPASSWORD typed with `read -rs`), so no password is ever on
# a command line. Point them at the SCRATCH project, never at production.
# Exits 0 only when every count matches.
set -Eeuo pipefail

MANIFEST="${1:?usage: check-restore.sh rata-<stamp>/manifest.txt}"
[ -f "$MANIFEST" ] || { echo "check-restore: no such file: $MANIFEST" >&2; exit 1; }
grep -q '^table ' "$MANIFEST" || { echo "check-restore: $MANIFEST lists no tables" >&2; exit 1; }

bad=0; n=0
printf '%-32s %10s %10s\n' table backup restored
while read -r kind t want; do
  [ "$kind" = table ] || continue
  # The names come from our own manifest, but they are still checked before
  # they go into SQL.
  if ! [[ "$t" =~ ^[a-z_][a-z0-9_]*\.[a-z_][a-z0-9_]*$ ]]; then
    echo "check-restore: refusing odd table name: $t" >&2; bad=1; continue
  fi
  got="$(psql -X -A -t -q -v ON_ERROR_STOP=1 -c "select count(*) from \"${t%%.*}\".\"${t#*.}\"" 2>&1)" || got="error: $got"
  n=$((n + 1))
  if [ "$got" = "$want" ]; then mark=ok; else mark=MISMATCH; bad=1; fi
  printf '%-32s %10s %10s  %s\n' "$t" "$want" "$got" "$mark"
done < "$MANIFEST"

if [ "$bad" -eq 0 ]; then
  echo "all $n tables match the backup"
else
  echo "the restore does NOT match the backup" >&2
  exit 1
fi
