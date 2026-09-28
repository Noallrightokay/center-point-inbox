#!/usr/bin/env bash
# Proves backup.sh, open-backup.sh and check-restore.sh work, without
# touching a real database or the network.
#
# A backup script that has never been run is a belief. This runs the real
# ones end to end: first against a stubbed `docker compose exec ... pg_dump`
# and a stubbed rclone, with real gzip, gpg and age, so the encryption, the
# refusals, the upload check and the opening are genuinely exercised; then,
# when a Postgres server is installed here, against a real Postgres shaped
# like Supabase's, running the restore drill from the README for real:
# database.sql into a fresh database, the dump loaded, counts checked.
#
#   bash infrastructure/backup/selftest.sh
#
# Needs gpg, gzip and tar; age for the age cases; a Postgres server
# (initdb, pg_ctl) for the last block, which says SKIP without one.
set -u
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
S="$HERE/backup.sh"
OPEN="$HERE/open-backup.sh"
CHECK="$HERE/check-restore.sh"
SCHEMA="$HERE/../../rata-next/database.sql"

WORK="$(mktemp -d)"
PGT=""
cleanup() {
  if [ -n "$PGT" ] && [ -f "$PGT/data/postmaster.pid" ]; then
    $AS_PG "$PGBIN/pg_ctl" -D "$PGT/data" -m immediate stop >/dev/null 2>&1
  fi
  [ -n "$PGT" ] && rm -rf "$PGT"
  GNUPGHOME="$WORK/gnupg" gpgconf --kill gpg-agent >/dev/null 2>&1
  rm -rf "$WORK"
}
trap cleanup EXIT
mkdir -p "$WORK"/{bin,stage,remote,compose,gnupg,open}
chmod 700 "$WORK/gnupg"
export GNUPGHOME="$WORK/gnupg"

