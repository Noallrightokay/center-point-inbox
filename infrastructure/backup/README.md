# Backups, and getting back from one

Everything RATA's server holds for a customer is in one Postgres: the
self-hosted Supabase behind `db.mailrata.org`, in Docker on one VPS. It is
small, and it is what turns "paid" into "licensed".

| Where | What (from `rata-next/database.sql` and the code that writes it) | If it were lost |
|---|---|---|
| `auth.users` and the rest of the `auth` schema | website logins, kept by Supabase Auth: address, password hash, identities, sessions | nobody can sign in to `/account` to get or renew a licence |
| `public.subscriptions` | one row per paying address: plan, status, Stripe customer id, add-ons, when Stripe last wrote it | licence issue (`/api/licence`) and renewal read it; nobody could renew |
| `public.pending_subscriptions` | a Stripe subscription event that arrived before its checkout, keyed by Stripe customer id (plan, status, when; no address), deleted once the checkout applies it | a checkout mid-flight lands its plan on Stripe's next subscription event instead of at once |
| `public.ai_usage` | per address and month, what AI cost and how many requests: a number, never text | this month's AI totals start again at zero, nothing worse |
| `public.workspaces` | per account, the synced `settings`, `folders`, `rules` and `linked` mailbox entries (`CLOUD_FIELDS` in `app.html`); signatures and API keys are stripped before upload (`CLOUD_STRIP`) | a customer's settings on a new device |

There is no mail and no mail password in it. Mailbox passwords are in each
customer's own OS keychain and mail stays at their provider; the desktop app
never sends either to RATA. A database set up before the desktop app may
still hold the retired `provider_tokens` and `link_states` tables
(`database.sql` section 4): old mailbox-credential ciphertext that nothing
reads. **`backup.sh` leaves them out**, so they are in no backup; drop them
when you are ready (section 4 says how).

What *is* in a backup is still sensitive: every customer's address, the
bcrypt hash of their website password, their sessions, their Stripe customer
id and their folder and rule names. That is why each backup is encrypted
before it leaves the database container, and why the key that opens it is
kept off the VPS.

## What `backup.sh` makes

One file a night, `rata-<stamp>.tar` (UTC stamp), holding three encrypted
parts:

| Part | What |
|---|---|
| `data.sql.gz.age` (or `.gpg`) | every row of the `auth` and `public` schemas, written the way the Supabase CLI's `supabase db dump --data-only` writes it, so it loads into any Supabase project after `database.sql` |
| `schema.sql.gz.age` | the live `public` schema, for comparing with `database.sql` if a restore ever disagrees with it |
| `manifest.txt.age` | the row count of every table in `data.sql`, counted from the dump itself; the restore is checked against it |

It refuses to ship, fails loudly, and tells the heartbeat (below) when:
`pg_dump` fails; the dump lacks the closing line every complete `pg_dump`
writes (a dump cut off mid-table, whatever exit code came back); a table
RATA needs is missing (`auth.users`, `public.workspaces`,
`public.subscriptions`, `public.ai_usage`: if `ai_usage` is the one, run
`database.sql`); the upload is missing or arrived at a different size. It
never prints a secret: the passphrase reaches gpg on a file descriptor, not
its command line, and the log carries only table names and row counts.

Why `supabase_admin` and not `postgres`, and why not the CLI itself, is
explained at the top of `backup.sh`: in current Supabase images `postgres` is
not a superuser, and the CLI's data dump is `pg_dump` with a fixed recipe
that the script runs with the `pg_dump` already inside the container.

## Setting it up (owner, on the VPS, once)

1. **Somewhere off the box.** Any rclone remote. Backblaze B2 is cheap and
   simple: make a private bucket (`rata-backups`), then an application key
   limited to that bucket **without `deleteFiles`**, so a thief on the VPS
   can add backups but not destroy them. Give the bucket a lifecycle rule
   that hides files 90 days after upload and deletes them a day later
   (`daysFromUploadingToHiding` 90, `daysFromHidingToDeleting` 1): backups
   then prune themselves, and 90 days is how long a deleted account lingers
   in them. `apt install rclone && rclone config` (name the remote `b2`).
   Check: `rclone lsf b2:rata-backups` answers without an error.
2. **A key.** Preferred: age, so the VPS holds only the public half and a
   stolen box opens no backup. On your own computer, not the VPS:
   `age-keygen -o rata-backup-identity.txt`. It prints `Public key: age1...`.
   Put the identity file in your password manager and delete it from disk.
   On the VPS: `apt install age`. (Or gpg: a long random passphrase, kept in
   the password manager. The VPS then holds what opens every backup.)
3. **The settings file**, which the script refuses to read if anyone but
   root can:
   ```bash
   install -m 600 /dev/null /etc/rata-backup.env
   nano /etc/rata-backup.env
   ```
   ```
   RCLONE_REMOTE=b2:rata-backups
   BACKUP_AGE_RECIPIENT=age1...                 # the public key from step 2
   HEARTBEAT_URL=https://hc-ping.com/<uuid>     # step 5
   ```
   If Supabase's compose file is not in `/root/supabase/docker`, add
   `COMPOSE_DIR=` with where it is.
