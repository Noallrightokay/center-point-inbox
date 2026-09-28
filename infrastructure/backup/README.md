# Backups, and getting back from one

Everything RATA's server holds for a customer is in one Postgres (the
self-hosted Supabase behind `db.mailrata.org`) on one VPS. It is small, and
it is what turns "paid" into "licensed":

| Where | What | Why it matters |
|---|---|---|
| `auth.users` | website logins (Supabase Auth) | a customer signs in to `/account` to get their licence key |
| `public.subscriptions` | one row per paying address: plan, status, Stripe customer id | licence issue and renewal read it; lose it and nobody can renew |
| `public.ai_usage` | per address and month, what AI cost (a number, no text) | the monthly AI cap; losing it resets this month's totals, nothing worse |
| `public.workspaces` | each account's synced preferences, folder names, rules and linked mailbox addresses (no passwords, no mail, no signatures) | a customer's settings on a new device |

There is no mail and no mail password in it. Mailbox passwords are in each
customer's own OS keychain and mail stays at their provider; the desktop app
never sends either to RATA. (A database set up before the desktop app may
still hold the retired `provider_tokens` and `link_states` tables:
ciphertext nothing reads any more. `database.sql` section 4 says how to
drop them. Until you do, they are in every backup too.)

`backup.sh` dumps nightly, encrypts, and ships the copy off the box. Set it up
with the instructions at the top of the script.

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
bucket. The same goes for `BACKUP_PASSPHRASE`: a backup nobody can decrypt
is not a backup.

## Restoring

Untested restores are not backups. Do this once now, and once after any change
to the schema or the Supabase version — on a scratch VPS or a local Docker, never
by pointing it at production to see what happens.

```bash
# 1. Fetch and open the dump
rclone copy "$RCLONE_REMOTE/rata-<stamp>.sql.gz.gpg" .
gpg --batch --passphrase "$BACKUP_PASSPHRASE" -d rata-<stamp>.sql.gz.gpg | gunzip > rata.sql

# 2. Load it into a database that is NOT production
docker compose exec -T db psql --username postgres < rata.sql

# 3. Check the things that matter
docker compose exec -T db psql --username postgres -c \
  "select count(*) from auth.users; \
   select count(*) from public.subscriptions where status in ('active','trialing'); \
   select count(*) from public.ai_usage; \
   select count(*) from public.workspaces;"
```

Then the check that actually proves it: bring the website up against the
restored database with the **same `LICENCE_PRIVATE_KEY`**, sign in to
`/account` as a test account that has a subscription, and paste the licence
key it shows into a released app. If the app accepts it, the restore is
real. Row counts only prove the rows came back.

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
- **The VPS itself.** This restores the data, not the machine. Hostinger's own
  snapshots cover the box; they are not a substitute for this, because a snapshot
  of a corrupted database is a corrupted database.

## Proving it works before you trust it

```bash
bash infrastructure/backup/selftest.sh
```

Runs the real `backup.sh` end to end against a stubbed `pg_dumpall` and a
stubbed `rclone`, with real gzip and real gpg — so the encryption, the size
guard, the upload verification and the restore are genuinely exercised. Eleven
checks, no network, no database. The one that matters is the round trip: a
canary row has to survive encrypt, ship and decrypt.

It found a real defect the first time it ran. The size guard refused anything
under 64KB, which is the right shape of check set to the wrong number: measured
through this exact pipeline an empty pipe encrypts to 86 bytes and a
valid-but-empty cluster to 172, while a real RATA with fifty users is 8–20KB. A
floor set for a mature database would have failed every night from launch until
there were enough customers — which is how an operator learns to ignore this
job. It is 1024 now, and `MIN_BYTES` overrides it.

## Checking it is still working

A cron job that stops running makes no noise. Set `HEARTBEAT_URL` to a
healthchecks.io check (free tier is enough) so a *missed* night pages you, not
just a failed one — the silent failure is the one that costs you the business.

And once a quarter, restore one. A backup you have never opened is a belief, not
a backup.

---

# Monitoring

## What to point it at

`https://mailrata.org/api/health` — not `/`. The front page answers 200 for as
long as the web server is alive, which is the least interesting thing that can
be true about RATA. It stays 200 while Supabase is unreachable and nobody can
sign in, and while `LICENCE_PRIVATE_KEY` is missing and no licence can be
issued or renewed.

That second one is why the endpoint exists. It is not a crash, it is a
variable, and a variable can go missing on any redeploy without producing a
single error anywhere.

```
GET /api/health          -> {"ok":true,"ready":true,"service":"rata","time":"..."}
                            200 when the database answers, 503 when it does not
```

`ok` and `ready` are different questions. **ok** is "can this serve at all" and
is the only thing that sets the status code, so a deliberately unconfigured
deployment does not page anyone at three in the morning. **ready** is
"everything the product needs is configured" — database, licence signing key,
Stripe signing secret. A launch monitor should alert on `ok`; a pre-launch one can
watch `ready` to know when the runbook is finished.

The public body names which checks are failing but never why, and never an
environment variable. Set `HEALTH_TOKEN` and pass it as `?token=` or
`Authorization: Bearer` to get the detail, including the variable to go and
set.

## The three to create

Free tiers cover all of this at RATA's size.

| | Watch | Alert when |
|---|---|---|
| **Uptime** | `GET /api/health` every 5 min | non-200, twice in a row |
| **Backup ran** | a healthchecks.io check, pinged by `HEARTBEAT_URL` in `backup.sh` | the nightly ping does not arrive |
| **Errors** | an error tracker (Sentry or similar); none is wired into the site today | anything unhandled |

The middle one is the one people skip and the one that matters most. A cron job
that stops running makes no noise, and silence is indistinguishable from
success — you find out the backups stopped three weeks ago on the night you
need one. `HEARTBEAT_URL` exists so a *missed* night pages you, not just a
failed one.

Two checks a monitor cannot do for you: restore a backup once a quarter, and
read what `?token=` says on the day `ready` goes false.
