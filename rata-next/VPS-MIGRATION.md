# Moving RATA onto the VPS

RATA runs on Hostinger's managed Node.js hosting today, with its database on a
VPS. This describes the second half — moving the app itself onto the same box.

**Do not start this until the database move is finished and RATA is working
against it.** They are independent, and doing them together means a failure
gives you two suspects instead of one.

## What you gain, and what you take on

| | Managed Node hosting (today) | VPS |
|---|---|---|
| Deploys | API-driven, automated | yours to run |
| TLS certificates | automatic | you install and renew |
| Node upgrades, security patches | Hostinger | you |
| Process restarts after a crash or reboot | Hostinger | you (systemd) |
| Cost | included in the plan | already paid for |
| Control | build settings only | full root |

The honest trade: you are buying control with sysadmin work. Nothing about
RATA needs it — this is worth doing if you want one machine and no platform
limits, not because the app is constrained today.

One consequence worth naming: **deploys stop being automatable from a Claude
Code session on the web**, because that container cannot reach port 22. After
this move, deploys happen from a machine that can SSH to the box.

## Before you start

- The database move is done and RATA works against `db.mailrata.org`.
- You can reach the VPS over SSH, or through hPanel's browser terminal.
- **Take a snapshot first** (hPanel → VPS → Snapshots). It is the difference
  between a bad afternoon and a lost server.

## 1. Node and a process manager

```bash
curl -fsSL https://deb.nodesource.com/setup_20.x | bash -
apt-get install -y nodejs git
node -v            # expect v20.x
```

RATA needs Node 20 or newer: `imapflow` and the Supabase SDK both require it.

## 2. Get the code onto the box

```bash
mkdir -p /srv && cd /srv
git clone https://github.com/Noallrightokay/center-point-inbox.git
cd center-point-inbox/rata-next
npm ci --omit=dev
```

`--omit=dev` matters: the dev dependencies include Playwright, whose install
hook downloads a browser you do not want on a server.

## 3. Environment

Create `/srv/center-point-inbox/rata-next/.env.production` — note that RATA
reads these at request time, so they are not baked into the build:

```
APP_URL=https://mailrata.org
SUPABASE_URL=https://db.mailrata.org
SUPABASE_ANON_KEY=<the anon key>
SUPABASE_SERVICE_ROLE_KEY=<the service_role key>
NEXT_TELEMETRY_DISABLED=1
```

`chmod 600` it. The service_role key bypasses row-level security and can read
every user's stored mail credentials.

Optional, when you have them: `GOOGLE_CLIENT_ID`, `MS_CLIENT_ID`,
`MS_CLIENT_SECRET`, `SLACK_CLIENT_ID`, `SLACK_CLIENT_SECRET`,
`STRIPE_MONTHLY`, `STRIPE_ANNUAL`, `STRIPE_PORTAL`.

## 4. Build and run under systemd

```bash
npm run build
```

`/etc/systemd/system/rata.service`:

```ini
[Unit]
Description=RATA
After=network.target

[Service]
Type=simple
WorkingDirectory=/srv/center-point-inbox/rata-next
EnvironmentFile=/srv/center-point-inbox/rata-next/.env.production
ExecStart=/usr/bin/npm run start -- -p 3000
Restart=always
RestartSec=5
User=www-data

[Install]
WantedBy=multi-user.target
```

```bash
chown -R www-data:www-data /srv/center-point-inbox
systemctl daemon-reload && systemctl enable --now rata
systemctl status rata --no-pager
curl -s localhost:3000/api/sync/ms      # expect JSON, not HTML
```

`Restart=always` plus `enable` is what makes it survive a crash and a reboot —
the thing managed hosting was doing for you.

## 5. Reverse proxy and TLS

Supabase already occupies 80/443 on this box, so RATA must share the proxy
rather than bind those ports itself. Add a server block for the app's domain
alongside the existing Supabase one, proxying to `127.0.0.1:3000`, then issue a
certificate for it. With nginx and certbot:

```bash
certbot --nginx -d mailrata.org -d www.mailrata.org
```

Certbot installs a renewal timer; confirm it with `systemctl list-timers | grep certbot`.
An expired certificate takes the whole site down, and nothing will remind you.

## 6. Cut over

Only after the app answers correctly on the VPS behind its own domain:

1. Point the DNS for `mailrata.org` (and `.com`) at `2.25.161.55`.
2. Watch for an hour. Keep the managed hosting deployment untouched.
3. If anything is wrong, revert the DNS — the old deployment is still there
   and still works. That is the entire reason for not deleting it.

Keep the managed deployment for at least a week before removing it.

## 7. Deploying updates afterwards

```bash
cd /srv/center-point-inbox && git pull
cd rata-next && npm ci --omit=dev && npm run build
systemctl restart rata
```

Worth putting in a script, since you will run it every time. Note that
`npm run build` briefly leaves the app serving the old build — acceptable for a
small site, but it is a real difference from the managed deploys, which build
first and switch afterwards.

## What can go wrong

- **`npm ci` fails on the lockfile** — you cloned a different commit than the
  lockfile expects. `git status` and check you are on the branch you meant.
- **App returns HTML for `/api/*`** — you are hitting the proxy's static
  handler, not Node. The proxy block for the app domain is wrong.
- **Everything 502s after a reboot** — `systemctl enable rata` was skipped.
- **Site dies ~90 days in** — certificate renewal. Check the certbot timer.
- **Supabase and RATA fight over port 80** — only one process binds it. RATA
  must sit behind the existing proxy, never bind 80 itself.
