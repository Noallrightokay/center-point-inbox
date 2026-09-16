#!/usr/bin/env bash
# RATA — nightly Postgres backup, encrypted, sent off the box.
#
# A backup that lives on the machine it is backing up is not a backup. The whole
# point is the case where that disk is gone, so every run ends with the copy
# somewhere else.
#
# Install:
#   cp backup.sh /usr/local/bin/rata-backup && chmod +x /usr/local/bin/rata-backup
#   crontab -e   ->   17 3 * * *  /usr/local/bin/rata-backup >> /var/log/rata-backup.log 2>&1
#
# Needs: BACKUP_PASSPHRASE (for the encryption), RCLONE_REMOTE (e.g. b2:rata-backups),
# and HEARTBEAT_URL (optional, see the bottom). Put them in /etc/rata-backup.env,
# chmod 600, owned by root.

set -Eeuo pipefail

[ -f /etc/rata-backup.env ] && . /etc/rata-backup.env

COMPOSE_DIR="${COMPOSE_DIR:-/root/supabase/docker}"
DB_SERVICE="${DB_SERVICE:-db}"
DB_USER="${DB_USER:-postgres}"
STAGE="${STAGE:-/var/backups/rata}"
KEEP_DAYS="${KEEP_DAYS:-14}"

: "${BACKUP_PASSPHRASE:?set BACKUP_PASSPHRASE in /etc/rata-backup.env}"
: "${RCLONE_REMOTE:?set RCLONE_REMOTE in /etc/rata-backup.env}"

STAMP="$(date -u +%Y-%m-%dT%H%M%SZ)"
OUT="$STAGE/rata-$STAMP.sql.gz.gpg"
mkdir -p "$STAGE"

fail() { echo "[$(date -u +%FT%TZ)] FAILED: $*" >&2; exit 1; }
trap 'fail "line $LINENO"' ERR

echo "[$(date -u +%FT%TZ)] dumping"

# --clean --if-exists so a restore into a non-empty database works rather than
# erroring halfway and leaving a half-restored one.
#
# Piped straight into gzip and gpg: the plaintext dump never touches the disk,
# which matters because it contains every account and every encrypted mailbox
# credential in one file.
cd "$COMPOSE_DIR"
docker compose exec -T "$DB_SERVICE" \
  pg_dumpall --clean --if-exists --username "$DB_USER" \
  | gzip -9 \
  | gpg --batch --yes --symmetric --cipher-algo AES256 \
        --passphrase "$BACKUP_PASSPHRASE" -o "$OUT"

SIZE=$(stat -c%s "$OUT")
# A dump that produced nothing is a failure that looks like a success. The
# floor is deliberately low, because it is guarding against an empty pipe and
# not against a small database: measured through this exact pipeline, an empty
# pipe encrypts to 86 bytes and a valid-but-empty cluster to 172, while a real
# RATA with fifty users is 8-20 KB. A floor set for a mature database would
# reject every legitimate backup until enough customers existed — failing
# nightly from launch, which is how an operator learns to ignore this job.
#
# The first line of defence is `pipefail` at the top: pg_dumpall exiting
# non-zero already aborts. This only catches "exited 0 and wrote nothing".
MIN_BYTES="${MIN_BYTES:-1024}"
[ "$SIZE" -gt "$MIN_BYTES" ] || fail "dump is only $SIZE bytes; refusing to call that a backup"

echo "[$(date -u +%FT%TZ)] $OUT ($SIZE bytes) — uploading"
rclone copy "$OUT" "$RCLONE_REMOTE/" --contimeout 30s --retries 3

# Prove it arrived rather than assuming the exit code meant it did.
rclone lsf "$RCLONE_REMOTE/$(basename "$OUT")" >/dev/null || fail "upload reported success but the file is not there"

find "$STAGE" -name 'rata-*.sql.gz.gpg' -mtime "+$KEEP_DAYS" -delete

# Optional dead-man's switch: a cron job that stops running is silent, and
# silence is indistinguishable from success. Point this at a healthchecks.io
# (or similar) URL and get told when a night is MISSED, not just when one fails.
[ -n "${HEARTBEAT_URL:-}" ] && curl -fsS -m 10 "$HEARTBEAT_URL" >/dev/null || true

echo "[$(date -u +%FT%TZ)] done"