4. **The script and the nightly run:**
   ```bash
   install -m 700 infrastructure/backup/backup.sh /usr/local/bin/rata-backup
   timedatectl | grep 'Time zone'      # expect UTC; otherwise the line below runs at 03:17 local time
   echo '17 3 * * *  root  flock -n /run/rata-backup.lock /usr/local/bin/rata-backup >> /var/log/rata-backup.log 2>&1' \
     > /etc/cron.d/rata-backup
   ```
   03:17 UTC: the middle of the night in Europe and evening in the Americas,
   off the hour when every other cron job runs. `flock` stops a slow night
   overlapping the next.
5. **A heartbeat** at [healthchecks.io](https://healthchecks.io) (free: 20
   checks): a check named "RATA backup", period 1 day, grace 2 hours, alerts
   to your email. Paste its ping URL as `HEARTBEAT_URL`. The script pings
   `/start` when it begins, the URL itself when it succeeds and `/fail` when
   it refuses, so a failed night alerts at once and a *missed* night (cron
   stopped, box down) alerts after the grace period.
6. **Run it once by hand** and read the last line:
   ```bash
   /usr/local/bin/rata-backup
   # [..] /var/backups/rata/rata-<stamp>.tar (N bytes; auth.users=.. public.workspaces=.. ...); uploading
   # [..] done
   rclone lsf b2:rata-backups
   ```
   Then do the drill below once, before trusting any of it.

## The licence key is not in the backup, and must not be

The other thing the business cannot lose is the **licence signing key**:
the Ed25519 private key at `/etc/rata/licence.key` on the VPS, which the
website holds as `LICENCE_PRIVATE_KEY` (see `rata-next/LICENSING.md`). It
lives in a file and in the host's environment, not in the database, so a
database backup does not contain it. Two consequences, and they pull in
opposite directions:

- **Lose the key and a restored database cannot issue a licence.** The
  subscriptions come back, but no licence can be issued or renewed until a
  new key is made and a new app build ships with its public half, which
  every customer then has to install. Licences already issued keep working
  until they expire, at most thirty days.
- **Store the key with the backup and one stolen file lets anyone mint
  licences** for any address and plan, until you rotate the key and ship
  that new build anyway.

So keep a copy of `/etc/rata/licence.key` somewhere durable and *separate*:
a password manager, a sealed envelope, anywhere that is not the backup
bucket. The same goes for the age identity (or `BACKUP_PASSPHRASE`): a
backup nobody can decrypt is not a backup. Drill step 11 proves the key copy.

## The restore drill (monthly)

Untested restores are not backups. Do this once now, then on the first of
every month, and after any change to `database.sql` or the Supabase version.
It restores into a **scratch** project: never point any of it at
`db.mailrata.org`.

It uses a *test account*: an address you control with a subscription in
production. Make it once: sign up at `https://mailrata.org/account` with,
say, `you+drill@yourdomain`, keep its password in your password manager,
and in production's SQL editor give it a plan:
`insert into public.subscriptions (email, plan, status) values ('you+drill@yourdomain', 'base', 'active') on conflict (email) do nothing;`

1. **A scratch Supabase project.** Either a free project at supabase.com
   named `rata-drill` (delete it afterwards), or `supabase start` on your
   own computer with Docker (its database is `127.0.0.1:54322`, user and
   password `postgres`, the CLI's local defaults). Note its API URL, anon
   key and service_role key: they are the scratch project's own, never
   production's.
2. **Fetch last night's backup** on your own computer (with `rclone config`
   for the same bucket, read access is enough):
   ```bash
   rclone lsf b2:rata-backups | sort | tail -1          # rata-<stamp>.tar
   rclone copy b2:rata-backups/rata-<stamp>.tar .
   ```
3. **Open it** with the identity from your password manager, saved to a file
   for the moment:
   ```bash
   AGE_IDENTITY=./rata-backup-identity.txt infrastructure/backup/open-backup.sh rata-<stamp>.tar
   # gpg backups: it asks for the passphrase instead
   ```
   It prints each table and its row count, and leaves
   `rata-<stamp>/{manifest.txt,data.sql,schema.sql}` readable by you alone.
4. **Point psql at the scratch project**, password typed, not on a command
   line (supabase.com: Connect, *Session pooler*):
   ```bash
   export PGHOST=<scratch host> PGPORT=5432 PGUSER=<scratch user> PGDATABASE=postgres
   read -rs PGPASSWORD && export PGPASSWORD
   echo "$PGHOST"; psql -Xc 'select 1'     # the scratch host, and it answers
   ```
5. **Run `database.sql`:**
   `psql -X -v ON_ERROR_STOP=1 -f rata-next/database.sql`
6. **Load the data**, in one transaction, so a failure leaves nothing half
   done: `psql -X -v ON_ERROR_STOP=1 --single-transaction -f rata-<stamp>/data.sql`.
   If it stops with `column "..." does not exist` in an `auth` table, the
   scratch project's Supabase Auth is older than production's; use a newer
   one (a supabase.com project runs the current release). Compare `rata-<stamp>/schema.sql`
   with `database.sql` if a `public` table is the one that disagrees.