REAL_GPG="$(command -v gpg)"
cp "$HERE/stubs/docker" "$WORK/bin/docker"
# gpg, as itself, but noting every command line it was given: the passphrase
# must never be on one, where `ps` shows it to every user on the box.
cat > "$WORK/bin/gpg" <<G
#!/usr/bin/env bash
echo "\$*" >> "$WORK/gpg-argv"
exec "$REAL_GPG" "\$@"
G
# curl for the heartbeat: note the address, send nothing.
cat > "$WORK/bin/curl" <<C
#!/usr/bin/env bash
for a in "\$@"; do case "\$a" in http*) echo "\$a" >> "$WORK/pings" ;; esac; done
C
chmod +x "$WORK"/bin/*

export PATH="$WORK/bin:$PATH"
PASS='correct-horse-battery-staple-not-a-real-one'
HB='https://hc-ping.example/7f3c-HEARTBEAT-SECRET-UUID'
export BACKUP_PASSPHRASE="$PASS"
export RCLONE_REMOTE="remote:$WORK/remote"
export COMPOSE_DIR="$WORK/compose"
export STAGE="$WORK/stage"
export RATA_BACKUP_ENV="$WORK/no-such-env-file"
export STUB_ARGS="$WORK/docker-args"
export KEEP_DAYS=14

# Restore a working rclone every run: some cases deliberately break it, and
# leaving it broken meant the next run started poisoned, which is what made
# the script look guilty when the harness was at fault.
good_rclone(){ cat > "$WORK/bin/rclone" <<'R'
#!/usr/bin/env bash
case "$1" in
  copy) cp "$2" "${3#*:}/" ;;
  lsf)  shift; f=""; [ "$1" = --format ] && { f="$2"; shift 2; }
        p="${1#*:}"; [ -f "$p" ] || exit 1
        if [ "$f" = s ]; then stat -c %s "$p"; else basename "$p"; fi ;;
esac
R
chmod +x "$WORK/bin/rclone"; }
fresh(){ rm -rf "$WORK"/stage/* "$WORK"/remote/* "$WORK"/open/* "$WORK/pings" "$WORK/gpg-argv" "$STUB_ARGS"; good_rclone; }

pass=0; fail=0
ck(){ if [ "$2" = "$3" ]; then echo "  PASS  $1"; pass=$((pass+1)); else echo "  FAIL  $1 (got '$2' want '$3')"; fail=$((fail+1)); fi; }
shipped(){ ls "$WORK"/remote/rata-*.tar 2>/dev/null | wc -l | tr -d ' '; }
count(){ grep -c -- "$1" "$2" 2>/dev/null; true; }

echo "- a normal night (gpg) -"
fresh
out=$(HEARTBEAT_URL="$HB" bash "$S" 2>&1); rc=$?
ck "the script exits clean" "$rc" "0"
ck "one backup reached the remote" "$(shipped)" "1"
f=$(ls "$WORK"/remote/rata-*.tar | head -1)
ck "nothing readable in it: no row" "$(count 'example.com' "$f")" "0"
ck "nothing readable in it: no canary" "$(count CANARY "$f")" "0"
ck "three encrypted parts in the tar" "$(tar -tf "$f" | grep -c '\.gpg$')" "3"
ck "the passphrase is not in the log" "$(echo "$out" | grep -c "$PASS")" "0"
ck "the passphrase was never on gpg's command line" "$(count "$PASS" "$WORK/gpg-argv")" "0"
ck "the heartbeat address is not in the log" "$(echo "$out" | grep -c 'HEARTBEAT-SECRET')" "0"
ck "the heartbeat heard start, then success" "$(tr '\n' ' ' < "$WORK/pings")" "$HB/start $HB "
ck "the log names the counts" "$(echo "$out" | grep -c 'auth.users=50 public.workspaces=50 public.subscriptions=50 public.ai_usage=1')" "1"
ck "dumped as supabase_admin" "$(grep -c -- '--username supabase_admin' "$STUB_ARGS")" "2"
ck "auth and public, data only" "$(grep -- '--data-only' "$STUB_ARGS" | grep -c -- '--schema auth --schema public')" "1"
ck "auth.schema_migrations left out" "$(grep -c -- '--exclude-table auth.schema_migrations' "$STUB_ARGS")" "1"
ck "the retired credential tables left out of both" "$(grep -- '--exclude-table public.provider_tokens --exclude-table public.link_states' "$STUB_ARGS" | wc -l | tr -d ' ')" "2"
ck "the staged copy is readable by root alone" "$(stat -c %a "$(ls "$WORK"/stage/rata-*.tar)")" "600"
ck "no plaintext or work files left behind" "$(find "$WORK/stage" -mindepth 1 ! -name 'rata-*.tar' | wc -l | tr -d ' ')" "0"

echo "- and it opens, which is the only thing that matters -"
out=$(bash "$OPEN" "$f" "$WORK/open" 2>&1); rc=$?
ck "open-backup exits clean" "$rc" "0"
d=$(ls -d "$WORK"/open/rata-* | head -1)
ck "the canary survived encrypt -> ship -> decrypt" "$(count CANARY "$d/data.sql")" "1"
ck "the manifest counted auth.users" "$(grep -c '^table auth.users 50$' "$d/manifest.txt")" "1"
ck "and public.workspaces" "$(grep -c '^table public.workspaces 50$' "$d/manifest.txt")" "1"
ck "and public.subscriptions" "$(grep -c '^table public.subscriptions 50$' "$d/manifest.txt")" "1"
ck "and public.ai_usage" "$(grep -c '^table public.ai_usage 1$' "$d/manifest.txt")" "1"
ck "data.sql loads with triggers off, as Supabase's does" "$(head -1 "$d/data.sql")" "SET session_replication_role = replica;"
ck "psql's \\restrict lines are commented out" "$(grep -c '^\\restrict' "$d/data.sql")/$(grep -c '^-- \\restrict' "$d/data.sql")" "0/1"
ck "schema.sql came too" "$(grep -c 'CREATE TABLE' "$d/schema.sql")" "2"
ck "the opened files are yours alone" "$(stat -c %a "$d/data.sql")" "600"
out=$(BACKUP_PASSPHRASE=wrong bash "$OPEN" "$f" "$WORK/open/again" 2>&1 </dev/null); rc=$?
ck "a wrong passphrase opens nothing" "$([ $rc -ne 0 ] && echo refused || echo opened)" "refused"

echo "- the same with age, where the box holds only the public key -"
if command -v age >/dev/null && command -v age-keygen >/dev/null; then
  fresh
  age-keygen -o "$WORK/id.txt" 2>/dev/null
  pub=$(age-keygen -y "$WORK/id.txt")
  out=$(BACKUP_PASSPHRASE='' BACKUP_AGE_RECIPIENT="$pub" bash "$S" 2>&1); rc=$?
  ck "the script exits clean" "$rc" "0"
  f=$(ls "$WORK"/remote/rata-*.tar | head -1)
  ck "three age parts in the tar" "$(tar -tf "$f" | grep -c '\.age$')" "3"
  out=$(AGE_IDENTITY="$WORK/id.txt" bash "$OPEN" "$f" "$WORK/open" 2>&1); rc=$?
  d=$(ls -d "$WORK"/open/rata-* | head -1)
  ck "the canary survived with the identity" "$(count CANARY "$d/data.sql")" "1"
  age-keygen -o "$WORK/other.txt" 2>/dev/null
  out=$(AGE_IDENTITY="$WORK/other.txt" bash "$OPEN" "$f" "$WORK/open/again" 2>&1); rc=$?
  ck "another identity opens nothing" "$([ $rc -ne 0 ] && echo refused || echo opened)" "refused"
else
  echo "  SKIP  age is not installed"
fi

echo "- a dump that is not whole is a failure, not a success -"
fresh
out=$(TINY=1 HEARTBEAT_URL="$HB" bash "$S" 2>&1); rc=$?
ck "an empty dump: refused" "$([ $rc -ne 0 ] && echo refused || echo accepted)" "refused"
ck "and says so" "$(echo "$out" | grep -c 'refusing')" "1"
ck "and tells the heartbeat it failed" "$(tail -1 "$WORK/pings")" "$HB/fail"
ck "nothing was shipped" "$(shipped)" "0"
fresh
out=$(TRUNCATE=1 bash "$S" 2>&1); rc=$?
ck "a dump cut off mid-table, exit code 0: refused" "$([ $rc -ne 0 ] && echo refused || echo accepted)" "refused"
ck "and says why" "$(echo "$out" | grep -c 'did not finish')" "1"
ck "nothing was shipped" "$(shipped)" "0"
fresh
out=$(FAIL_DUMP=1 bash "$S" 2>&1); rc=$?
ck "pg_dump failing halfway: refused" "$([ $rc -ne 0 ] && echo refused || echo accepted)" "refused"
ck "and says which step" "$(echo "$out" | grep -c 'FAILED: the data dump failed')" "1"
ck "with pg_dump's own words above it" "$(echo "$out" | grep -c 'server closed the connection')" "1"
ck "nothing was shipped" "$(shipped)" "0"
fresh
out=$(NO_AI_USAGE=1 bash "$S" 2>&1); rc=$?
ck "a database without ai_usage: refused" "$([ $rc -ne 0 ] && echo refused || echo accepted)" "refused"
ck "naming the table and database.sql" "$(echo "$out" | grep -c 'no public.ai_usage table.*database.sql')" "1"
ck "nothing was shipped" "$(shipped)" "0"
ck "no work files left behind after a failure" "$(ls -A "$WORK/stage" | wc -l | tr -d ' ')" "0"

echo "- keys and settings -"
fresh
out=$(BACKUP_PASSPHRASE='' bash "$S" 2>&1); rc=$?
ck "no key at all: refused" "$([ $rc -ne 0 ] && echo refused || echo accepted)" "refused"
ck "and says what to set" "$(echo "$out" | grep -c 'BACKUP_AGE_RECIPIENT (preferred) or BACKUP_PASSPHRASE')" "1"
printf 'BACKUP_PASSPHRASE=%s\n' "$PASS" > "$WORK/env"; chmod 644 "$WORK/env"
out=$(BACKUP_PASSPHRASE='' RATA_BACKUP_ENV="$WORK/env" bash "$S" 2>&1); rc=$?
ck "an env file others can read: refused" "$([ $rc -ne 0 ] && echo refused || echo accepted)" "refused"
ck "without printing what is in it" "$(echo "$out" | grep -c "$PASS")" "0"
chmod 600 "$WORK/env"
out=$(BACKUP_PASSPHRASE='' RATA_BACKUP_ENV="$WORK/env" bash "$S" 2>&1); rc=$?
ck "the same file at 600: read, and the night succeeds" "$rc" "0"

echo "- an upload that did not arrive, or arrived short -"
fresh
cat > "$WORK/bin/rclone" <<'R'
#!/usr/bin/env bash
case "$1" in copy) exit 0 ;; lsf) exit 1 ;; esac
R
out=$(bash "$S" 2>&1); rc=$?
ck "a copy that reports success but lands nothing is caught" "$([ $rc -ne 0 ] && echo caught || echo missed)" "caught"
ck "and names the problem" "$(echo "$out" | grep -c 'not there')" "1"
fresh
cat > "$WORK/bin/rclone" <<'R'
#!/usr/bin/env bash
case "$1" in
  copy) head -c 100 "$2" > "${3#*:}/$(basename "$2")" ;;
  lsf)  shift 3; stat -c %s "${1#*:}" ;;
esac
R
out=$(bash "$S" 2>&1); rc=$?
ck "a copy that lands short is caught" "$([ $rc -ne 0 ] && echo caught || echo missed)" "caught"
ck "and says how short" "$(echo "$out" | grep -c 'arrived as 100 bytes')" "1"

echo "- open-backup unpacks only what backup.sh makes -"
mkdir -p "$WORK/evil/rata-2026-01-01T000000Z"
echo x > "$WORK/evil/rata-2026-01-01T000000Z/manifest.txt.gpg"
echo x > "$WORK/evil/escape.sh"
tar -C "$WORK/evil" -cf "$WORK/evil.tar" rata-2026-01-01T000000Z escape.sh
out=$(bash "$OPEN" "$WORK/evil.tar" "$WORK/open" 2>&1 </dev/null); rc=$?
ck "a tar with anything else in it is refused" "$([ $rc -ne 0 ] && echo refused || echo opened)" "refused"
ck "and nothing is unpacked" "$(ls -A "$WORK/open" | wc -l | tr -d ' ')" "0"

echo "- the restore drill, against a real Postgres shaped like Supabase's -"
PGBIN="${PG_BIN:-$(ls -d /usr/lib/postgresql/*/bin 2>/dev/null | sort -V | tail -1)}"
AS_PG=""
if [ "$(id -u)" = 0 ] && id postgres >/dev/null 2>&1 && command -v runuser >/dev/null; then AS_PG="runuser -u postgres --"; fi
if [ -z "$PGBIN" ] || [ ! -x "$PGBIN/initdb" ]; then
  echo "  SKIP  no Postgres server here (initdb); set PG_BIN to its bin directory to run this block"
