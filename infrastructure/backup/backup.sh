#!/usr/bin/env bash
# RATA: nightly database backup, encrypted, sent off the box.
#
# A backup that lives on the machine it is backing up is not a backup. The
# whole point is the case where that disk is gone, so every run ends with the
# copy somewhere else, and checks that it arrived.
#
# What it takes (see README.md for why each one):
#   data.sql     every row of the `auth` and `public` schemas, in the form the
#                Supabase CLI's own `supabase db dump --data-only` writes:
#                auth.users (website logins), public.workspaces,
#                public.subscriptions, public.ai_usage. Loads into any
#                Supabase project after rata-next/database.sql.
#   schema.sql   the live `public` schema, to compare with database.sql if a
#                restore ever disagrees with it.
#   manifest.txt the row count of every table in data.sql, counted from the
#                dump itself, which is what a restore is checked against.
# Not taken: public.provider_tokens and public.link_states, the retired
# mailbox-credential tables (database.sql section 4). Nothing reads them, and
# copying their ciphertext into every night's backup would only spread it.
#
# Each file is encrypted as it streams out of the database, so no plaintext
# row ever touches this disk; the three are then put in one tar,
# rata-<stamp>.tar, and shipped.
#
# Install (as root on the VPS):
#   install -m 700 backup.sh /usr/local/bin/rata-backup
#   install -m 600 /dev/null /etc/rata-backup.env      # then fill it in
#   /etc/cron.d/rata-backup, one line (03:17 UTC; the box's clock is UTC):
#     17 3 * * *  root  flock -n /run/rata-backup.lock /usr/local/bin/rata-backup >> /var/log/rata-backup.log 2>&1
#
# /etc/rata-backup.env (mode 600, owned by root; the script refuses anything
# looser, because it holds the key to every backup):
#   RCLONE_REMOTE=b2:rata-backups        where the copy goes (an rclone remote)
#   and ONE of:
#   BACKUP_AGE_RECIPIENT=age1...         preferred: the PUBLIC half of an age
#                                        key; the box can encrypt but never
#                                        decrypt, so a stolen box opens nothing
#   BACKUP_PASSPHRASE=...                gpg symmetric (AES256): simpler, but the
#                                        box then holds what opens every backup
#   HEARTBEAT_URL=https://hc-ping.com/…  optional: healthchecks.io check; told on
#                                        start, success and failure
#
# It prints no secret: not the passphrase, not the heartbeat URL, not a row.
# The passphrase goes to gpg on a file descriptor, never on its command line,
# where any user on the box could read it in `ps`.

set -Eeuo pipefail
umask 077

log() { echo "[$(date -u +%FT%TZ)] $*"; }

ENV_FILE="${RATA_BACKUP_ENV:-/etc/rata-backup.env}"
if [ -f "$ENV_FILE" ]; then
  mode="$(stat -c %a "$ENV_FILE")"
  case "$mode" in
    [4567]00) ;;
    *) log "FAILED: $ENV_FILE is mode $mode, readable by others; chmod 600 it. Refusing to read it." >&2; exit 1 ;;
  esac
  # shellcheck disable=SC1090
  . "$ENV_FILE"
fi

COMPOSE_DIR="${COMPOSE_DIR:-/root/supabase/docker}"
DB_SERVICE="${DB_SERVICE:-db}"
# supabase_admin, not postgres: in current Supabase images postgres is not a
# superuser (supabase/postgres migration 10000000000000_demote-postgres.sql),
# and inside the container supabase_admin signs in over the local socket
# without a password (its pg_hba.conf: `local all supabase_admin trust`).
DB_USER="${DB_USER:-supabase_admin}"
DB_NAME="${DB_NAME:-postgres}"
STAGE="${STAGE:-/var/backups/rata}"
KEEP_DAYS="${KEEP_DAYS:-14}"
# Tables that must be in every dump. A dump without one of them is either not
# RATA's database or a database that database.sql has not been run on.
EXPECT_TABLES="${EXPECT_TABLES:-auth.users public.workspaces public.subscriptions public.ai_usage}"

heartbeat() {
  [ -n "${HEARTBEAT_URL:-}" ] || return 0
  curl -fsS -m 10 --retry 2 -o /dev/null "${HEARTBEAT_URL}$1" 2>/dev/null \
    || log "heartbeat ping did not go through (not fatal)" >&2
}

fail() {
  log "FAILED: $*" >&2
  heartbeat /fail
  exit 1
}
# An error inside a pipeline's subshell only has to end that subshell; the
# pipeline then fails in the main script, which reports it once.
on_err() { [ "$BASHPID" = "$$" ] || exit 1; fail "unexpected error at line $1"; }
trap 'on_err $LINENO' ERR

[ -n "${RCLONE_REMOTE:-}" ] || fail "set RCLONE_REMOTE in $ENV_FILE"
if [ -n "${BACKUP_AGE_RECIPIENT:-}" ]; then
  ENC=age
  command -v age >/dev/null || fail "BACKUP_AGE_RECIPIENT is set but age is not installed (apt install age)"
elif [ -n "${BACKUP_PASSPHRASE:-}" ]; then
  ENC=gpg
else
  fail "set BACKUP_AGE_RECIPIENT (preferred) or BACKUP_PASSPHRASE in $ENV_FILE"
fi

