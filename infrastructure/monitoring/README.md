# Monitoring

Without alerts you find out about an outage from a customer. This is the
whole of it at RATA's size, on free tiers, set up by the owner in about half
an hour. Nothing here goes on the VPS: a monitor that runs on the box it
watches goes down with it.

| What | Where | Alerts when |
|---|---|---|
| The site and its database | UptimeRobot, `/api/health`, every 5 min | not `"ok":true` (any non-200 included) |
| Everything a sale needs is configured | UptimeRobot, `/api/health?token=…`, every 5 min | not `"ready":true` |
| Sign-in (Supabase Auth) | UptimeRobot, `db.mailrata.org/auth/v1/health`, every 5 min | not 200 |
| The nightly backup ran | healthchecks.io, pinged by `backup.sh` | a failed night at once; a missed one after 2 hours |
| The monthly restore drill was done | healthchecks.io, pinged by you | 37 days without one |
| Payments reached RATA | Stripe's own emails, and its Event deliveries tab | a webhook endpoint keeps failing |
| Apple's App Store notifications reached RATA | the server log (`apple/notifications:` lines), and Apple's *Get Notification History* | never by itself: look after an outage, after a change to the `APPLE_*` settings, and after the first App Store purchase ([below](#app-store-notifications-that-did-not-arrive)) |

Send every alert to an inbox that does not depend on RATA's own servers (a
personal Gmail or Outlook address, say), so the alert about the box is not
waiting on the box.

## What `/api/health` says

From `rata-next/app/api/health/route.js`:

```
GET https://mailrata.org/api/health
200  {"ok":true,"ready":true,"service":"rata","time":"..."}
503  {"ok":false,"ready":false,"service":"rata","time":"...","failing":["database"]}
```

- `ok` is "the database answers" (`const up = database.ok`), and it alone sets
  the status: `status: up ? 200 : 503`. So `ok:false` always comes as a 503.
- `ready` is "every check passes": the database, licensing (it signs a
  throwaway licence with `LICENCE_PRIVATE_KEY` and discards it) and billing
  (`STRIPE_WEBHOOK_SECRET` is set). A missing variable after a redeploy is
  `ready:false` with a 200, which is exactly the failure that is otherwise
  silent: the page loads and nobody can get a licence.
- Without the token the body names what is failing (`failing`) but never
  why. With `HEALTH_TOKEN`, as `Authorization: Bearer <token>` or
  `?token=<token>` (`authorised()` takes either, compared in constant time),
  it adds `checks`, with the reason and the variable to set, and `tookMs`.
- The database check has a 4-second deadline, so a hung database reads as a
  503, not as a monitor timeout.

**Which URL for what:**

- `https://mailrata.org/api/health`, no token: the public "is it up" check.
  Safe anywhere, including a public status page.
- `https://mailrata.org/api/health?token=<HEALTH_TOKEN>`: the "is it ready"
  check, for your monitor only. The header form keeps the token out of
  access logs, but UptimeRobot's free plan cannot send custom headers
  (they are a paid feature), so its monitor uses the query form. Make
  `HEALTH_TOKEN` a value used for nothing else (`openssl rand -hex 32`), set
  it in hPanel (task D1), paste it straight into UptimeRobot, and never into
  chat, an issue or this repository. If it leaks, what it reveals is which
  of three checks is failing and the name of a variable, not a key; change
  it in hPanel and in the monitor.

Do not point a monitor at `/`: the front page answers 200 while Supabase is
unreachable and while no licence can be issued.

## Three UptimeRobot monitors (free)

UptimeRobot's free plan: 50 monitors, a 5-minute interval, keyword
monitoring, email alerts. A keyword monitor rather than a plain HTTP one,
because a 200 is not always RATA: a hosting error or parking page also
answers 200. Keyword monitors go down when the connection fails too.

1. Sign up at uptimerobot.com; under *Integrations & Team* (alert contacts),
   confirm the email address alerts go to.
2. **RATA up**: *New monitor* → type **Keyword**.
   URL `https://mailrata.org/api/health`, keyword `"ok":true` (with the
   quotes; the route writes JSON with no spaces), *Alert when*
   **keyword not exists**, case sensitive, interval **5 minutes**. Alert
   after 2 failures if the plan offers it, so one slow response does not
   wake you.
3. **RATA ready**: the same, with URL
   `https://mailrata.org/api/health?token=<HEALTH_TOKEN>` and keyword
   `"ready":true`. Create it after D1 has set every variable, or pause it
   until then: before that it is correctly down. When it alerts, open the
   same URL in a browser; `checks` says which variable to set.
4. **RATA sign-in**: `/api/health` checks the database through the REST API,
   not Supabase Auth, so a stopped auth container would leave both green
   while nobody can sign in. Monitor type **HTTP(s)**, URL
   `https://db.mailrata.org/auth/v1/health?apikey=<SUPABASE_ANON_KEY>`.
   The anon key is public by design (every visitor's browser gets it from
   `/api/config`); the gateway in front of self-hosted Supabase accepts it
   as the `apikey` query parameter. Open the URL in a browser first: it
   must answer 200 with a small JSON naming the auth version. If it answers
   401, your gateway does not take the key from the query; drop this
   monitor rather than put the service_role key anywhere.

## Two healthchecks.io heartbeats (free)

The backup and the drill are set up in `../backup/README.md` (Setting it up,
step 5; the drill's monthly reminder). Two checks: "RATA backup", period 1
day, grace 2 hours, pinged by `backup.sh` through `HEARTBEAT_URL` (start,
success, and `/fail` on a refusal); "RATA restore drill", period 30 days,
grace 7 days, pinged by you at the end of each drill. The ping URLs are
not secrets in the way a key is, but anyone holding one can mark the job
done, so keep them in `/etc/rata-backup.env` and your password manager only.

## Stripe: failed webhook deliveries

A payment reaches RATA only through the webhook
(`https://mailrata.org/api/stripe/webhook`), which writes `subscriptions`.
If it fails, the customer paid and cannot get a licence.

- **Stripe retries for you, for a while.** In live mode it retries a failed
  delivery for up to three days with exponential back-off (in a sandbox,
  three times over a few hours), and emails the account when an endpoint
  keeps failing. Make sure those emails reach an inbox you read daily: the
  account owner's address, or add yours under the Stripe account's team and
  notification settings.
- **What the codes mean here** (`app/api/stripe/webhook/route.js`): `400` on
  every delivery is a wrong or missing `STRIPE_WEBHOOK_SECRET` (*RATA ready*
  shows the missing case); `500` is the database unreachable (*RATA up* will
  be down too); a `409` "no subscription row for that customer yet, and
  nowhere to keep the event" means `pending_subscriptions` is missing
  (`database.sql` section 6 has not run; the server log says so): the event
  succeeds on Stripe's retry once the checkout has made the row, but every
  first checkout waits for that retry until section 6 is run. With the table
  there, a subscription event that arrives before its checkout answers `200`
  and is applied by the checkout.
- **After any outage**, and whenever Stripe emails: Workbench → Webhooks →
  the endpoint → **Event deliveries**, filter *Failed*. Anything still
  failed after the fix: **Resend** (the Dashboard can resend for 15 days
  after the event). Then check the customer's row in `subscriptions`.

## App Store: notifications that did not arrive

A refund, a revocation and a renewal date extended reach RATA only through
App Store Server Notifications (`https://mailrata.org/api/apple/notifications`,
`rata-next/LAUNCH.md` section 11, step 5), which write `apple_subscriptions`;
so does a billing grace period, unless the phone's own build reports it. One
lost means a refunded subscription's licence keeps renewing to the end of the
refunded period, and its AI answering to 3 days past it; or, for an extended
one, renewal refused from the old date, and its licence and AI ending 3 days
after it, unless the phone sends the extended purchase first.

- **Apple retries less than Stripe, and tells nobody.** A notification
  answered with anything but 200 to 206 (the route's 400 and 500 alike) is
  sent again five times, 1, 12, 24, 48 and 72 hours after the attempt
  before, about six and a half days in all, in Production only; a Sandbox
  one is sent once (Apple, [*Responding to App Store Server
  Notifications*](https://developer.apple.com/documentation/appstoreservernotifications/responding-to-app-store-server-notifications)).
  Apple's documentation describes no email or alert for a failed delivery,
  and RATA has no code that replays one.
- **What the log lines mean** (`app/api/apple/notifications/route.js`, which
  logs each refusal and fault as it happens, never the purchase; each line
  begins `apple/notifications:`):
  - `refused (INVALID_APP_IDENTIFIER): Apple signed it for bundle id …, app
    id …` (400): `APPLE_APP_ID` or `APPLE_BUNDLE_ID` is not the app's, and
    the line names which, with its value. A wrong `APPLE_APP_ID` refuses
    every Production notification and nothing else, so the phone's
    purchases are still licensed and only this line shows it (LAUNCH.md
    section 11, *How to tell it worked*).
  - `a Production notification arrived and APPLE_APP_ID is not set` (500):
    section 11, step 7.
  - `refused (<status>)`, `its transaction was refused (<status>)`, `its
    renewal info was refused (<status>)` (400): a signature that did not
    verify, by the library's status name. Anyone can post to the address,
    so a stray one is noise; one at every delivery after a deploy points
    at Apple's roots in `lib/apple-roots`, or at an `APPLE_TEST_ROOT`,
    which must never be set on the live site.
  - `the App Store root certificates could not be read (…)` (500): the
    deploy lost `lib/apple-roots`, or cannot read it.
  - `SUPABASE_URL or SUPABASE_SERVICE_ROLE_KEY is not set` (500), `run
    section 7 of rata-next/database.sql` (500), `could not record a
    notification <code>` (500: the database refused or did not answer;
    *RATA up* is likely down too).
- **After an outage, or once a wrong setting is fixed** (and look once after
  the first purchase from the released app: no `refused
  (INVALID_APP_IDENTIFIER)` line, section 11): a Production notification
  first refused less than about six and a half days before the fix comes
  back at Apple's next retry, up to 72 hours later; nothing else comes back
  by itself. For the rest, call Apple's [*Get Notification
  History*](https://developer.apple.com/documentation/appstoreserverapi/get-notification-history)
  (App Store Server API, `POST /inApps/v1/notifications/history` at
  `api.storekit.apple.com`, or `api.storekit-sandbox.apple.com` for the
  sandbox) with `startDate` the start of the outage, `endDate` now and
  `onlyFailures: true`. It answers the notifications that have not reached
  the server, and those Apple is still retrying, each as Apple sent it
  (`signedPayload`) with the date and result of each attempt
  (`sendAttempts`), 20 at a time with a `paginationToken` while `hasMore`
  is true; the history goes back 180 days in Production and 30 in the
  sandbox ([*NotificationHistoryRequest*](https://developer.apple.com/documentation/appstoreserverapi/notificationhistoryrequest),
  [*NotificationHistoryResponse*](https://developer.apple.com/documentation/appstoreserverapi/notificationhistoryresponse)).
  It needs the In-App Purchase key of section 11 (*When a subscriber asks
  about their record*), kept on your own computer; Apple's library, which
  `rata-next` already has, calls it as `getNotificationHistory`. Post each
  one back as Apple would:
  `curl -sS -X POST -H 'Content-Type: application/json' -d '{"signedPayload":"<it>"}' https://mailrata.org/api/apple/notifications`
  answers `{"received":true,…}`. The library checks Apple's certificates
  as of when Apple signed it (online checks are off), and the record is
  ordered by the period each statement is about, then by when Apple signed
  it, so one posted late, or twice, changes nothing a later one has already
  said. Then check the
  subscription's row in `apple_subscriptions`. To check only that the
  address answers again, Apple's *Request a Test Notification* sends a
  `TEST`, which the route answers 200 and records nothing of, and *Get
  Test Notification Status* says whether it arrived.

## Supabase: "project paused", and what stands in for it here

RATA's Supabase is self-hosted on the VPS, and Supabase does not pause
self-hosted projects. (Its free hosted projects are paused after a period of
inactivity; if RATA ever moves to one, the pause email goes to the Supabase
account's owner, and *RATA up* goes down within five minutes of it.) What
takes the database away here instead, and what catches each:

| What happens | Caught by |
|---|---|
| The `db` container stops, or Postgres will not answer | *RATA up* (`ok:false`, 503), within 5 minutes |
| The auth container stops | *RATA sign-in* |
| The whole VPS is down or unreachable | *RATA up* and *RATA sign-in*; the backup heartbeat the next night |
| The disk fills | the next backup refuses and pings `/fail`; writes start failing before reads do, so *RATA up* may stay green: look at `df -h` when a backup fails |

## Errors

No error tracker is wired into the site today. Until one is, the Node
process's own log in hPanel is where a 500's reason is.

## Checking the monitors themselves

Once, after setting up: in UptimeRobot, edit *RATA up*'s keyword to
`"ok":nottrue` and wait one interval. The alert must reach your inbox. Put
it back. Then `curl -fsS <HEARTBEAT_URL>/fail` once from anywhere: the
backup check must email you. A monitor that has never alerted is as
unproven as a backup that has never been restored.