elif [ "$(id -u)" = 0 ] && [ -z "$AS_PG" ]; then
  echo "  SKIP  running as root with no postgres user to start the server as"
else
  PGT="$(mktemp -d)"; [ -n "$AS_PG" ] && chown postgres "$PGT"
  $AS_PG "$PGBIN/initdb" -D "$PGT/data" -U supabase_admin --auth=trust -E UTF8 >/dev/null 2>&1
  $AS_PG "$PGBIN/pg_ctl" -D "$PGT/data" -l "$PGT/log" -w \
    -o "-k $PGT -c listen_addresses= -c fsync=off" start >/dev/null 2>&1
  export PGHOST="$PGT" PGUSER=supabase_admin PGOPTIONS='-c client_min_messages=warning'
  q(){ "$PGBIN/psql" -X -A -t -q -v ON_ERROR_STOP=1 "$@"; }
  # What a fresh Supabase project has before database.sql: its roles, an auth
  # schema GoTrue made (the columns RATA's rows use), GoTrue's own migration
  # history, and the two auth functions database.sql's policies call.
  skeleton(){ q -d "$1" <<'SQL'
create schema auth;
create table auth.users (instance_id uuid, id uuid primary key, aud varchar(255), role varchar(255),
  email varchar(255), encrypted_password varchar(255), raw_user_meta_data jsonb, created_at timestamptz default now());
create table auth.identities (provider_id text not null, user_id uuid not null references auth.users (id) on delete cascade,
  identity_data jsonb not null, provider text not null, id uuid primary key default gen_random_uuid());
create table auth.schema_migrations (version varchar(255) primary key);
insert into auth.schema_migrations values ('20240101000000');
create function auth.uid() returns uuid language sql stable as $$ select nullif(current_setting('request.jwt.claim.sub', true), '')::uuid $$;
create function auth.jwt() returns jsonb language sql stable as $$ select coalesce(nullif(current_setting('request.jwt.claims', true), ''), '{}')::jsonb $$;
SQL
  }
  q -d postgres -c "create role anon nologin; create role authenticated nologin; create role service_role nologin bypassrls;"
  skeleton postgres
  q -d postgres -f "$SCHEMA" >/dev/null
  q -d postgres >/dev/null <<'SQL'