# stdin -> encrypted file $1
encrypt() {
  if [ "$ENC" = age ]; then
    age -r "$BACKUP_AGE_RECIPIENT" -o "$1"
  else
    gpg --batch --yes --quiet --pinentry-mode loopback --symmetric --cipher-algo AES256 \
        --passphrase-fd 3 -o "$1" 3< <(printf '%s' "$BACKUP_PASSPHRASE")
  fi
}

in_db() { docker compose exec -T "$DB_SERVICE" "$@"; }

# The Supabase CLI's data dump (`supabase db dump --data-only`, its script
# pkg/migration/scripts/dump_data.sh), run with the pg_dump that is already
# inside the database container and so matches the server's version, instead
# of installing the CLI and a second Postgres image and handing them a
# password on a command line. Same flags, same framing: triggers off while
# loading, psql's \restrict lines commented out so any psql can load it,
# auth.schema_migrations left out because every Supabase project has its own.
dump_data() {
  echo "SET session_replication_role = replica;"
  echo
  in_db pg_dump --username "$DB_USER" --dbname "$DB_NAME" \
      --data-only --quote-all-identifiers \
      --schema auth --schema public \
      --exclude-table auth.schema_migrations \
      --exclude-table public.provider_tokens --exclude-table public.link_states \
    | sed -E 's/^\\(un)?restrict .*$/-- &/' || return 1
  echo "RESET ALL;"
}

dump_schema() {
  in_db pg_dump --username "$DB_USER" --dbname "$DB_NAME" \
      --schema-only --schema public \
      --exclude-table public.provider_tokens --exclude-table public.link_states \
    | sed -E 's/^\\(un)?restrict .*$/-- &/' || return 1
}

# Passes the dump through untouched and, on the way, counts the rows of every
# COPY block and notes whether pg_dump wrote its closing line. A dump that was
# cut off (a dropped connection, a killed process, a full disk inside the
# container) has no closing line, whatever exit code came back with it.
# In COPY's text format a row is one line and no row can be `\.`, so the
# counts are exact.
count_rows() {
  awk -v out="$1" '
    { print }
    /^COPY .* FROM stdin;$/ { t = $2; gsub(/"/, "", t); n = 0; inside = 1; next }
    inside && $0 == "\\." { order[++k] = t; rows[t] = n; inside = 0; next }
    inside { n++ }
    /^-- PostgreSQL database dump complete/ { done = 1 }
    END {
      for (i = 1; i <= k; i++) printf "table %s %d\n", order[i], rows[order[i]] > out
      if (done) print "complete" > out
    }'
}

STAMP="$(date -u +%Y-%m-%dT%H%M%SZ)"
NAME="rata-$STAMP"
mkdir -p "$STAGE"
WORK="$(mktemp -d "$STAGE/.work-XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
mkdir "$WORK/$NAME"
PART="$WORK/$NAME"

heartbeat /start
log "dumping"
cd "$COMPOSE_DIR"
# `|| fail` rather than the ERR trap, so the message says which step; with
# pipefail a failure anywhere in the pipe (pg_dump, gzip, the encryption)
# fails the line. pg_dump's own words are above it in the log.
dump_data   | count_rows "$WORK/data.counts"   | gzip -9 | encrypt "$PART/data.sql.gz.$ENC" \
  || fail "the data dump failed; refusing to call that a backup"
dump_schema | count_rows "$WORK/schema.counts" | gzip -9 | encrypt "$PART/schema.sql.gz.$ENC" \
  || fail "the schema dump failed; refusing to call that a backup"

grep -qx complete "$WORK/data.counts" \
  || fail "the data dump did not finish (no closing line from pg_dump); refusing to call that a backup"
grep -qx complete "$WORK/schema.counts" \
  || fail "the schema dump did not finish (no closing line from pg_dump); refusing to call that a backup"
for t in $EXPECT_TABLES; do
  grep -q "^table $t " "$WORK/data.counts" \
    || fail "the dump has no $t table; refusing to call that a backup. If database.sql has not been run on this database, run it (rata-next/database.sql)"
done

{
  echo "# RATA backup manifest: row counts of every table in data.sql, counted from the dump"
  echo "stamp $STAMP"
  echo "encryption $ENC"
  grep '^table ' "$WORK/data.counts"
} | encrypt "$PART/manifest.txt.$ENC"

OUT="$STAGE/$NAME.tar"
tar -C "$WORK" -cf "$OUT" "$NAME"
SIZE="$(stat -c %s "$OUT")"
SUMMARY="$(awk '/^table (auth\.users|public\.(subscriptions|workspaces|ai_usage)) /{printf "%s%s=%s", s, $2, $3; s=" "}' "$WORK/data.counts")"
log "$OUT ($SIZE bytes; $SUMMARY); uploading"

rclone copy "$OUT" "$RCLONE_REMOTE/" --contimeout 30s --retries 3 \
  || fail "upload failed"
# Prove it arrived, whole, rather than trusting the exit code.
THERE="$(rclone lsf --format s "$RCLONE_REMOTE/$NAME.tar" 2>/dev/null || true)"
[ -n "$THERE" ] || fail "upload reported success but the file is not there"
[ "$THERE" = "$SIZE" ] || fail "upload arrived as $THERE bytes, not $SIZE"

find "$STAGE" -maxdepth 1 -name 'rata-*.tar' -mtime "+$KEEP_DAYS" -delete

# The dead-man's switch: a cron job that stops running is silent, and silence
# looks exactly like success. healthchecks.io pages when this ping is MISSED.
heartbeat ""
log "done"
