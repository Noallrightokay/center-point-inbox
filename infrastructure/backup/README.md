# Backups, and getting back from one

Everything RATA holds for a customer is in one Postgres on one VPS: the logins,
the plans, and the encrypted mailbox credentials. At three thousand customers
that is a forty-eight-thousand-dollar-a-month business sitting on a single disk.
This is the cheapest insurance in the whole system and the one most often left
until after it was needed.

`backup.sh` dumps nightly, encrypts, and ships the copy off the box. Set it up
with the instructions at the top of the script.

## The key is not in the backup, and must not be

Mailbox credentials are encrypted with `TOKEN_ENC_KEY`, which lives in the
environment, not the database. Two consequences, and they pull in opposite
directions:

- **Lose the key and the backup cannot give you the mailboxes.** The rows
  restore, the passwords inside them stay shut, and every customer has to relink
  every mailbox. Accounts and billing survive; mail connections do not.
- **Store the key with the backup and one stolen file is everything.** The
  encryption exists precisely so that a copy of the database is not a copy of
  everyone's mail credentials. Putting the key beside it undoes that.

So keep `TOKEN_ENC_KEY` somewhere durable and *separate*: a password manager, a
sealed envelope, anywhere that is not the backup bucket. Write down which key
was current on which date — `TOKEN_ENC_KEY_OLD` exists for rotation, and a
restore of an older backup may need the key that was current then.

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

# 3. Check the three things that matter
docker compose exec -T db psql --username postgres -c \
  "select count(*) from auth.users; \
   select count(*) from public.subscriptions where status in ('active','trialing'); \
   select count(*) from public.provider_tokens;"
```

Then the check that actually proves it: bring the app up against the restored
database with the **same `TOKEN_ENC_KEY`**, sign in as a test account, and sync a
mailbox. If the mail arrives, the restore is real. Row counts only prove the
rows came back.

## What this does not cover

- **The customers' mail.** It was never RATA's to back up — it is at their
  provider, which is the entire point of the local-first design.
- **Anything on a customer's device.** Messages, files and the audit chain live
  there. That is deliberate and documented in BACKEND-SETUP.md; it also means a
  customer who loses their laptop loses those, and they should be told so plainly
  rather than discovering it.
- **The VPS itself.** This restores the data, not the machine. Hostinger's own
  snapshots cover the box; they are not a substitute for this, because a snapshot
  of a corrupted database is a corrupted database.

## Checking it is still working

A cron job that stops running makes no noise. Set `HEARTBEAT_URL` to a
healthchecks.io check (free tier is enough) so a *missed* night pages you, not
just a failed one — the silent failure is the one that costs you the business.

And once a quarter, restore one. A backup you have never opened is a belief, not
a backup.