insert into auth.users (instance_id, id, aud, role, email, encrypted_password, raw_user_meta_data)
  select '00000000-0000-0000-0000-000000000000', gen_random_uuid(), 'authenticated', 'authenticated',
         'user' || g || '@example.com', '$2a$10$' || md5(g::text), '{}' from generate_series(1, 40) g;
insert into auth.users (instance_id, id, aud, role, email, encrypted_password, raw_user_meta_data)
  values ('00000000-0000-0000-0000-000000000000', gen_random_uuid(), 'authenticated', 'authenticated',
          'drill@example.com', '$2a$10$drilldrilldrilldrilldrilldrilldrilldrilldrilldrilldr', '{"name": "Zoë O''Brien"}');
insert into auth.identities (provider_id, user_id, identity_data, provider)
  select id::text, id, jsonb_build_object('sub', id::text, 'email', email), 'email' from auth.users;
insert into public.workspaces (id, data)
  select id, jsonb_build_object('v', 6, 'settings', jsonb_build_object('notify', 'full'),
         'folders', jsonb_build_array('Clients', 'Invoices'), 'rules', '[]'::jsonb,
         'linked', jsonb_build_array(jsonb_build_object('label', email))) from auth.users;
-- the rows a line-by-line counter could get wrong: a tab, a newline, a
-- backslash, a lone "\.", and text that is not ASCII
update public.workspaces set data = data || jsonb_build_object('folders',
  jsonb_build_array(E'Tab\there', E'Line\nbreak', E'Back\\slash', E'\\.', '日本語 ✓'))
  where id = (select id from auth.users where email = 'drill@example.com');