7. **Check the counts against the manifest:**
   `infrastructure/backup/check-restore.sh rata-<stamp>/manifest.txt`.
   Every line must say `ok`, and it ends `all N tables match the backup`.
   Row counts only prove the rows came back; the next steps prove they work.
8. **Run the website against the scratch project**, with a throwaway licence
   key pair (never production's key on a laptop; step 11 proves that one):
   ```bash
   cd rata-next && npm ci && npm run build
   export SUPABASE_URL=<scratch API URL>
   read -rs SUPABASE_ANON_KEY && export SUPABASE_ANON_KEY
   read -rs SUPABASE_SERVICE_ROLE_KEY && export SUPABASE_SERVICE_ROLE_KEY
   eval "$(node --input-type=module -e "import { generateKeys } from './lib/licence.js'; const k = generateKeys(); console.log('export LICENCE_PRIVATE_KEY=' + JSON.stringify(k.privateKey) + ' LICENCE_PUBLIC_KEY=' + JSON.stringify(k.publicKey))")"
   npm start          # http://localhost:3000
   ```
9. **Sign in as the test account** at `http://localhost:3000/account`, with
   its production password. It signs in because its password hash came back
   with `auth.users`, and the page shows a licence key: `/api/licence` found
   the test subscription in the restored `subscriptions` and signed one.
10. **Check that key**, in the same shell as step 8 (paste the key, then
    Ctrl-D):
    ```bash
    node --input-type=module -e "import { check } from './lib/licence.js'; let s = ''; process.stdin.on('data', d => s += d).on('end', () => { const r = check(s.trim()); console.log(r.ok ? 'GOOD: licence for ' + r.licence.sub + ', plan ' + r.licence.plan : 'BAD: ' + r.reason); })"
    ```
    It must say `GOOD: licence for you+drill@yourdomain, plan base`.
11. **Prove the real key copy** (from step "The licence key" above) is the
    pair of the key in shipped apps, wherever that copy is kept:
    `openssl pkey -in licence.key -pubout | diff - licence.pub && echo SAME`,
    where `licence.pub` is the public key in the `RATA_LICENCE_PUBLIC_KEY`
    GitHub secret. `SAME`, or the backup of the key is not the key.
12. **Clean up and record it:** stop `npm start`, delete the scratch project
    (or `supabase stop --no-backup`), `rm -rf rata-<stamp> rata-<stamp>.tar
    rata-backup-identity.txt`, close the shell (it holds the scratch keys).
    Then ping the drill check (below) and note the date, the stamp and the
    counts in `MVP-EVIDENCE.md`.

**The monthly reminder:** a second healthchecks.io check, "RATA restore
drill", period 30 days, grace 7 days. Ping its URL at the end of step 12
(open it in a browser, or `curl -fsS <its URL>`). A month without a drill
emails you; a calendar reminder on the 1st is the nudge before it does.

A real restore after a disaster is the same procedure with a new
production project in place of the scratch one (a fresh self-hosted Supabase
or a supabase.com project), the **real** `LICENCE_PRIVATE_KEY` on the site
from the separate copy, and the site's `SUPABASE_*` variables pointed at it.

## Proving the scripts work

```bash
bash infrastructure/backup/selftest.sh
```

Runs the real `backup.sh`, `open-backup.sh` and `check-restore.sh` with no
network and no production anything. First against a stubbed `docker compose
exec … pg_dump` and a stubbed `rclone`, with real gzip, gpg and age: the
encryption, every refusal above, the heartbeat, the upload check, that the
passphrase is never on a command line or in the log. Then, when a Postgres
server is installed (`initdb`), against a real one shaped like Supabase's:
it runs `database.sql`, fills it (with rows a line counter could get wrong:
tabs, newlines, backslashes, a lone `\.`), backs it up, and performs drill
steps 5 to 7 into a fresh database, including a workspace compared byte for
byte, the test account's password hash, the licence lookup, and the AI total
carrying on. Without a Postgres server that block says SKIP.

## What this does not cover

- **The customers' mail and mail passwords.** They were never RATA's to back
  up: the mail is at their provider and the passwords are in their own
  keychain, which is the entire point of the local-first design.
- **Anything on a customer's device.** Messages, signatures, files and the
  audit chain live there. That is deliberate (see the top of `CLAUDE.md`);
  it also means a customer who loses their laptop loses those, and they
  should be told so plainly rather than discovering it. Their mail is still
  at their provider, and RATA fetches it again.
- **The licence signing key.** See above: back it up yourself, separately.
- **Supabase's other schemas** (`storage`, `realtime` and the rest). RATA
  uses none of them; a new Supabase makes its own.
- **The VPS itself.** This restores the data, not the machine. Hostinger's own
  snapshots cover the box; they are not a substitute for this, because a snapshot
  of a corrupted database is a corrupted database.

Uptime and alerting are in [`../monitoring/README.md`](../monitoring/README.md).