insert into public.subscriptions (email, plan, status, stripe_customer)
  select email, 'pro', 'active', 'cus_' || left(md5(email), 10) from auth.users where email like 'user1%';
insert into public.subscriptions (email, plan, status, stripe_customer, event_at)
  values ('drill@example.com', 'base', 'active', 'cus_drill', now());
select public.ai_charge('drill@example.com', '2026-09', 1234);
select public.ai_charge('drill@example.com', '2026-09', 1000);
-- the retired table, still standing on an old database
create table public.provider_tokens (user_id uuid, provider text, access text);
insert into public.provider_tokens values (gen_random_uuid(), 'mail', 'v1.RETIRED-CIPHERTEXT-MUST-NOT-BE-BACKED-UP');
SQL
  mkdir -p "$WORK/pgbin"
  printf '#!/usr/bin/env bash\nshift 4\nexec "$@"\n' > "$WORK/pgbin/docker"; chmod +x "$WORK/pgbin/docker"
  fresh
  out=$(PATH="$WORK/pgbin:$PGBIN:$PATH" bash "$S" 2>&1); rc=$?
  ck "backup.sh against real Postgres exits clean" "$rc" "0"
  [ $rc -eq 0 ] || echo "$out" | sed 's/^/        /'
  f=$(ls "$WORK"/remote/rata-*.tar 2>/dev/null | head -1)
  bash "$OPEN" "$f" "$WORK/open" >/dev/null 2>&1
  d=$(ls -d "$WORK"/open/rata-* 2>/dev/null | head -1)
  ck "real pg_dump wrote \\restrict, and it was commented out" "$(grep -c '^-- \\restrict' "$d/data.sql")" "1"
  ck "the retired ciphertext is not in the backup" "$(cat "$d"/*.sql | grep -c RETIRED-CIPHERTEXT)" "0"
  ck "nor is provider_tokens in the manifest" "$(grep -c provider_tokens "$d/manifest.txt")" "0"
  ck "nor GoTrue's migration history" "$(grep -c schema_migrations "$d/data.sql")" "0"
  out=$(PGDATABASE=postgres bash "$CHECK" "$d/manifest.txt" 2>&1); rc=$?
  ck "the manifest's counts equal the live database's" "$rc" "0"
  ck "the manifest has auth.users 41" "$(grep -c '^table auth.users 41$' "$d/manifest.txt")" "1"

  # README drill steps 5-7: a fresh project, database.sql, the data, the counts.
  q -d postgres -c "create database scratch"
  skeleton scratch
  q -d scratch -f "$SCHEMA" >/dev/null
  q -d scratch --single-transaction -f "$d/data.sql" >"$WORK/load.log" 2>&1; rc=$?
  ck "data.sql loads into a fresh database after database.sql" "$rc" "0"
  [ $rc -eq 0 ] || sed 's/^/        /' "$WORK/load.log" | head -20
  out=$(PGDATABASE=scratch bash "$CHECK" "$d/manifest.txt" 2>&1); rc=$?
  ck "check-restore: every table matches" "$rc" "0"
  [ $rc -eq 0 ] || echo "$out" | sed 's/^/        /'
  ws(){ q -d "$1" -c "select string_agg(md5(data::text), ',' order by id) from public.workspaces"; }
  ck "every workspace came back byte for byte" "$(ws scratch)" "$(ws postgres)"
  pw(){ q -d "$1" -c "select encrypted_password from auth.users where email = 'drill@example.com'"; }
  ck "the test account's password hash came back, so it can sign in" "$(pw scratch)" "$(pw postgres)"
  ck "the licence lookup finds the test subscription (plan/status)" \
     "$(q -d scratch -c "select plan || '/' || status from public.subscriptions where email = 'drill@example.com'")" "base/active"
  ck "the AI cap carries on from the month's total" \
     "$(q -d scratch -c "select public.ai_charge('drill@example.com', '2026-09', 1)")" "2235"
  ck "the scratch project's own migration history is untouched" \
     "$(q -d scratch -c "select count(*) from auth.schema_migrations")" "1"
  q -d scratch -c "delete from public.subscriptions where email = 'user10@example.com'"
  out=$(PGDATABASE=scratch bash "$CHECK" "$d/manifest.txt" 2>&1); rc=$?
  ck "a restore missing one row fails the check" "$([ $rc -ne 0 ] && echo caught || echo missed)" "caught"
  ck "and names the table" "$(echo "$out" | grep -c 'public.subscriptions.*MISMATCH')" "1"
fi

echo; echo "$pass passed, $fail failed"; [ "$fail" -eq 0 ]
