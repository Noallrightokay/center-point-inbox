# Working on RATA

Read `README.md` first — it has the layout, the build and test commands, and the
full list of traps. This file is what an assistant needs that the README does
not say, and the state of play as of the last session.

## The one-line version

RATA is a desktop mail client. Mail is read on the customer's own machine over
IMAP and SMTP; the password lives in the OS keychain and never reaches a server.
`mailrata.org` sells licences and runs the AI relay — it has no credentials, no
connection to anyone's mailbox, and no code for one.

Every decision in this repository follows from that. The owner's two rules
(2026-09-26): RATA must never need big servers, and must never hold anything
a breach could leak. So a mail password never reaches a server, and mail
never does either, with **one** deliberate exception the owner chose: the AI
relay (`rata-next/app/api/ai`), which passes the text of a message a customer
asked to summarize or translate to Anthropic and back, stores and logs none
of it, and costs RATA at most `AI_MONTHLY_CAP_USD` ($2) per customer a month
on Haiku. Anything else that would put mail on a server is wrong, however
convenient.

## Where things are

| Path | What |
|---|---|
| `desktop/rata-mail/` | Mail engine: DNS discovery, IMAP, SMTP, outbound guard, server-side message actions, paging back through history, what a reply needs, decoding message bodies and attachments, sending with attachments, sanitising HTML, the Sent, Archive, Spam and Drafts folders and the customer's own, saving RATA's drafts to Drafts, Gmail's archive, refreshing only what is new, Cc and Bcc and named addresses (`"Name" <addr>`), a list's unsubscribe address (`List-Unsubscribe`), the last id of `References` (for the conversation view), being told of new mail (IDLE), signing in with a password or an OAuth token (XOAUTH2), judging a sender's file names (`names.rs`). Standalone, knows nothing about the app. 318 tests, plus 16 loopback tests against real Dovecot and GreenMail (`tests/loopback.rs`, `--features loopback-tests`, CI job *Mail layer against real servers*). |
| `desktop/rata-app/` | Tauri shell: keychain, store, licence verification, the bridge to the interface, saving attachments and marking them as downloads (`mark.rs`), updating itself, new-mail notifications, watching inboxes for new mail, Sign in with Microsoft (`oauth.rs`), Copy diagnostics (`diagnostics.rs`), telling a picture attachment by its bytes (`core::sniff_image`), connected iCloud Drive and Creative Cloud Files folders (`cloud.rs`, K5), files made by Create file (`created.rs`, K6), Share to Slack (`slack.rs`, K3). 222 tests. |
| `rata-next/` | The website: marketing, Stripe, licence issue and renewal, the AI relay (`/api/ai`), account deletion and `/api/health`, the help page (`public/help.html`, served at `/help` through `proxy.js`), and the privacy policy and terms (`public/privacy.html`, `public/terms.html`). Next.js on Hostinger. 813 checks (`npm test`, after `npm run build`); `scripts/live-check.sh` checks a deploy from outside, with no credentials, and fails while a live privacy or terms page still shows an `[OWNER: …]` placeholder (`LAUNCH.md`, the step before §6). |
| `rata-next/public/app.html` | The interface. **One copy.** The desktop app builds its own from this at build time. |
| `desktop/rata-app/harness/` | `ui-harness.mjs` drives the real interface against a fake backend under the app's own CSP (527 checks, among them axe-core scans that fail on any serious or critical accessibility rule; CI job *Desktop interface, driven*); `feed.cjs` writes the update feed's files (its rules tested by `feed.test.cjs`, in Desktop CI and the release gate); `shots.mjs` takes the website's screenshots; `verify-release.sh` checks a published release (`--title <v>` prints the title it expects); `smoke-installed.sh`/`.ps1` install and launch an installer on its release runner. |
| `desktop/rata-app/demo/` | A clickable demo: the real interface and `bridge.js` over a fake Rust side with invented sample mail (`demo-backend.js`: a doctor at two invented hospital networks, a personal mailbox and two side businesses, nine saved people, real PDF, Word and Excel attachments made by the Format Bridge's own writers, a Base/Pro switch). `build.sh [out]` assembles it from `sync-ui.sh`'s output and writes the vendored libraries' control characters and U+FFFD as escapes, which some hosts require. It sends nothing and opens no mailbox; no patient information. |
| `infrastructure/backup/` | Nightly encrypted Postgres backup for the VPS, the restore drill, and `selftest.sh`. |
| `infrastructure/licence/` | `mint.sh`: mints a licence on the VPS with OpenSSL alone, in exactly the format `lib/licence.js` issues; `mint.sh check` says whether the signing key is the one the released installers trust, and `new` never overwrites a key. |
| `infrastructure/monitoring/` | The uptime and alerting recipe (`/api/health`, healthchecks, Stripe failures). |
| `docs/` | `MVP-PLAN.md` (the plan to MVP and its task cards), `SECURITY-REVIEW-2026-09.md`, `SMOKE.md` and `MVP-EVIDENCE.md` (the owner's live-provider checklist and its results), `WINDOWS-SIGNING.md`, `CONNECTIONS-SETUP.md` (the owner's registrations for OneDrive, Slack and Google Drive), `hostinger-mcp.md` (managing the VPS and DNS from the repo). |
| `DESIGN.md` | How RATA looks, app and website: tokens, type, shape, copy. The CSS variables at the top of each page are its tokens. |

There is no `src/`. Nine .NET microservices on Kubernetes were abandoned and
deleted in `064674b`. Any reference to `centerpoint-inbox.com`, an API gateway
or a translation worker is a ghost — report it.

## Before changing anything

- `desktop/rata-app/` → run `./sync-ui.sh` **before** `cargo build`, every time.
  Tauri compiles the interface into the binary; without this you are testing the
  previous build and nothing warns you.
- Do not remove `"dangerousDisableAssetCspModification": ["style-src"]` from
  `tauri.conf.json`. Without it Tauri nonces `style-src`, a nonce makes
  `'unsafe-inline'` ignored, and every inline `style=` attribute is silently
  dropped — in the packaged build only, never in `tauri dev`. That shipped in
  v0.1.2: `auth.html` hides panels with `style="display:none"`, so the first
  screen showed a password-reset form over the sign-up form. Only `style-src`
  is exempted; scripts keep Tauri's nonce and hash protection.
- Check UI changes in a **packaged** build (`cargo build --release --features
  tauri/custom-protocol`), not `tauri dev`: the two differ in exactly the ways
  that ship bugs.
- `bridge.js` replaces the browser's `fetch` to route the interface's server
  calls into Rust. Read it before touching either side.

## Releases

**To release, bump the version and merge.** `release.yml` runs on every merge
to `main` that touches the app; if `v<version>` from `tauri.conf.json` has not
been released, it builds the four installers into a draft pre-release (0.x is
beta) and publishes it once installers are attached — publishing is what
creates the tag, with the workflow's own token. A merge without a version bump
builds nothing. Nobody creates a tag by hand: that step failed three times in
four through the GitHub UI, and an assistant session cannot do it at all.

Nothing is built until `gate` (*Tests before a release*: `sync-ui.sh`, the
engine and shell tests, the driven harness, and the update feed's rules in
`harness/feed.test.cjs`) passes on the commit being released (BUG-R,
v0.1.43); `create-release` and `installers` both need it, and `installers`
names `needs.gate.result == 'success'` because its `always()` turns off the
implicit check. A merge without a bump still stops at `plan`. Keep gate's
steps in step with `desktop-ci.yml`. The first real run of the gate is
v0.1.43's.

apt never waits on a stalled download (#111, first released in v0.1.44).
Before each of the seven steps that install with apt (`apt-get install`,
or `playwright install --with-deps`: three in `desktop-ci.yml`, one in
`rata-next-ci.yml`, three in `release.yml`, on Linux runners only), a step
*Make apt retry a stalled download instead of hanging* writes
`/etc/apt/apt.conf.d/80-rata-retries` (`Acquire::Retries "3"`, 30 s http
and https timeouts), and each of those seven has `timeout-minutes: 8`, so
a real hang fails under its own name instead of using up the job. The
installed-app smoke's `xvfb` install runs later in the same installers job,
so the settings cover it too, under that step's own 10 minutes. On #110 two
jobs had spent their whole 20 minutes in apt before any test ran, and a
session here cannot re-run a job. A new apt step gets both.

The version lives in four places, bumped together: `tauri.conf.json`
(`version` *and* the window `title`, which is how testers report their build),
`src-tauri/Cargo.toml`, `src-tauri/Cargo.lock` (`cargo update -p rata-app
--precise <version>`), and the cache name in `rata-next/public/sw.js`
(`rata-shell-v<n>`, one up), so the website's shell is fetched afresh.

Before an installer is kept, `release.yml` installs and launches it on its
own runner (`harness/smoke-installed.sh`, `.ps1` on Windows; B3): it must
stay up 15 s with the window titled `RATA <version> beta` (`RATA <version>`
for 1.x) and no `panicked at` in its log, or that platform's files come off
the draft release and stay on the run as an artifact. A bump that forgets
the window `title` fails here. The first real run is v0.1.39's; the macOS
and Windows halves had never run before it, and on macOS a runner that will
not let the title be read falls back to "stays up, clean log" with a
warning. The build step also gets `RATA_MS_CLIENT_ID` from a repository
**variable** (not a secret: a public client id), which turns on Sign in
with Microsoft; unset, the build has none (see *Sign in with Microsoft*).

Windows signing is optional, like Apple's (E3): Azure Artifact Signing
(six `AZURE_*` values) or a `.pfx` (`WINDOWS_CERTIFICATE` +
`WINDOWS_CERTIFICATE_PASSWORD`), never both, and half a set fails the
Windows job before it compiles. Owner steps are in
`docs/WINDOWS-SIGNING.md`. The bundler signs `rata-app.exe` before NSIS
packs it, then the uninstaller and the `-setup.exe`; `signtool verify /pa`
then checks the installer and the exe inside it (and the uninstaller when
7-Zip lists it), and a bad signature takes the installer off the draft. `target/release/rata-app.exe` is left unsigned by design (the
bundler restores it), so check the copy inside the installer. With no
secrets the job logs `Unsigned build` and carries on.

Traps, all of which have bitten:

- A manual `workflow_dispatch` produces artifacts but **no release**. This hid
  a missing `GITHUB_TOKEN` for two runs.
- A tag build uses the workflow **as it existed at that tag**; fixing the
  workflow does not fix an existing tag.
- Pushing a `v*` tag by hand still works. It must start with `v`, and in the
  GitHub UI the tag and title are separate fields — the tag takes no spaces.
- After a release, verify the shipped binary rather than assuming: extract it and
  check the licence public key, `org.mailrata.desktop`, and that no
  `fonts.googleapis`/`fonts.gstatic` string is present.
  `harness/verify-release.sh` does this for all five installers (I3,
  v0.1.45: the `.deb`, the `.exe`, the AppImage's squashfs found after its
  runtime and read with `unsquashfs`, never run, and both `.dmg` files
  through `7z`, with `Info.plist` naming `org.mailrata.desktop`). It exits
  0 when all pass, 1 on a failure and 3 when a tool is missing and a file
  was skipped (`squashfs-tools`, `7z`). On the Intel Mac binary the title is
  compiled into the code as 8-byte immediates, reported `title-in-code=1`,
  which passes. `verify-release.sh --title <version>`
  prints the window title it expects (0.x is `RATA <v> beta`, 1.x is
  `RATA <v>`), worked out the way release.yml's smoke works it out.

Released: **v0.1.1**, four of five files (the Linux `.AppImage` upload failed
with `Error saving asset`, probably transient; the bundle built fine and is on
the run as an artifact). v0.1.2 shipped with a broken first screen (see the
CSP note above) and must not be given to anyone. **v0.1.3 is the beta build
testers started on**; v0.1.4 adds server-side read/star/delete/archive;
v0.1.5 adds Load older mail and shipped all five files; v0.1.6 adds Reply and
removes every control that led nowhere; v0.1.7 decodes message bodies;
v0.1.8 opens whole messages and saves attachments; v0.1.9 sends
attachments; v0.1.10 shrinks the window to fit small screens (it opened
1280×860, taller than a 1366×768 laptop, with Send below the edge — see
`fit_to_screen` in `main.rs`, which has to use the configured size because
the window reports 0×0 during setup); v0.1.11 adds Forward, which carries
the original's attachments; v0.1.12 shows HTML mail formatted; v0.1.13
keeps mail in IndexedDB, one record per message, with no storage cap;
v0.1.14 opens links in the browser, asking first for links in formatted mail;
v0.1.15 keeps a closed draft whole; v0.1.16 draws only the rows of the
list that are on screen; v0.1.17 adds AI summaries, translation and task
flags through the relay (which needs the website deployed — see below);
v0.1.18 rebuilds the Format Bridge (real .docx, structure kept, every sheet)
and makes its downloads work in the app at all; v0.1.19 reads PDFs; v0.1.20
converts an attachment straight from the message; v0.1.21 keeps message
text on disk until it is needed instead of in memory; v0.1.22 reads the
Sent folder too; v0.1.23 can update itself (once the owner has added the
update key — `rata-next/LAUNCH.md` §9); v0.1.24 takes the plan from the
licence, so Pro opens Side by side in the app; v0.1.25 adds Archive and
Spam; v0.1.26 adds the customer's own folders and Move to, and files
synced mail under its mailbox (replies had gone from the first one);
v0.1.27 fetches new mail by itself and downloads only what is new; v0.1.28
fetches the mail a refresh had to leave out after a long absence; v0.1.29
shows a desktop notification for new mail; v0.1.30 archives, moves home
and files several messages at once; v0.1.31 shows Drafts and finishes one
begun elsewhere; v0.1.32 adds Cc and Reply all; v0.1.33 adds Bcc; v0.1.34 brings new mail
as it arrives (IMAP IDLE); v0.1.35 signs in far less often; v0.1.36 adds signatures; v0.1.37 is the
redesign to `DESIGN.md`; v0.1.38 shows Gmail's archive; v0.1.39 adds Sign
in with Microsoft (live once the owner's client id is in the build), no
longer lets one unreadable message stop a refresh, archives on servers that
only name the folder, says why a typed server address failed, and is the
first release whose installers are installed and launched before they are
kept (watch that run); v0.1.40 marks every file it saves as a download,
labels a program named to look like a document and asks before saving
it, and holds the AI cap under concurrent requests; v0.1.41 saves RATA's
own drafts to the mailbox's Drafts folder, so they follow the customer to
the phone, and fixes the security review's cheap Lows (SEC-4); v0.1.42
fixes the security review's part 3: the website takes the plan from
Stripe's subscription events (every checkout had been recorded as Base),
IMAP sign-in errors no longer repeat a password, Gmail's replaced drafts go
to the Trash, and an invisible character can no longer hide a program's
real extension (SEC-5, SEC-6); v0.1.43 adds Unsubscribe, Copy diagnostics
and a Help link, and address suggestions in the composer, renews the
licence while RATA stays open, never hides mail the server kept when Delete
or Archive is refused, finds Trash by name, keeps the server's own words
when it refuses a password, sends to Outlook and iCloud on 587 first, sends
the AI relay only the start of a long message, and is the first release
built behind the `gate` job, with an update feed file per kind of
installation (BUG-R); v0.1.44 shows picture attachments in the message
and opens them large (H11), lists the rest of a conversation under an open
message, linked by ids only (H5), and makes the list, dialogs and toasts
work by keyboard and screen reader, checked by axe, with pinch zoom no
longer blocked (H10); its CI retries a stalled apt download instead of hanging;
v0.1.45 fetches a message's pictures in one sign-in (I4), lets mail of a
removed mailbox be deleted and keeps a folder the server renumbered from
mixing two messages (I6), makes Delete account in the app remove what RATA
keeps on the computer (I7), sends the briefing's messages by number only and
writes a non-ASCII sender name correctly (I1, I2), tries Zoho's organisation
servers, and says the server's own reason when a renewal is refused (I3);
v0.1.46 leaves nothing behind when a sign-in or licence renewal finishes
after Unlink or Delete account (SEC-7), and makes Copy diagnostics keep the
latest failure of every operation and say how each special folder was found
(J1);
v0.1.47 keeps folder, file and subject names out of Copy diagnostics,
stops a link still in progress when Delete account runs, and lets a
Microsoft mailbox whose list entry lost `auth` renew again (SEC-8);
v0.1.48 finds a company mailbox behind a mail filter from its SPF record or
autodiscover name (L1), and carries K5's folder commands, which no screen
calls yet; v0.1.49 adds Create file, Created by you and Send with RATA
(K6, Pro, with SEC-9's fixes) and Connected accounts with iCloud Drive and
Creative Cloud Files (K1, K5; OneDrive, Google Drive and Slack report
`available: false` and stay hidden until K2 to K4), and brings back the
Start in your browser links; v0.1.50 adds Share to Slack (K3, live in a
build with the owner's `RATA_SLACK_CLIENT_ID`) and fixes the review of
K1 and K6's page (SEC-10). An
upload that fails
still leaves the installer on the run as an artifact, and the release is still
published with whatever did attach.

## Permissions in this environment

An assistant session here can push branches, open PRs and merge them — which
is all a release needs now. It **cannot** push or delete tags, create or delete
releases directly, or dispatch workflows — all return 403. Do not discover this
mid-task and hand it back; say so immediately.

It cannot re-run a job either (`rerun-failed-jobs` is 403). A job that ends
`cancelled` about 15 minutes after its run started, with no runner name and
no steps, never got a machine: check https://www.githubstatus.com (on
2026-10-05, 19:50 to 21:55 UTC, an Actions incident did this to every run
of #142, #143 and #144). It is not the change's failure. Ask the owner to
press **Re-run failed jobs**, or start a fresh run with the next real
change once Actions is back; never an empty commit.

## Licensing

A signed Ed25519 token, verified offline against a public key compiled in at
build time via `RATA_LICENCE_PUBLIC_KEY`. Tokens last 30 days
(`LICENCE_DAYS` in `rata-next/lib/licence.js`) and the app renews itself in the
final week. The private key lives on the VPS at `/etc/rata/licence.key`
and is minted with `infrastructure/licence/mint.sh <email> <plan> <days>` (OpenSSL only;
`mint.sh check` says whether a key is the one the released app trusts). The website's
`LICENCE_PRIVATE_KEY` is this same VPS key (`rata-next/LAUNCH.md` §2);
never generate a new one for the site, or every licence it issues fails in
every installed app.

A build without the public key rejects every licence; a build with the wrong one
rejects every legitimate licence with a signature error.

Renewal refuses a licence that expired more than `RENEW_GRACE_DAYS` (90,
`lib/licence.js`, `renewable`) ago, before the database is asked, with
`reason: 'too-old'` and "Sign in at mailrata.org to get a new one." (SEC-4,
v0.1.41), so a token that leaked long ago cannot be revived. bridge.js
`renew()` takes any answer with a `reason` and a sentence as a real answer
(`no-subscription`, `too-old`, `pending`, `malformed`, `bad-signature`,
`no-public-key`, `not-configured`; I3, v0.1.45, SEC-7) and never passes it to `set_licence`, so a
working licence is kept; only a bare error or no answer is "could not
reach mailrata.org". `/api/licence/renew` answers `pending` for a row being
set up (`settingUp`), as `/api/licence` does, and its `no-public-key`
sentence is the customer's, with the operator's hint in the server log.
When `issue()` throws (no signing key, or one that will not read), both
`/api/licence` and `/api/licence/renew` answer 500 `reason:
'not-configured'` with a fixed sentence and no `error` field, and log the
reason (SEC-7).
`settleLicence` (in bridge.js) puts the server's sentence in the licence box when the
licence is still not good afterwards (harness: "a licence too old to renew
itself says what to do). **Renewal while RATA is open (BUG-L, v0.1.43).**
`settleLicence(force)` is the only renewal path, one at a time (a second
caller shares the one in flight): at launch, every six hours
(`RENEW_EVERY`; the harness shortens it with `window.__RATA_RENEW_EVERY`),
on `window` `online`, when `refresh_mail` answers `unlicensed` (it renews,
then reads mail again), when the AI relay answers `reason: 'expired'`
(forced, then one retry with the new token, never more), and from the
licence box: Submit on an expired key that can still renew, or on an empty
box, and **Try again**, shown only when there is a token to renew. Each run
fires `rata-standing`. `renew()` gives up after 20 s and keeps a renewal only
when `set_licence` answers licensed with nothing `refused`; anything else
counts as unreachable, the old licence stays, and the box says it "could
not reach mailrata.org". Before 0.1.43 renewal ran once at launch, so a
copy left open across its expiry stopped fetching mail in silence.
`set_licence` (core.rs, `outweighs`) never lets a key that fails
`licence::check` (malformed, forged, another key's, or genuine but
expired) replace a working licence, nor one that expired at most
`RENEW_GRACE_DAYS` ago (`licence::renewable`; the constant must equal
`lib/licence.js`'s) unless the key is genuine and expires no earlier; it
then writes nothing and answers the kept licence's standing with
`refused: { reason, message }`, which the box shows. White space is stripped
from a key in the box, in `licence::check` (`clean`), in what is stored and
in `/api/licence/renew`, so a key a mail client wrapped still reads. A copy
that keeps renewing inside the 90 days is still not stopped (review row 15,
a business decision).

On the website side, a licence is issued or renewed only while the
`subscriptions` row is live: `LIVE_STATUSES` (`active`, `trialing`,
`past_due`) lives once in `lib/plan.js` and `lib/stripe.js` re-exports it.
`past_due`, Stripe's grace state while a failed card is retried, is
entitled everywhere; that is safe because a licence lasts 30 days and
renews only while the row is live. A `checkout.session.completed` for an
address whose row is live under a **different** `stripe_customer` gets 200
`conflict: true`, writes nothing and logs one line without the address or
customer id (`checkoutConflict`, which since SEC-6 also holds an
`incomplete` row, review P3-5); the owner reconciles it in Stripe. The
same customer, a hand-minted row with no customer, or a row no longer live
may be written. **The plan comes from the subscription events** (SEC-6,
v0.1.42). A delivered Checkout Session never carries `line_items`, so
before SEC-6 every checkout was recorded as Base. The plan and domain
add-ons now come only from `customer.subscription.created`/`updated`,
which carry the prices; a checkout event writes only the email, the
customer, whether the money is in, and the row's stamp, the session's own
`created`, so no subscription event of that purchase is refused as stale
(`checkoutWrite`). Until a subscription event of the purchase has named
the plan, the row is `incomplete` even when paid, so `/account` says "not
yet" rather than issuing a 30-day Base licence to a Pro buyer. A checkout
whose `payment_status` is not `paid` or `no_payment_required` (a bank
transfer still clearing) is recorded `incomplete` too, which is not live,
so the customer's later events still find the row. An `incomplete` write
never replaces a live row (200 `pending: true`, nothing written).
`checkout.session.async_payment_succeeded` says only that the money is
in: the row is `active` then if a subscription event has already named the
plan, otherwise once one does. The webhook must subscribe to exactly five
events: `checkout.session.completed`,
`checkout.session.async_payment_succeeded`,
`customer.subscription.created`, `customer.subscription.updated` and
`customer.subscription.deleted` (`STRIPE-SETUP.md` §3, `LAUNCH.md` §3);
without `customer.subscription.created` a card checkout is never
entitled. **The subscription event that comes first (BUG-S, v0.1.43).**
Stripe usually sends `customer.subscription.created` before the checkout,
when there is no row to write yet; that event was answered 409 and the plan
landed only on Stripe's retry, hours later in test mode. Now a subscription
event that finds no row for its customer is kept in
`pending_subscriptions` (`database.sql` §6, keyed by customer, no email,
RLS on and no policy) and answered 200 `pending: true`; the checkout event
applies it in the same request, after its own upsert, through the same
guarded update (`pending_applied: true`), then deletes it. A kept event
raised before that checkout is dropped, not applied. 409 is left for one
case: §6 has not run (the log names the section, not the customer).
`customer.subscription.created` writes under a strict `lt` guard
(`orderGuard`, `lib/stripe.js`) and `updated`/`deleted` under `lte`, so a
`created` in the same second as an `updated` cannot lower a live row to
`incomplete`. `/api/licence` answers `reason: 'pending'` for an
`incomplete` row that has a customer (`settingUp`, `lib/plan.js`), and
`/account` then shows "Setting up your licence" with no buy buttons,
asking 30 times 2 s apart (also after `checkout=success`), and ends on a
Reload button, never on "has not reached us". Signed-out links on
`/account` carry `next=account` and the just-paid state through
`auth.html`. Every buy link built for a signed-in person carries
`prefilled_email` and `client_reference_id` (`buyLink` in index.html and
account.html, `withBuyer` in `renderPlan`); the webhook does not read
`client_reference_id` yet (the licence is still keyed on the address paid
with). A live plan in Settings switches only in the billing portal, never
through a Payment Link, which made a second Stripe customer the webhook
refused. The desktop build's `stripePortal` is `https://mailrata.org/app`
(`sync-ui.sh`, I1, v0.1.45), whose Settings shows the real portal once
signed in; it had been `/account`, which has no portal link. Deleting an account (`app/api/account/route.js`) removes
`workspaces`, `subscriptions` and the person's `ai_usage` rows, and still
finishes when `ai_usage` does not exist yet (PGRST205 or 42P01: §5 of
`database.sql` never ran). It is refused (`blocksDeletion`,
`lib/account.js`) while the subscription is live or `incomplete`, and for
`LICENCE_DAYS` (30) after it ended, naming the date (review P3-6): a
licence issued while it was live is the AI relay's credential, and
deleting `ai_usage` would let it spend the month's allowance again.
`incomplete_expired` issued no licence and does not block. The app's
Delete account row names what `DELETION_REMOVES` names (tested).
**Delete account in the desktop app (I7, v0.1.45)** is about this
computer: bridge.js `/api/account` GET says what it would remove and keep,
and DELETE calls `forget_everything` (core.rs), which unlinks every mailbox
through `unlink` (keychain entries first, Microsoft pieces too, then the
list entry, so a refused keychain leaves the mailbox listed with the error),
then removes the licence stored here and the diagnostics notes; a keychain
refusal stops it before the licence goes, and a mailbox linked meanwhile
makes it refuse. **Nothing comes back afterwards (SEC-7).** `unlink` holds
the list's lock from emptying the keychain to saving the list, and a
Microsoft renewal that answers later writes its rotated token, caches its
access token, or parks the mailbox only if `still_linked` (still listed,
still OAuth, same `added_at`), so it lands wholly before Unlink or finds
the mailbox gone or linked again. `still_linked` is the same `added_at`
and `auth` as when the renewal began and a Microsoft sign-in still in the
keychain, which matches `account()` (SEC-8: a list entry an older RATA
rewrote without `auth` could not renew under 0.1.46), so a late renewal
never writes over a password relinked meanwhile. Delete account cancels a
Microsoft sign-in waiting for the browser, and every link is kept through
`keep_linked`, which under the list's lock re-checks the epoch, the licence
and the plan's limit (`may_link_in`) before writing the keychain, the list
and the access token, so a link begun before Delete account keeps nothing
(SEC-8). `forget_everything` bumps a counter (`forgotten`) as it begins and
again once the licence is gone, which `Standing` carries as `epoch`; `renew()` passes it to
`set_licence` as `since`, and one older than the last Delete account is
refused without writing, so a renewal waiting on mailrata.org cannot store
the licence again. The licence box sends no `since`. Only after Rust says ok does the page clear IndexedDB,
localStorage (the Undo-send outbox included, on the website too) and the
sign-in. Delete permanently stays off until the GET has answered, and on
any error. The mailrata.org account is never deleted from the app:
**Open mailrata.org account** opens `https://mailrata.org/app`, whose
Settings holds the website's Delete account. Before 0.1.45 the app wiped
only the page, and the next refresh brought every mailbox back.
`retry_mailbox` (no route called it) is gone.
`pending_subscriptions` rows hold no address and are not deleted with an
account; they go when applied or dropped. **The privacy policy and terms
(H1)** are tied to the code by `tests/legal.test.mjs`: `LICENCE_DAYS`,
`RENEW_GRACE_DAYS`, `blocksDeletion`, `DELETION_REMOVES`/`KEEPS`, `PLANS`
and their prices, `LIMITS` with the briefing's numbers, `CLOUD_FIELDS`/
`CLOUD_STRIP` and database.sql's tables, so changing any of them means
changing the pages. `manifest.json` and the plan blurbs are tested to sell
nothing RATA does not do (`app.test.mjs`, BUG-D). `next.config.js` sends HSTS (one year,
subdomains, no preload), `X-Frame-Options: DENY`, `frame-ancestors 'none'`,
`nosniff` and `strict-origin-when-cross-origin` on every path
(`tests/headers.test.mjs`); they reach mailrata.org only when it is
redeployed.

## Status

The MVP plan is `docs/MVP-PLAN.md`; the security review is `docs/SECURITY-REVIEW-2026-09.md`.

**In beta.** Testers follow `BETA.md` and report through the *Beta bug* issue
template. A report's exact error text is the primary evidence — read it before
theorising, and prefer a failing test that reproduces it to a guess.

The ten findings of the 2026-09-23 code review are fixed on the way to v0.1.2:
auth classification now only believes permanent refusals during sign-in (SMTP
5xx, IMAP `NO` without a "come back later"), a locked keychain is `keychain`
rather than `auth`, Remove really removes, discovery continues past a refused
host, partial fetches and lapsed licences no longer report success.

Works: licence verification (incl. offline and renewal), adding a mailbox by
address and app password, server discovery via SRV → MX → conventional names,
fetching newest messages from INBOX across several mailboxes, sending, a
**Reply** button that threads (`replyTo` in `app.html` → `inReplyTo` →
`Message.message_id`, which `thread_id` refuses if it could smuggle a header)
and goes to the sender's Reply-To, and a guard stopping a hostile server
redirecting the app at the local network.

**Conversation view (H5, v0.1.44).** `Message.references_last`
(`serde(default)`) is the last id of `References`, read from the headers at
the start of the body a refresh already fetches (`BODY.PEEK[]<0.65536>`;
the ENVELOPE has no References, so there is no extra FETCH item) with
`header_values`, and checked by `thread_id` like `message_id`. A last id
that fails the check leaves it empty; an earlier id is never used instead.
The bridge passes it as `refsLast`, and now `inReplyTo` for all mail (it
had been drafts only, so received and sent mail never reached the page
with it). The reading pane shows **In this conversation** under the text
and above the attachments (`#md-convo`, `fillConvo`, `conversationOf`,
headed by an `h3` that labels the section): every held message linked
transitively by `messageId`, `inReplyTo` or `refsLast` (`convoKeys`), never
by subject, oldest first, each row the sender, date and first 140
characters (`bodyNow` or else the preview; never `m.body`, never the disk).
The other rows are buttons that open that message, and the focus goes to
its row; the open one is a `div` with `aria-current` and "Open". Sent mail
joins, and RATA's own record of a sent reply now carries `inReplyTo`
(`deliver()`). Spam never joins and an open spam message shows none;
drafts and gone mail are left out (`convoJoins`); the same Message-ID held
twice is one row (a Gmail label's copy loses to the inbox's). The walk is a
breadth-first search over an index built from `S.messages`, stopping at
`CONVO_MAX` (200), nearest first, and says so when it stops (a chain of 300
in ~50 ms in the harness). The section stays hidden when nothing else is
linked. Mail stored before 0.1.44 has only `messageId` (drafts also
`inReplyTo`) and links by what it has until it is fetched again; there is
no backfill, and a message listed from its envelope alone (`UNREADABLE`)
has no References. Any sender who knows a Message-ID can place mail in a
conversation by writing `In-Reply-To`, as in every mail client; each row
names its sender, and spam is kept out.

**Email only.** v0.1.6 removed Slack, Discord, texts, Google/Microsoft sign-in,
translation, the AI brief, the Extras/Connections/launcher screens and the
operator config rows — all were switches with nothing behind them, and
translation and the AI brief sent mail text to Google or Anthropic. `tidyV6`
strips their leftovers from an older saved workspace. Translation and AI came
back in v0.1.17 through RATA's own relay, and signing in to a Microsoft
*mailbox* in v0.1.39 (below); each is real. Adding a mailbox has one
home: Settings → Linked accounts (`openAddMailbox`).

**The website's app page.** In a browser, `app.html` shows one dismissible
banner at the top of the workspace (`#web-note`, `webNote()`): "RATA reads
your mail in the desktop app. This page keeps your account and settings.",
with Download and Dismiss (kept in `localStorage` as
`rata_web_note_dismissed`). The markup starts hidden and the app never
shows it (harness check, licensed and unlicensed).

Stored, and acted on for real: the interface keeps every fetched message, so
mail persists between launches and is searchable. Read, unread, star, delete and archive change RATA's
copy at once and then the real mailbox (`serverAct` → `/api/mail/act` →
`change_messages` → `rata_mail::imap::act`), one connection per mailbox for a
whole selection. `act` never permanently deletes (Trash is a move to the
server's declared `\Trash`, else, on a server that declares none, to a
folder named exactly Trash, Deleted Items, Deleted Messages, Bin,
INBOX.Trash or INBOX/Trash, in any case — `TRASH_NAMES`, `trash_in`, BUG-M,
v0.1.43: never a name that only contains the word, a `\Noselect` folder, the
inbox, or one declared or read as something else; none of those means
refused) and never acts on a UID it cannot vouch for (UIDVALIDITY must
match; only UIDs still present are touched). Deletions are also remembered
in `S.gone` so sync does not restore them, but only once the server has
said yes (below), and a sync takes the server's read/starred for messages
it already holds, except that sent and draft mail is never made unread.
**The page never hides mail the server kept (BUG-M, v0.1.43).** Delete,
Archive, Move to and Move to inbox / Not spam, one message or a selection
(`leave`, `bulkLeave`, bulk Delete), take mail off the list at once through
`takeOff`, which holds it in `LEAVING` (`isGone` honours it, so a refresh in
between cannot bring it back); it goes into `S.gone` only when its group's
answer is `ok`. Any other answer (`no-place`, `stale`, `net`, `auth`, none)
puts it back, text and all (read from disk before it left), and the toast
says "Nothing was changed — <mailbox>: <why>"; a partial selection says how
many came back. Until 0.1.43 a refused Delete had already hidden the
message for good. A move out of Gmail's archive is a COPY (`Acted::Copied`,
`copied: true` on the wire), so it leaves the list without `S.gone`.
**Mail of a removed mailbox (I6, v0.1.45)** is RATA's copy only:
`linkedOf(m)` finds a message's linked entry by id, else by address, and
without one `serverRef` is false, so Delete goes straight to `S.gone`,
read and star stay local, and Archive, Move and opening in full are not
offered; before, every action on it was refused with "not linked in RATA"
and the mail came back. **A message's generation (I6).** Ids stay
`<key>_<place><uid>`, but the page counts UIDVALIDITY as part of a
message's identity: when a refresh, a folder read or a server search
brings mail of a folder under a new UIDVALIDITY, `settleGenerations` drops
held mail of the old one (not through `S.gone`, sparing `LEAVING` and
`ownDraft`, with its gaps and what the session keeps by id), `take` and
`loadBox` replace a record of another generation whole instead of merging
(which had shown one message while actions reached another), `S.goneV[id]`
keeps each deletion's UIDVALIDITY so `isGone(id, v)` never hides the new
generation, and `heldKnown` reports one generation per folder. A listing
alone (`present`, `archives`) never triggers it, so a folder rebuilt empty
keeps its old mail until new mail arrives. **Fields on held mail (I6).**
`takeFields` copies `inReplyTo`, `refsLast`, `unsub`, `toAll`, `cc`,
`messageId` and `replyTo` onto held mail whenever it is fetched again
(refresh, folder, re-read, older mail, search) and never clears one; mail
never fetched again keeps what it had. In the app a failed refresh or link
says "RATA could not refresh your mail: …" or "RATA could not link …",
never "download the app". What is
missing is scale. **New mail by itself, and only what is new (v0.1.27).**
Until 0.1.27 the desktop app never refreshed on its own — boot only synced
website accounts — so mail arrived only when someone pressed Sync. Now
`startAutoSync` (started by `takeStanding` once the licence is good) refreshes
0.8 s after start, every five minutes, and on `focus`/`visibilitychange`
after a minute away; `serverSync(p, quiet)` runs one refresh at a time
(`SYNCING`), and a quiet one only speaks, or writes to the audit log, when
there is new mail. Each refresh sends `known` (`heldKnown`: per mailbox and
fixed folder, the newest UID held and its UIDVALIDITY) → `refresh_mail` →
`fetch_newest(…, known)`: for a known folder the engine asks `UID FETCH
since+1:* (UID)` and downloads only those (at most `NEW_MAX`, 200), and sends
read/starred for the newest `limit` as `flags` (`FETCH n:* (UID FLAGS)`, no
text), which `absorbMail` applies; an unknown folder or a changed
UIDVALIDITY gets the newest `limit` whole, as before. So a refresh with
nothing new is a sign-in and a few hundred bytes a folder. **Gaps (v0.1.28).**
When more than 200 arrived in one folder, the engine returns a `Gap`
(folder, UIDVALIDITY, `top` = oldest UID downloaded now, `floor` = newest
held before); the app adds the mailbox (`MailGap`), the page keeps it in
`S.gaps` (`addGaps`), and `fillGaps` pages it with `fetch_older` from `top`
down — one page a gap after each automatic refresh, and first in Load older
mail, whose button also shows while a gap is open — until a page reaches
`floor` (closed), comes back empty (closed) or stale (dropped; the next
refresh reads the folder afresh). Offline, a gap is kept. Before 0.1.28 the
messages in between were never fetched at all, since Load older mail pages
below the *oldest* message held. **Following the server (BUG-C, v0.1.43).**
Each refresh also lists which UIDs the inbox, Sent, Spam and a non-Gmail
Archive still hold (`imap::Present` {folder, uidvalidity, floor, next,
uids}, `Newest.present`, per mailbox `MailPresent`, `present` on the wire),
on the same connection after each folder's refresh. The window is the
newest `PRESENT_WINDOW` (2 000) messages by position: a folder that size
or smaller is listed whole; a bigger one asks one `FETCH <EXISTS-1999>
(UID)` for its floor (UIDNEXT−2000 was rejected: UIDs are sparse where mail
is archived often). The search is `UID SEARCH UID floor:(UIDNEXT-1)`, so
only UIDs that existed at SELECT are judged and mail arriving meanwhile is
never taken for gone; only a tagged OK is believed, and a refusal sends
nothing. Drafts are skipped (listed whole already) and so is Gmail's All
Mail (`settleArchives`). The page (`settlePresent`, in `absorbMail` after
`settleArchives`) drops held messages of that mailbox, folder and
UIDVALIDITY with `floor <= uid < next` that are not listed, not through
`S.gone`, sparing `LEAVING`, `ownDraft`, anything outside the window
(which covers `srvFound`) and everything this refresh brought. So mail
deleted, archived or filed on the phone leaves RATA too, and on Gmail a
message archived elsewhere leaves the inbox instead of showing twice.
Before 0.1.43 it stayed for good, and Delete on the stale copy said
"Deleted" while nothing happened. Gaps too: a gap whose mailbox is no
longer linked (or answers `unknown`) is dropped, Remove drops that
mailbox's gaps, and Load older mail pages normally when the gaps brought
nothing, so a parked mailbox's gap never blocks the others.
**Notifications (v0.1.29).**
`tauri-plugin-notification`, called from Rust only (`notify_mail`, no JS
plugin permissions): after an automatic refresh brings unread inbox mail
while the window is hidden or unfocused, `notifyNew` asks for one
notification — sender and subject for one message, "N new messages / From
…" for several, or only "1 new message" if Settings → New-mail notifications
says so (`S.settings.notify`: full | count | off). Spam, filed and sent mail
never notify, and nor does a refresh the customer asked for. The words are a
stranger's, so `notify.rs` makes them one plain line: no control or
bidi-control characters, 80/200-character caps, and on Linux the body's
`& < >` escaped, since notification servers read markup there (seen on the
D-Bus wire with `dbus-monitor`). Not seen on a real desktop: Windows needs the
installed app's identifier as its AppUserModelID (the plugin sets it outside
`target/`), macOS may ask permission the first time.
Before 0.1.27 every refresh re-downloaded up to 64 KB of each of the newest
15 in four folders. Older mail comes 50 at a
time through **Load older mail** (`loadOlder` → `/api/mail/older` →
`older_mail` → `rata_mail::imap::fetch_older`), which pages by position below
the oldest UID RATA holds, and a newly linked mailbox gets one page straight
away.
**Where mail is kept (v0.1.13).** Each message is its own record in
IndexedDB (`rata-mail-<uid>`, store `messages`, key = id, value = the
message as JSON); everything else in `S` stays one small `localStorage`
entry, marked `store:'idb'` and written without `messages`. `S.messages`
was still all in memory then (0.1.21 moved the text out — below). `save()` finds
what changed by comparing each message with how it was last stored
(`MS_HELD`: the body by identity — nearly all the bytes, almost never
changed — and the rest as JSON), and writes only that, plus deletions, in
one transaction started in the same turn. At 10 000 messages a star costs
~27 ms (it was ~480 ms comparing whole JSON). `load()` reads both, merges
(the `localStorage` copy wins), drops anything in `S.gone` — a delete the
window closed on still holds — and moves mail an older build kept in the
entry into IndexedDB, rewriting the entry only after IndexedDB confirms.
If IndexedDB will not open (5 s timeout) mail stays in the entry the old
way and `loadOlder` keeps its `STORE_SOFT_CAP`; the two halves merge the
next time it opens. Deleting the account deletes this database too.
Verified in the packaged WebKit build: 1 500 messages (12 MB) written,
the app killed and restarted, all back with bodies and a star intact.
**Text on disk, not in memory (v0.1.21).** The database is version 2 with
two stores: `meta` (each message without its text — what start-up reads
and `S.messages` holds) and `messages` (the text, as JSON with a `body`
field). The text store keeps the old store's name on purpose: the upgrade
from version 1 leaves every whole-message record where it is and only
copies the rest of each into `meta` (`textOf` reads `body` from either
shape). Rewriting the text instead took 30 s at 10 000 messages in
Chromium here, because writes are what is slow; this way it is ~2 s once.
A message that arrives with `m.body` (sync, older mail, re-read, sent,
import) keeps it until the next `msFlush`, which moves it to `BODY_PUT`
(waiting), then `BODY_FLY` (being written — so the next save does not
write it again, which once made a relaunch wait out a 48 MB write), then
disk. **Never read `m.body`**: use `bodyNow(m)` (text if to hand, else
undefined) or `bodyOf(m)` / `bodiesOf(list)` (read from disk in one
transaction; a small LRU `BODY_CACHE` keeps what was read lately and is
only filled, never overwritten, by a read). Search (`bodiesMatching`)
walks the text store with a cursor and parses a record only when its raw
JSON matches; batching with `getAll` was slower. Export reads every text
back into the file. Without IndexedDB the old way stands: text stays on
each message in the `localStorage` entry. At 10 000 messages (48 MB of
text) in Chromium: messages held 48 MB → 2.9 MB, JS heap 103 → 13 MB,
start-up ~0.9 → ~0.75 s, search 65 → ~480 ms (it says "Searching…").
Verified in the packaged WebKit build: a 0.1.20-format mailbox of 3 000
messages upgraded in place, star kept, no text in memory, opening, search
(~250 ms), reply quoting from disk, and the same after a restart.
**The list draws a screenful (v0.1.16).** `vlist(sc,pool,tail)` puts a
spacer as tall as every row in a list and draws only the rows in view plus
`VL_MARGIN` either side, moved with `translateY`; `vlDraw` redraws on
scroll (one per frame), resize, and a `ResizeObserver` width change. Every
row is one height — subject, preview and badges each one line
(`.m-meta` no longer wraps; a missing subject shows "(no subject)") — set
as `--vl-h` from a measured row, so row i is at i × stride. A list that is
hidden when drawn (no height to measure) draws its first `VL_BLIND` rows
until it is shown. Clicks, ticks and drags are handled on the list
(`wireRows`), not per row, since rows come and go; ticks live in `SEL`, so
they survive scrolling. `vlist` restores the spacer's height before the
scroll position, or a redraw (a star) jumps to the top. At 10 000
messages drawing the list takes ~12 ms (it was ~800) and a star click
~30 ms in all. Side-by-side columns use the same list; a column that is
rebuilt drops its listeners when it finds itself detached.
**Bodies are decoded (v0.1.7).** Fetch takes `BODY.PEEK[]<0.65536>` — headers
and the start of the message, since the text cannot be decoded without the
headers that declare its encoding — and `body::read` parses it with
`mail-parser` (with `full_encoding` for CJK charsets): the text/plain part if
there is one, else `html::to_text`, our own HTML reader, which keeps block
structure and every link's address as `text <url>` and drops styles, scripts
and hidden preheaders. `html.rs` runs on a stranger's input, so its tag scan
is bounded (`MAX_TAG`) and linear; keep it that way. A part cut by the 64 KiB
limit, or text past `BODY_CHARS` (16 000), sets `truncated` and the reading
pane says so. Mail stored
by an older build (no `bodyV: 2`) is re-read by UID (`reread_mail` →
`rata_mail::imap::fetch_uids`), 50 per mailbox after each sync and at once
when opened. **A reply RATA cannot parse (v0.1.39)** no longer fails the
page. GreenMail sends `BODY[]<0>{n}` with no space, imap-proto rejects it,
and async-imap then reads nothing more on that connection; so `collect`
tells a parse failure from an I/O failure (`unreadable`), keeps what
arrived, and `recover` fetches the rest on a fresh connection, at most
`REDIALS` (2) more per request since each is a sign-in. The message whose
reply cannot be read is listed from its envelope with `UNREADABLE` as its
text and `truncated` set, so opening it fetches it whole; anything left
when the redials run out is listed as `NOT_READ`. A connection that really
fails still fails the folder. Errors never carry message text or FETCH
bytes: `reason()` makes each one line of at most 200 characters, a fixed
sentence for a parse failure, naming the folder (`place`); a server's own
NO/BAD text is kept, cut to that one line. Stream listings (flags, UID
lists, LIST, STORE) go through `drain`, so one cut short by an error is a
failure, never a complete answer. SMTP says as little (SEC-4, v0.1.41):
`smtp::hear` reads a reply line up to `LINE_MAX` (1 KiB) and refuses a
longer one, and checks the reply code as bytes (`line[..3]` panicked on a
multibyte character there); every server sentence in an SMTP error goes
through `said`, one line of at most 200 characters with no control or
bidi-control characters, and while signing in the password, its base64,
the `AUTH PLAIN` string, the token and its XOAUTH2 string are taken out
wherever they appear, in any case, then every run of 16 or more base64
characters, all before the line is cut. IMAP sign-in follows the same
rule (SEC-5, v0.1.42) through the one shared `credential::said`: `login`
and `xoauth2` pass every NO, BAD and other failure through it, with the
password, its LOGIN-quoted form, the LOGIN line, the token and their
base64 as the secrets, each also in async-imap's `Debug` form. While
signing in `said` also removes any start of a secret 8 or more characters
long (what a server's cut-short echo leaves) and keeps a bracketed IMAP
response code such as `[AUTHENTICATIONFAILED]`; `one_line` is `said`
with no secrets, so every IMAP error also loses bidi controls. `said(text,
secrets)` is `said_within(text, secrets, 200)`; `said_within` is public so
Copy diagnostics can keep 600 characters of the app's own longer sentences
(H8). A password refused while linking keeps the server's words after
RATA's advice ("The server said: …", `refusal`, already through `said`;
BUG-M, v0.1.43), since Gmail's "IMAP access is disabled for your domain"
has nothing to do with the password. async-imap
reports each NO or BAD as `code: None, info: Some("…")`, and that is what
the customer sees; the wording is left as it is.
**Opening in full (v0.1.8).** A message marked `truncated`, or with
attachments seen in its first 64 KiB, is fetched whole when opened
(`openWhole` → `open_message` → `rata_mail::imap::fetch_whole`, which asks
`RFC822.SIZE` first and refuses over `WHOLE_MAX`, 60 MB). The full text lives
in `OPENED` for the session only; the attachment list is stored, which is what
puts 📎 in the list. Inline images with a Content-ID are not listed. Saving
(`save_attachment`) re-fetches the message and writes one part into the
Downloads folder: the page supplies only message and index, and Rust picks the
folder, cleans the name (`safe_file_name`: no path parts, Windows device
names including `COM¹`–`³`, `LPT¹`–`³`, `CONIN$` and `CONOUT$`, bidi
controls that disguise `.exe` as `.pdf`, or runs of blank space that push
`.exe` out of view; every run of blanks is one space, none before the last
extension) and creates the file exclusively (`write_new`, `name (2).pdf`).
RATA never opens a saved file. **Disguised programs (v0.1.40).**
`rata_mail::names` holds `safe_file_name` and `looks_disguised` (both moved
out of the app's `core.rs`); `looks_disguised` is true when a document
extension is followed by a program one (`invoice.pdf.exe`), and that name
is kept as sent. The engine marks such an attachment `disguised` wherever
it lists attachments (`body::Attachment`, `serde(default)`, so a list
stored earlier reads `false`); the reading pane labels it ("a program
(.exe), not a PDF") and asks before saving (`askDisguised`, `#warn-ov`:
Cancel focused, Escape, Enter and the backdrop all cancel, only **Save
anyway** goes on). `save_attachment` and `read_attachment` refuse it
without `confirmed` (kind `needs-confirmation`, `consented`), judged from
the name as fetched, never from what the page says, so the page alone
cannot skip the question; a list stored before the flag gets the question
when Rust refuses. Only disguised names are asked about: `setup.exe` says
what it is and saves without a question (the owner's choice). Since
SEC-6 (v0.1.42, review P3-1 and P3-4) `safe_file_name` drops every format
character (Unicode Cf) and the rest of Default_Ignorable_Code_Point
(`invisible`), so `invoice.pdf<U+200B>.exe` is caught; `looks_disguised`
also splits on eleven characters drawn as a full stop
(`looks_like_a_dot`: U+2024, U+FF0E, U+FE52…); and the lists add the
documents csv, rtf, html and htm and the programs cpl, reg, url, iso and
one (`DISGUISED_AS` in the page labels the new documents). Convert asks
too, though its button never shows for such a name, since the last
extension is a program's. **Marked as downloads (v0.1.40).** Every file
`write_new` saves, attachments and Format Bridge downloads alike, is then
marked as a download (`mark.rs`: a `Zone.Identifier` stream with `ZoneId=3`
on Windows, `com.apple.quarantine` `0081;<hex time>;RATA;` on macOS,
nothing on Linux, never naming the sender), so SmartScreen, Protected View
and Gatekeeper check it; a failed mark never fails the save and nothing is
logged. Neither mark has been seen on a real Windows or Mac.
**Sending attachments (v0.1.9).** The composer reads picked files only when
Send is pressed and hands them over as base64 (`send_mail`'s `attachments`);
`core::send` refuses over `ATTACH_MAX` (18 MB) before dialling, and
`compose::render` builds `multipart/mixed` with every part base64, so the
`=_rata_` boundary can never occur inside one. File names are cleaned for
headers (`compose::file_name`: no path, no control characters, at most 150
bytes so every header line stays under 998) and sent both as encoded-words in
`filename=` — what Gmail and Outlook send and read — and as RFC 2231
`filename*`. SMTP DATA gets a minute plus a second per 64 KiB.
**Forwarding (v0.1.11).** `forwardMessage` fetches the whole message first if
the stored copy may be partial, and the composer holds `CMP_FWD`: the
original's mailbox, UID, UIDVALIDITY and attachment indexes — never bytes.
`send_mail` takes one `draft` (clippy's 7-argument limit; `core::Draft`), and
`core::send` fetches those attachments from the mailbox (`forwarded_files`)
before the size check. The drag-between-panes forward uses the same path; it
used to announce files it never sent.
**One draft, kept whole (v0.1.15).** The composer's draft is To, Subject,
body, From, `CMP_FILES`, `CMP_FWD`, `REPLYING` and `DRAFT` (what it is, for
the title). Closing keeps all of it; Compose reopens it; Reply, Forward, a
person's Write and a `mailto:` link call `newDraft()` first; a send clears
it. Until 0.1.15 closing kept the words and dropped the rest, so a closed
forward came back saying "Forwarded message" with no files, and a reply lost
its thread. Because a reopened draft can carry a thread nobody sees, its
title says what it is ("Reply to Ann", "Forward") and **Discard** starts a
blank message.
**HTML mail (v0.1.12) — three locks, keep all three.** (1) `html::safe`
sanitises with ammonia: scripts, handlers, forms, frames, `<meta>`, `<base>`,
relative URLs and any `href` that is not http, https or mailto go; layout, `<style>`, an allowlist of
CSS properties and `cid:` pictures (inlined as `data:`, images only, capped)
stay. An inline picture is charged against `INLINE_ALL` (4 MiB) once for
**every** `cid:` reference; one that does not fit in full is left out
everywhere and the text stays, so the output is bounded however often a
picture is named (SEC-1: one 1 MiB PNG named 200 times built 280 MB before).
Ammonia does not treat `background` as a URL, so `attribute_filter` keeps
it only for an `https://` picture or `data:image/{png,jpeg,gif,webp};base64,`
(what a `cid:` becomes) and drops it otherwise (SEC-4). (2) The interface shows it in `<iframe sandbox="allow-popups">` —
never add `allow-scripts` or `allow-same-origin` (together they undo the
sandbox, and this page can call the app), nor `allow-top-navigation`,
`allow-forms` or `allow-popups-to-escape-sandbox`. `allow-popups` is there
only because a click on a link is a request for a new window (`<base
target="_blank">`), which is how Rust hears of it; no window ever opens (see
*Links* below). (3) The frame's own CSP
(`frameDoc`) fetches nothing but `https:` pictures after **Load images**; the
app CSP's `img-src … https:` exists only so that can work, and its
`frame-src 'none'` also blocks a frame navigating itself. Verified in the
packaged WebKit build with a hostile message that bypassed the sanitiser: it
rendered, and neither its script nor its `onerror` ran. Testing traps found
on the way: Chromium fires a `request` event for pictures its CSP then
refuses — count what reaches the network (a route handler or a real server),
not request events; and the desktop harness
(`desktop/rata-app/harness/ui-harness.mjs`) serves the page under the app's
real CSP from `tauri.conf.json`. Opening a message fetches it whole when it may have HTML
(`m.html`, or unknown for mail stored before 0.1.12).
**Links (v0.1.14).** The app's webview holds the bridge to everything, so no
outside page may ever load in it; links go to the customer's browser, and
only http and https do (`links.rs`: `classify` refuses `file:`,
`javascript:`, `data:`, custom schemes, and `user@host` addresses that
disguise the real host). The main window is built in `main.rs` rather than
from the config (`"create": false`) so it can carry two handlers.
`on_new_window` receives every click on a link in a formatted message (the
frame's only permission), always answers `Deny`, and tells the page via
`eval` of a JSON literal: `window.__rataLink({kind:'web',url,host})` shows
"Open this link in your browser?" naming the host — the words of a link can
say anything — and only **Open in browser** calls `open_link`; `kind:'mail'` opens
RATA's composer (`writeTo`, with an empty body). `on_navigation` keeps the
window only where this platform serves the app (`links::navigation`, SEC-4):
`tauri://localhost` on macOS and Linux, `http(s)://tauri.localhost` on
Windows, on the default port with no user name, plus exactly `about:blank`
and `about:srcdoc`. `blob:` is refused from every origin, the app's own
too (the app saves what it makes through `save_file` and never navigates
to a blob), and so is any other `tauri://` host or port. It
sends mailrata.org links to the browser and refuses the rest — the licence
box's "Sign in at mailrata.org" used to replace the app with the website,
with no way back. In the Text view, addresses are linked (`linkify(el,text)`, which since
H12 builds text nodes and `<a>` elements, never an HTML string; a web
address over 64 characters shows as its host and the start of its path
with `…`, its `href` and `title` whole) and
open straight away, since the text *is* the address; bridge.js hands every
http(s) link on the app's own page to `open_link`, and `open_link` checks
again in Rust whatever the page asked. Verified in the packaged WebKit build
(a click in the frame reached `on_new_window`, the dialog named the host,
Open passed exactly that URL to `xdg-open`, `javascript:` did nothing,
`location.href` to example.com was refused). Testing trap: under Xvfb here
the webview draws at 3/4 width but takes clicks at full width — multiply a
screenshot's x by 4/3 before clicking with xdotool; y is unchanged.
**AI (v0.1.17), through RATA's relay.** Summarize, Translate (reading pane)
and Run briefing (Assist) call `askAI` → bridge `/api/ai` → `POST
https://mailrata.org/api/ai` with the licence token as the credential
(`rata-next/app/api/ai/route.js`, rules in `lib/ai.js`): plan-gated
(`translate` for Base, `ai` for Pro — `lib/plan.js`), Haiku 4.5, text capped
(12 000 chars; the briefing sends the start, 1 500 chars, of the 25 most
recent messages from the last 14 days, each named only by its place,
`"1"`..`"25"`, never by `m.id`, which carries the mailbox key and so its
address; I1, v0.1.45, harness check), every prompt fences the email as
untrusted (`fence` turns any `<` that starts an `email` tag, in any case or
spacing, opening or closing, into `‹`, so a message cannot close its fence
early; `clean` strips control and bidi-control characters from what goes in
and from each task that comes back), the task list is parsed strictly (ids must be ones sent). Each
request's worst case (`worstCaseMicro`: every prompt byte as a token, plus
64, plus `max_tokens` of output) is reserved before the call, settled to
Anthropic's reported token use after, and refused at the cap atomically
(`ai_reserve`/`ai_settle`, `database.sql` §5; v0.1.40, SEC-2): a refused
reserve is 429, a database error 503 with no model call (it fails closed),
a missing function 503 "AI is not switched on for RATA yet." with only the
server log naming section 5, and a settle that fails keeps the whole hold.
One worst case is about $0.06 (a 12 000-character CJK translation), so
`AI_MONTHLY_CAP_USD` must stay well above that. Nothing is logged but a
status. The app asks once before the first AI use (`aiOk`, `AI_NOTICE`),
never sends anything on its own (opening Assist runs the local briefing), and
falls back to the local summary/briefing with the reason when AI cannot be
used. Flags are stored as `m.flag` and shown in the list. Translations live
in `TRANSLATED` for the session. **Long mail and long answers (BUG-A,
v0.1.43).** The page cuts what it sends (`aiCut`): 12 000 characters of one
message (`AI_TEXT`, exactly what the relay reads and what the privacy page
promises), 300 of a subject, 200 of a sender, 1 500 of each briefing
message, with control and bidi-control characters removed first, since
JSON spells each as six; it knows for itself that it cut (`aiLonger`), so
a translation still says "only the start of this long message". The
briefing drops its oldest messages until its JSON is at most 70 000
characters (`AI_BRIEF_JSON`) and re-flags only what was sent. Before 0.1.43
an opened message went whole, and anything over the route's raw 80 000
guard was refused. The relay reads `stop_reason`: a translation or summary
that stopped at `max_tokens` is `cut: true`; a briefing that did is 502
"The briefing was cut short. Try again." (`reason: 'cut'`, still settled),
and the page keeps every flag and says "Flags from earlier briefings are
kept." The briefing's `max_tokens` is 3 000 (was 1 200, which an answer
naming most of the 25 could run past), and the hold follows it through
`worstCaseMicro`. Translate keeps `max_tokens` 4 096, so a long CJK
translation can now be reported cut. A deploy without `LICENCE_PUBLIC_KEY`
answers 503 "AI is not switched on for RATA yet." (`not-configured`) and
logs the variable's name and LAUNCH.md §2, never a value, rather than
telling a licensed customer they are not. **Two things found on the way:** the live
mailrata.org is an old deploy (`/api/licence/renew` is 404 there), and even
the current code never answered the app's cross-origin preflight, so licence
renewal from the app has never worked — `lib/cors.js` now allows exactly the
app's origins on `/api/ai` and `/api/licence/renew`. Neither AI nor renewal
works for testers until the site is redeployed with the variables in
`rata-next/LAUNCH.md` §4 and `database.sql` §5 has been run. Testing trap:
this container's Chromium reports `navigator.language` as `en-US@posix`,
which `Intl.DisplayNames` rejects — `userLang()` takes only the leading
letters.
**Format Bridge (v0.1.18).** Every input is read into one model —
headings 1–3, paragraphs, list items, tables, with bold/italic runs — or,
for spreadsheets, every sheet's rows (`BR.blocks` / `BR.sheets`), and every
output is written from it: `.docx` by `blocksToDocx` (a stored zip written
by `zipStore`, with Heading1–3, a list style and a bordered table style —
the old "Word" output was HTML named `.doc`), Markdown, PDF (jsPDF), `.xlsx`
with all sheets, CSV (several sheets → one CSV each in a `.zip`). `.docx` is
read through mammoth's HTML, `.html` directly — both parsed with
**DOMParser**, never `innerHTML`: a stranger's `<img onerror>` put into this
page's DOM, even detached, runs with the app's powers (the old HTML reader
did exactly that). PDF's standard fonts are Windows-1252 only, so
`pdfBlocker` refuses text outside it, naming the script, rather than
producing gibberish. Pictures are not carried over, and the note says so.
Checked by reading the outputs back with LibreOffice (install
`libreoffice-writer-nogui libreoffice-calc-nogui`; the container has only
the core) and in CI with mammoth and SheetJS. **Downloads in the app:** the
webview silently drops a page's downloads, so "Convert & download" and
Export did nothing in the desktop build; `brDownload` now sends the bytes
to `save_file` (core.rs: `safe_file_name`, `write_new`, 100 MB cap). And on
Linux Tauri's `download_dir()` exists only when the desktop has
`user-dirs.dirs` — without it every save, attachments included, failed —
so `commands::downloads` falls back to `~/Downloads`.
**PDF input (v0.1.19).** PDF.js 6.3.289, legacy build (older engines than
the modern one; the Linux webview lags), vendored as `.js` so every server
sends a script type, loaded with `import()` (`pdfLib`) and parsing in its
module worker. `pdfToBlocks` rebuilds structure from positions: runs on one
baseline make a line, and a line keeps its cells (runs more than 0.3 em
apart); two or more consecutive rows whose cell left edges line up
(within max(4pt, 0.35 em)) make a table; the body size is the size most
text is set in and the line step the median gap, so a gap over 1.3 steps
starts a paragraph; larger type is a heading (levels by size); a leading
bullet — including the Symbol/Wingdings private-use ones Word and
LibreOffice write — or number makes a list item, whose indented next line
continues it; a hyphen at a line's end before a lowercase letter joins the
word; a page break continues the paragraph when the last line had no room
for the next page's first word; lone page numbers are dropped. Scanned
(text-less), password-locked and non-PDF files each get their own message.
Limits, named in the note: layout and pictures; Chrome's PDFs draw bullets
as shapes and merge a row's cells into one run, so their lists read as
paragraphs and their tables as lines. Checked against LibreOffice and
Chrome PDFs of known documents (structure matched exactly for
LibreOffice), in CI with a LibreOffice-made fixture and jsPDF-made
hyphen, scan, locked and junk cases, and in the packaged WebKit build
(read in ~0.3 s, saved, read back by LibreOffice).
**Attachments into the Bridge (v0.1.20).** An attachment whose extension the
Bridge reads (`bridgeable`, from `BR_KINDS`) gets **⇄ Convert** beside it;
`convertAttachment` → bridge `/api/mail/attachment/read` →
`read_attachment` → `core::read_attachment`, which fetches it like a save
(`attachment_of`, shared with `save_attachment`) but hands the bytes back as
base64 instead of writing them, capped at `READ_MAX` (25 MB, kind
`too-large`). The page asks by message and index only; the bytes go to
`brIngest` as a `File`, through the same readers as a dropped file, and
nothing is saved until the customer converts.
**Pictures in the message (H11, v0.1.44).** In the app, once the whole
message is in, an attachment named `.png`, `.jpg`, `.jpeg`, `.gif` or
`.webp`, not `disguised`, and not known to be over 5 MB (`b`, its size in
bytes, which `asAttachment` now passes) shows as a thumbnail above the
attachment list (`pictureable`). At most `PIC_SHOW` (6) show, then "Show N
more". **One fetch for all of them (I4, v0.1.45):** `read_pictures`
(bridge `/api/mail/attachment/pictures`) takes up to `PICTURES_ASK` (24)
indexes (`too-many` past that, before anything is dialled), fetches the
message once through `whole()`, and `core::pictures_fetched` judges each
from the fetched name and bytes, answering with no data and a reason for
`gone`, `disguised`, `too-large`, `not-a-picture`, or `left-out` once
`PICTURES_TOTAL` (30 MB decoded) is spent; `body::attachments_at` parses
once for several parts. The page asks once when the message opens, once
for "Show N more", and again only for what was left out. Until 0.1.45
each picture was its own `read_attachment`, a sign-in each. Pictures are kept in
`PICS` for the session, at most `PIC_KEEP` (48); a failed read is not kept,
so opening the message again tries again, and until then the tile goes and
the file stays listed. **What a picture is comes from the bytes alone:**
`core::sniff_image` reads the magic numbers (PNG, JPEG, GIF87a/89a,
RIFF…WEBP), and `hand_fetched` sets `Handed.picture` only for one of those
four at `PICTURE_MAX` (5 MB) or less; `commands::read_attachment` and the
bridge pass `picture`, null otherwise. The page draws only
`data:image/<picture>;base64,` in an `<img>` built through the DOM, never
from the name or the declared type, and refuses data longer than 5 MB
would encode to; an SVG or an HTML file named `.png` stays listed and is not
shown. The alt text and caption are the name without control, bidi or
invisible characters (`picName`). A click opens `#pic-ov` with the same
`data:` image: no navigation, no window, no `blob:`. Escape, Close and the
backdrop close it, Escape closes only the picture (not the message), the
focus goes back to the thumbnail, and `keyDialog` counts it as open, so
message keys do nothing behind it. The CSP is unchanged (`img-src data:` was
already allowed), and so is saving. `--thumb` (168 px) is the thumbnail's
size. Not seen in a packaged build.
**The Sent folder (v0.1.22).** A refresh reads the newest mail of the
inbox and then of Sent, on the same connection (`fetch_newest`); a mailbox
without a Sent folder, or whose Sent will not open, still brings its inbox.
Sent is found by its RFC 6154 `\Sent` attribute, else by name
(`SENT_NAMES`: "Sent", "Sent Items", "INBOX.Sent"…, exact match only — a
folder called "Unsent ideas" is not it). `Folder` (Inbox | Sent) is part of
every request that names a message by UID — open, save, convert, re-read,
older mail, act, forward — because a UID means nothing outside its folder:
`select` opens the right one and the UIDVALIDITY check is against *that*
folder's, so a Sent UID can never touch the inbox message with the same
number (tested). Sent mail has ids of its own (`<key>_sent_<uid>`),
`folder:'sent'`, and the first recipient (`to_name`/`to_addr`); the bridge
marks it `sent:true` with `toName`, which is how the interface already knew
sent mail — so it lands in the Sent filter, stays out of the inbox and the
new-mail count, and joins that person's thread in People (`mailRecord`
sets `toId`). Older mail pages each folder on its own (`historyDone`,
`sentDone`). Sent mail has Forward and Delete but not Archive. **No
doubles:** `send` now returns the Message-ID it wrote (`Delivered`), kept on
RATA's own record of the message; when the provider's copy arrives from
Sent with the same id it replaces that record (`localTwin`). Mail sent by
0.1.21 and earlier kept no id and is matched by subject, recipient address
and a quarter of an hour. RATA does not APPEND to Sent itself: a provider
that does not file mail sent over SMTP (Gmail and Outlook do; not every
provider does) shows only RATA's own record, as before.
**Updating itself (v0.1.23).** `update.rs`, with `tauri-plugin-updater`.
The app checks the release tagged `updater` (a fixed address — 0.x
releases are pre-releases, which GitHub's "latest" link skips) a few
seconds after start and twice a day, and offers "RATA x is
ready." with a **Restart to update** button (`#upd-bar`, above the licence prompt so a copy
whose licence check fails can still update). Nothing downloads until that
click. An update installs only if its minisign signature verifies against
the public key in the app's config, **and** the signature names the version
the feed announced (`requireSignedVersion`) — so a feed cannot relabel an
older signed build as newer. The key is not committed: `plugins.updater.pubkey`
is empty in `tauri.conf.json`, and the release workflow writes
`src-tauri/tauri.updater.conf.json` (gitignored) from the
`RATA_UPDATER_PUBKEY` secret and passes it with `--config`, which also turns
on `createUpdaterArtifacts`; the private key is `TAURI_SIGNING_PRIVATE_KEY`
(+ `_PASSWORD`). Half the pair fails the build. A build with no key
registers no updater and checks nothing (`has_key`). The publish job then
maps each signed asset to its own key in the feed — `linux-x86_64-appimage`,
`linux-x86_64-deb` (installed through pkexec), `windows-x86_64-nsis`,
`darwin-{aarch64,x86_64}-app` — never a bare `linux-x86_64`, which would
hand a .deb install the AppImage; the feed never moves backwards. **A
feed file per kind of installation (BUG-R, v0.1.43).** The app reads
`latest-<target>-<arch>-<installer>.json` on the `updater` release first
(tauri.conf.json's first endpoint, `latest-{{target}}-{{arch}}-{{bundle_type}}.json`,
which the plugin fills in and passes over on a 404), then `latest.json`.
The publish job writes one file per kind of installation, each with its
own version, plus `latest.json` with this release's signed installers only
(`harness/feed.cjs`, a pure function tested by `feed.test.cjs` in the gate
and in Desktop CI). A platform whose installer came off a release keeps
its own file at the version it had, seeded the first time from the
previous `latest.json`. It is not kept in `latest.json`: under
`requireSignedVersion` the plugin compares each installer's signed version
with the feed's single `version`, so an older installer announced as newer
would be offered and then refused as a bad signature. Copies up to 0.1.42
read only `latest.json`. A feed with no entry for this computer
(`TargetNotFound`/`TargetsNotFound`, `not_here_yet`) is no update, not an
error, and Install says "There is no update for this computer yet.";
`check` reports it as `notHere`, and the Settings row and a manual check
say the same words, not "the newest version" (I1, v0.1.45);
"cannot update itself" is for an unsupported OS or architecture and for
failed installs, with the plugin's reason in brackets. The first per-
installation files are written by 0.1.43's publish job. The
Tauri CLI ignores a `TAURI_CONFIG` variable: use `--config`, and it signs
against the config's pubkey, so an empty one fails the bundling step.
Verified here with a throwaway key: a 0.1.23 AppImage fetched a local feed,
installed a signed 0.1.99 (the file on disk replaced, the app back up as
"RATA 0.1.99 beta", still signed in), and refused both a wrong signature and
a genuine 0.1.23 announced as 0.1.99, leaving the file untouched.
**The plan in the app is the licence's (v0.1.24).** `tier()`/`can()` read
`planKey()`: on the website the subscription the site recorded
(`S.settings.plan`), in the desktop app the licence's plan (`LIC`, from
`/api/licence` → `licence_status` at boot, and the `rata-standing` event
bridge.js fires after it settles and renews). Until 0.1.24 the app read the
website's record, which it never fills, so every paying tester saw "No plan
yet" and Side by side locked behind Pro. Settings → Current plan now shows the
licence's own sentence ("Licensed for RATA Pro until …").
**Archive and Spam (v0.1.25).** `Folder` gains `Archive` and `Junk`, and
`places()` finds Sent, Spam and Archive from one LIST — by RFC 6154
attribute, else by exact name (`SENT_NAMES`, `JUNK_NAMES`, `ARCHIVE_NAMES`);
never the inbox, never one folder for two purposes, never a name that only
contains the word ("Spam reports", "Old archive stuff"). **Gmail has no
Archive folder:** it declares only `\All` ("All Mail"), which holds the inbox
and Sent too, so reading it whole would show every message twice (until
0.1.38 Gmail showed no archive at all; see *Gmail's archive* below). A refresh reads
the newest mail of each folder the mailbox has; any that fails is left out.
In the interface `inInbox(m)` (received, in the inbox) replaces `!m.sent`
wherever "the inbox" was meant — the list, Unread, `#nc-inbox`, the digest,
categories, the briefing and the to-dos — so Archive and Spam only appear
under their own chips (shown when they hold anything), in Starred (archive
only) and in search (labelled). Spam is never linked to a contact
(`mailRecord`): a forged From would put it in their People thread. A spam
message says it was in Spam and offers **Not spam**, an archived one **Move
to inbox** — both `Action::Inbox` (a MOVE to INBOX; the message leaves RATA
and the next refresh brings it back as inbox mail, since a moved message
gets the newest UID). Older mail pages every folder (`FOLDER_DONE`).
Archive by exact name is also where Archive files mail (v0.1.39:
`destination` asks `name_of(Folder::Archive)`, the same `places()` answer a
refresh reads), so a server that declares no `\Archive` can be archived to;
until then RATA showed such a folder but refused to archive into it. On
Gmail that means a label named exactly "Archive" is now archived into, as
it was already shown as Archive. Trash is the declared `\Trash`, else a
folder named exactly as `TRASH_NAMES` lists (since 0.1.43, BUG-M; see
*Stored, and acted on for real*).
**Your own folders (v0.1.26).** `Folder::Named(name)` — the server's own
name, exactly as LIST gave it (modified UTF-7 and all; `folder_label` decodes
it for showing, joins levels with " / " and drops an `INBOX.` prefix).
`own_folders` is every selectable folder except the inbox, anything with a
special-use attribute (Sent, Archive, Junk, Trash, Drafts, All, Flagged,
Gmail's `\Important`), anything `places()` took by name, and Trash/Drafts/
Outbox by name on servers that declare nothing. Gmail labels are folders here.
A named folder is opened only while it is still one of those (`name_of`
checks the LIST every time), so the page cannot make RATA open Trash or a
folder that is not there, and Move never creates one. Unlike the four fixed
folders these are **not read on a refresh**: Folders (a chip) lists them per
mailbox (`list_folders`, asked each time it is opened, kept in `S.boxes`
keyed by address), picking one reads its newest 50 (`folder_mail` →
`fetch_folder`), and Load older mail in that view pages that folder. Ids are
`<key>_f<12 hex of sha256(name)>_<uid>`; in the page the folder is
`folder:'named'` with the name in `box`, and requests send `{named: box}`
(`folderArg`). Filed mail shows only in its folder's view and in search
(labelled); Gmail keeps the same message in the inbox and under each label,
so `unDup` drops a label's copy wherever the inbox's copy is in the same list
(People threads, search). **Move to** on a message (`Action::Move`, serde
`{"move": folder}`) moves it on the server; it leaves RATA's list and is there
when that folder is opened. **Mailbox attribution fix, same version:**
`mailRecord` used to let the app's `acct` (the address) overwrite the linked
entry's id, so everything that asked "which mailbox?" by id — reply's From,
the per-mailbox chips, Side by side columns, "via" in the reading pane,
drag-to-forward's "already in this inbox" — found none; a reply to mail in a
second mailbox went from the first. Mail is now filed under the id with the
address kept as `mailbox`; `tidyAccts` refiles stored mail on load and on
every sync (also after a mailbox is removed and linked again).
**Several at once (v0.1.30).** The selection bar (Select) gains, in the app
only, **Archive** (the selection's inbox mail, `canArchive`), **Move to inbox**
(archived, spam and filed mail, `canGoHome`) and **Move to ▾** (one of the
mailbox's folders, `bulkFileMenu`; a selection spanning two mailboxes is
asked to pick one). `bulkLeave` takes what a choice applies to off the list at
once, sends it through `serverAct` — one connection per mailbox, folder and
UIDVALIDITY — and says how many could not be moved (sent mail, mail with no
server reference). Before 0.1.30 only read, unread, star and delete worked on
a selection.
**Drafts (v0.1.31).** `Folder::Drafts` (serde `"drafts"`), found like the
others (`\Drafts`, else `DRAFTS_NAMES`) and read on every refresh; ids
`<key>_drafts_<uid>`. `Message` now carries every To (`to_all`), `cc` and
`in_reply_to` (the first id of it, through `thread_id`), and an empty draft
keeps an empty body rather than the "no text RATA can show" sentence. A
draft changes UID each time another device saves it and goes when it is
sent, so a refresh that reads Drafts also returns every id there
(`Newest.drafts`, `UID SEARCH ALL`, capped at `DRAFTS_MAX`; the app wraps it
per mailbox as `MailDrafts`), and the page drops held drafts not listed
(`dropVanishedDrafts` — not through `S.gone`, since nothing was deleted
here); a refresh that could not read Drafts sends nothing and removes
nothing. In the page a draft is `draft:true` (not `sent`), never unread,
never linked to a contact, under its own chip; its pane offers only
**Continue** and Delete. `continueDraft` fills the composer: To from
`draftTo` (To and Cc together — the composer has one address line; a toast
says so), subject (never "(no subject)"), the whole text, attachments as
`CMP_FWD` with folder `drafts` (fetched from the mailbox at send time, like
a forward's), `REPLYING` from `inReplyTo`, and `CMP_DRAFT`, the draft
itself. After a real send `CMP_DRAFT` leaves the list and goes to the Trash
(`serverAct` trash in folder Drafts); Discard leaves it in Drafts. A
formatted draft continues as plain text. From 0.1.41 RATA saves its own
drafts there too (*Drafts saved to the server*, below). (Until 0.1.32 Cc was folded into To here.)
**Cc and Reply all (v0.1.32).** `Outgoing.cc`; `compose::render` writes To
and Cc through `addresses`, which folds between addresses so no header line
passes 998 bytes however many there are (a list of 120 was one illegal line
before). `smtp::recipients` still reads the envelope back out of the
rendered headers — one list, never a second that could drift — now joining
folded lines, reading To and Cc, and naming each address once. `core::send`
takes `Draft.cc` (as typed; empty is none), refuses a bad Cc address by
name, and refuses more than `RECIPIENTS_MAX` (100) people before dialling.
Every message now carries `toAll`/`cc` to the page (the bridge passes
them for all mail). The composer has a Cc line, hidden until **Cc** is
pressed or something fills it (`showCc`). **Reply all** (`replyAllOf`)
puts the sender, or their Reply-To, in To and everyone else in Cc — the
original sender too when a list asked replies to go to it — never the
customer's own linked addresses; it is offered only when there is someone
else, and not for mail stored before 0.1.31, which does not know. A draft
continued now keeps its Cc as Cc.
**Bcc (v0.1.33).** `Outgoing.bcc` is never rendered — no `Bcc:` header,
which is what makes it blind — and is the one deliberate addition to the
envelope: `smtp::envelope` is `recipients(rendered)` (To and Cc, read back
from the headers) plus Bcc, each address once, and `attempt` takes that
list. `core::send` takes `Draft.bcc`, refuses a bad address naming Bcc, and
counts it in `RECIPIENTS_MAX`. `Message.bcc` is filled for Drafts only
(the customer's own blind copies, which finishing a draft must keep);
`continueDraft` fills the Bcc line from it. The composer's Bcc line sits
under Cc, hidden until **Bcc** is pressed (`showBcc`). RATA's own record of
what it sent does not show Bcc, and the provider's Sent copy (which
replaces it) usually does not either.
**Address suggestions (H3, v0.1.43).** To, Cc and Bcc are comboboxes
(`role="combobox"`, one `#cmp-sugg` `role="listbox"`,
`aria-activedescendant`) that suggest people RATA has seen: every From, To
and Cc of held mail plus People (`suggPool`; a contact's name wins), never
spam, drafts or anything that is not email, and only addresses and names,
never text. Each message adds `1/(1+age/14 days)`, so recency and frequency
both count; a match at the start of a word (in the name, or in the address
split at `.@_+-`) ranks above one inside a word, which needs 3 or more
letters (2 found "le" in every "example"); the customer's own linked
addresses rank last, labelled "Your mailbox". Suggestions start at 2
characters, for the address under the cursor in a comma list (split at
commas outside quotes and brackets, as the engine splits it), leave out
anyone already in the field, and show at most 8. Up and Down wrap, Enter or
Tab chooses, Escape closes the list only, and the mouse works. A choice
goes in as `"Name" <addr>, ` (the name without control or bidi characters,
`"`, `\`, `<` or `>`, at most 64 characters), or the bare address; Send
trims a trailing comma. Nothing is stored and nothing goes to the network.
**The engine takes that form** (before 0.1.43 `parse_list` split `"Smith,
Ann" <ann@example.org>` at the comma and refused the list):
`compose::split_list` cuts only at commas outside quotes (with backslash
escapes) and angle brackets, and an unclosed quote refuses the list;
`Address` keeps a cleaned display name (`name()`, `NAME_MAX` 64: a control
character refuses the address, bidi controls, quotes, backslashes and
brackets are removed); `addresses()` writes it quoted when ASCII, as
encoded-words otherwise, still folding under 998 bytes; and
`smtp::recipients` reads the envelope from each `<…>`, so a comma in a name
is never a second recipient. RATA's own record of a sent message keeps the
typed line as `toName` until the provider's copy replaces it. From is
written by the same `named()` (I2, v0.1.45); before, a non-ASCII sender
name was an encoded-word inside quotes, which RFC 2047 §5 forbids (the app
sends no `from_name` yet, so none shipped).
**Keyboard shortcuts (H4, v0.1.43).** One table, `KEYS` in `app.html`,
drives both the handler (a `keydown` on `window`, after every other
handler) and the `?` sheet, so the two cannot disagree (harness). A message
key presses that message's own button (`keyPress`), so it works only where
the button shows, and Delete and Archive go through `takeOff`. Keys are
ignored while a field has focus, with Ctrl, Cmd or Alt held, during IME
composition, while a question overlay is open, or when another handler
already took the key (`defaultPrevented`, which is how H3's suggestion
list keeps Down and Escape). Settings → Workspace turns them off
(`S.settings.keys`); Escape closing the composer does not depend on it. The
composer's subject and body carry `spellcheck` and `lang` (`userLang()`).
**Search the server (H6, v0.1.43).** `rata_mail::imap::search_folder`
sends one `UID SEARCH [CHARSET UTF-8] OR OR FROM {n} SUBJECT {n} TEXT {n}`
with the query as three IMAP literals, waiting for the server's go-ahead
before each, so a `"`, `\`, CRLF or non-ASCII letter can never change the
command; `CHARSET UTF-8` goes only with a non-ASCII query, and a server
that refuses it is reported as unable to search outside plain ASCII (an
ASCII query never carries CHARSET, so there is nothing to retry). Only a
tagged OK is believed; a NO or BAD is an error with the server's sentence,
never "nothing found". `search_query` trims and refuses an empty query,
one over 200 characters, or a control character, before any connection
(kind `query`). The newest 50 matches come through the `fetch_uids` path;
inbox only (Gmail's archive and named folders are refused before a
select). `Rata::search` is behind `usable` and `signed`, results go
through `as_links`, command `search_mail`, bridge `/api/mail/search`. In
the app a local search shows **Search on the server** per mailbox (and
for all at once); results are stored like older mail, labelled with the
mailbox and "Found on the server", and marked `srvFound`, which
`oldestRef` and `heldKnown` skip so no mail in between is missed; Load
older mail, or a refresh that brings the message, takes the mark off.
**Undo send (H9, v0.1.43).** In the app, Send closes the composer and holds
the message `S.settings.undoSend` seconds (Settings → Workspace: 0, 5 or
10; default 5; the website always 0), with "Sending in N s · Undo" in
`#send-toast` (`role=status`; the focus goes to it, so Tab reaches Undo).
Undo brings the whole draft back (`snapCompose`/`restoreCompose`: every
address line and whether Cc and Bcc show, subject, text, files, forward
references, the thread, `CMP_DRAFT`, `CMP_SRV`, `CMP_SIG`, From), and
nothing is trashed at Undo. The count ending calls `deliver()`, the same
path as a no-wait Send; RATA's Drafts copy goes to the Trash only after
success (`sentAfter`), and a failure puts the draft back. Sends go one at
a time in order (`OUTBOX`, `obRun`); a draft that finds the composer busy
waits in `HELD` until Compose finds it empty (`composeNew`). **Never
twice:** `rata_outbox_<uid>` in localStorage is written at Send (with the
files' base64 when they fit), marked `sending` just before `send_mail`
and removed after; on `pagehide`/`beforeunload` everything waiting goes at
once, and the next licensed start (`outboxResume`) sends `waiting` records
once and puts `sending` ones back in the composer ("Look in Sent before
sending it again"), never resending them. A `waiting` record holding a
Create file reference (`{created: id}`) is never sent at start either: it
is put back too ("It would attach X as it is now, so check it before
sending.", SEC-10), since the file may have changed since Send; put-backs
are said after the "Sending the message RATA closed on" toast. The file is
also read when the draft is saved to Drafts (`saveDraft`). Not seen in a packaged build:
closing the only window exits the app and probably drops a send in
flight, which the outbox then puts back. Harness: `open()` sets
`undoSend = 0` unless `{undo: true}`; `window.__RATA_SEND_SECOND`
shortens a second of the count.
**Quoted text folded (H12, v0.1.43).** In the Text view only (the
formatted view is untouched), `readingParts` splits a message into text,
signature and quote parts and `fillReading` draws them with DOM methods.
Folded behind a **Show quoted text** button (`aria-expanded`,
`aria-controls`; `QUOTE_OPEN` keeps a part open across redraws for the
session): a run of two or more `>` lines (a blank line inside counts); an
attribution (English "On … wrote:", including Gmail's two-line form,
German "Am … schrieb", French "Le … a écrit", Spanish "El … escribió",
Dutch "Op … schreef") with the `>` lines under it, or to the end when none
follow, so an interleaved reply's answers stay visible; Outlook's
"-----Original Message-----" or a `From:` with `Sent:` within three lines
(and the Von/Gesendet, De/Envoyé, De/Enviado, Van/Verzonden forms), to the
end. Never folded: a single `>` line (code, maths), Gmail's "Forwarded
message" block, or anything when only a signature or nothing would be
left. A line that is exactly `-- ` or `--` starts the signature, shown in
`--slate`, never hidden. A translation folds the same way.
**Accessibility (H10, v0.1.44).** *The list is a listbox:* `.vl-win` is
`role=listbox`, labelled "Messages" ("Messages in <mailbox>" in Side by
side), and each row an option with `aria-selected` (the open message),
`aria-checked` in Select mode, and `aria-posinset`/`aria-setsize` counted
against the whole pool, so they stay right when only a screenful is drawn.
Exactly one row has `tabindex=0`: the one focused last, else `KEY_AT`, else
the open message, else the first on screen; `vlist`/`vlDraw` put the focus
back on the same row after a redraw (`vlFocus`/`vlRefocus`), or on the
listbox while that row is scrolled out of the drawn window. On a focused
row, Arrows, Home and End move the focus (`rowStep`, through `keyMark` in
the main list so `KEY_AT` follows), Enter and Space do what a click does,
and j/k move the focus when it is in the list; the drawing half of
`keyMark` is `rowShow`. The ring is 2 px `--tint`, drawn inside
(`outline-offset:-2px`), because the scroller clips one outside. *Dialogs:*
every overlay, the composer, Assist and the welcome are `role=dialog
aria-modal` labelled by their heading (`#warn-ov` stays an alertdialog, and
bridge.js's licence box is one too, described by its reason, errors
`role=alert`); `topDialog()` (`DLG_ORDER`, licence box first) keeps Tab
inside the top one after every other handler, honouring
`defaultPrevented` so H3's Tab-to-choose still works; `dlgOpen`/`dlgClose`
give the focus back unless the closing code already put it somewhere (the
Undo toast, H9). The picture overlay and the shortcut sheet keep their own
trap and return. Delete account is an inline labelled `role=group`, not a
dialog: focus in, Escape out, Cancel returns to Delete account. *Toasts,
landmarks, words:* `#toast` is `role=status aria-live=polite aria-atomic`
and takes no focus; `<main>` replaces `div#main`, both navs are
"Sections" with `aria-current=page`, `.side-list` is region "Message list",
`#mail-detail` "Reading pane", `#person-detail` "Person", the bulk bar a
named group; unread, starred and the 📎 count carry words in the `.vh`
visually-hidden class, so none relies on colour; icon-only buttons are
named, and the rail's labels are visually hidden, not `display:none`, in the
901–1200 px icons-only band. *Motion and contrast:* under reduced motion
the 1 px press goes and every `scrollIntoView` is instant (`glide()`);
`--faint` becomes `--slate` wherever it sits on a tint or fill (axe measured
4.16:1 on `--tint-soft`), and More is no longer dimmed. *Zoom:* the
viewport no longer sets `maximum-scale=1.0, user-scalable=no`, which is
what a browser's pinch zoom obeys; the desktop app has no zoom keys, since
`tauri.conf.json` does not set `zoomHotkeysEnabled` (H10's note said "zoom
allowed"; that is the website's page, not the app's keys). *The
harness runs axe* (`@axe-core/playwright`, a rata-next dev dependency; it
needs a page from `browser.newContext()`) at the inbox, an open message
with a conversation, a picture and a folded quote, the composer with
suggestions open, Settings, and the link, disguised-program, shortcut and
licence dialogs, plus the inbox and a message in dark and the icons-only
rail, and fails on any serious or critical rule; no rule or selector is
excluded. Not seen in a packaged build (markup, CSS and page JS only, no
new inline `style=`).
**New mail as it arrives (v0.1.34).** `watch.rs` keeps one connection per
linked mailbox waiting on its inbox with IMAP IDLE (`rata_mail::watch` →
`Watch::wait(IDLE_FOR)`, nine minutes a round, then a fresh IDLE). It is an
accelerator only: the five-minute refresh carries on, so a server without
IDLE, a connection a laptop's sleep killed, or any failure here costs speed,
never mail. `idle_until` wakes only when the inbox grows past what it held
(`exists` follows EXPUNGE down and EXISTS up); a flag changing — RATA
marking a message read on its other connection — starts a fresh IDLE rather
than waking, or every read would cause a refresh. The watch reads nothing
and keeps no password. The supervisor (`watch::supervise`) re-checks every
minute: `Rata::watchable` is every linked mailbox not parked for its
password, and none without a licence, so a lapse or an unlink closes the
connection; `Rata::watch` runs the refresh's own checks (`usable`, then
`signed`) and parks a mailbox whose password is refused, so a wrong one is
never sent twice. A failed connection is retried after 30 s, doubling to 15 minutes;
a server without IDLE is not asked again until RATA restarts. On new mail
Rust evals `window.__rataMail({email})` (a JSON literal, as with links);
the page waits 1.2 s so a burst is one refresh, then runs `autoSync` — the
same quiet refresh as the timer, so fetching, showing and notifying stay in
one place — and a wake during a refresh (`MAIL_AGAIN`) gets one more after
it. Checked against a real Dovecot in CI
(`idle_wakes_on_an_append_but_not_on_a_flag_change`).
**Fewer sign-ins (v0.1.35).** Every refresh used to sign in to every
mailbox — 288 a day each, and providers throttle that. `refresh_mail`
takes `only` (addresses; none is all), `Rata::refresh` reads just those,
and the bridge stamps only the mailboxes it was asked about. A wake reads
only the mailbox that woke (`WOKE`); a mailbox's own **Sync now** reads that
mailbox. The timer (`autoTick`, every 30 s) reads each mailbox when it is
due, by `LAST_TRY`: every five minutes, or every half hour
(`AUTO_WATCHED`) while its connection is live — `watch::Watching`, the
set of mailboxes in IDLE right now, held by a `Live` guard that drops with
the connection (failure, lapse or abort alike) and read through the
`watching` command. Coming back to the window after a minute still reads
every mailbox (`lastFull`, not `lastSync`, so a one-mailbox wake does not
suppress it): a laptop's sleep kills connections without either side
noticing for minutes. At start RATA reads everything, as before.
**Signatures (v0.1.36).** One per mailbox, in Settings → Signatures (app
only — only the app sends), kept as `S.settings.sigs` keyed by address and
**stripped from what syncs to the online account** (`CLOUD_STRIP.settings`
has `sigs`; `applyCloud` merges, so the local ones survive a sync): a
name, phone and address are what a breach would leak. `sigOf(acctId)`
gives the block, "-- " line first (the standard mark mail apps use to fold
a signature away). `openCompose` puts it under an empty new message with
the cursor above; Reply, Reply all and Forward put it above the quote
unless Settings → "In replies and forwards too" is off
(`S.settings.sigReplies`); a draft being finished keeps its own text.
`CMP_SIG` is the block RATA put in: changing From swaps it for that
mailbox's (or removes it) only while it is still there unchanged, and a
message holding only the signature still counts as empty for Discard.
**The redesign (v0.1.37).** `DESIGN.md` is the design system, written after
auditing the old interface against Taste Skill's redesign audit and Vercel's
Web Interface Guidelines: one cobalt accent (`--tint`, `--tint-btn` for
filled buttons, which differs in dark mode so white text keeps 5.4:1), one
cool neutral family, Geist and Geist Mono (Fredoka stays for the wordmark
only), controls 8 px and containers 12 px, flat surfaces with hairline
borders, no gradients, glows or overshoot. The old token names (`--grad-*`,
`--sheen*`, `--glow*`, `--r-bubble`) still exist but resolve to flat values,
so a rule nobody touched cannot bring the old look back; `app.test.mjs`
checks the accent fill, 8 px controls, Geist, and that nothing on screen has
a gradient. Behaviour changed in three places only: a mail row's badges sit on
the sender's line (rows are three lines, so none is cut when the list sizes
every row from one it drew), the provider badge ("Imap") is gone from rows,
and "via mailbox" is text in the sender line rather than a dead button. The
account button shows initials from the start. `@keyframes pop` (a fade
and a 4 px rise, 160 ms, none under reduced motion) opens the account menu
and the link, warning, shortcut and welcome cards; it was missing until
I1 (v0.1.45), so none of them moved, and `app.test.mjs` now checks every
animation used is defined. The website was rewritten to
say only what the app does (it still sold texts, Slack, Discord, DLP and a
browser install; all but DLP have been gone from the app since 0.1.6, and
DLP is still in the app, not sold on the website: Privacy checks, compose
scanning (`runDLP`) and the sidebar's "DLP profile HIPAA", all local, no
network), with screenshots of the real interface in `public/shots` and no
em dashes in visible copy. Retake the screenshots after any visible change:
`./sync-ui.sh`, serve `desktop/rata-app/ui` (`python3 -m http.server 3185
--bind 127.0.0.1`), then `node desktop/rata-app/harness/shots.mjs
http://127.0.0.1:3185 rata-next/public/shots`. `vendor-fonts.mjs` now keeps a variable font once
instead of once per weight (460 KB → 167 KB).
**Gmail's archive (v0.1.38).** Where a server has no `\Archive` but
declares `\All`, `places()` takes that as Archive with `all_mail` set, and
every read of it goes through Gmail's own search, `UID SEARCH UID lo:hi
X-GM-RAW "-in:inbox -in:sent -in:drafts"` (`GMAIL_ARCHIVED`). Any server but
Gmail refuses X-GM-RAW, which leaves Archive out, as before. The catch that
shaped it: an archived Gmail message keeps the All Mail UID it arrived
with, so "newer than the newest held" never finds a message archived
today that arrived last week. So a refresh (`refresh_archived`) downloads
what is newer as usual, and also lists every archived UID within
`ARCHIVE_WINDOW` (2 000) of the top of All Mail (`Newest.archived`, per
mailbox `MailArchive`, `archives` on the wire). The page (`settleArchives`,
after the refresh's own mail is in) drops held archive mail at or above the
floor that is not listed (moved back to the inbox or deleted elsewhere; not
through `S.gone`), and fetches listed UIDs it does not hold, but only at or
above the oldest one it held before (below that is Load older mail's), 50
a mailbox a refresh, through `reread_mail`. Load older mail and gaps page
by the same search (`newest_archived`, widening the block searched until a
page fills). **Acting on it:** taking a message out of All Mail deletes it
everywhere in Gmail, so Move to inbox and Move to from Gmail's archive are
a `UID COPY` (adding the label) and never a MOVE; a message moved into a
folder is therefore still archived and comes back under Archive on the
next refresh, which is what Gmail shows too. Trash is still a MOVE to
Gmail's Trash, and Archive on archived mail does nothing. **Found on the
way:** async-imap's `uid_search` reads a refusal (NO or BAD) as "found
nothing", so a server refusing the Drafts listing would have had the page
drop every draft; `search()` reads the tagged status itself and returns
nothing on a refusal (tested). Not seen against real Gmail: covered by a
scripted Gmail server in the engine tests.
**OAuth in the engine (C1, v0.1.39).** `Account.credential` is
`Credential::Password` or `Credential::OAuth { user, access_token }`. A
token goes by `AUTHENTICATE XOAUTH2` (IMAP) or `AUTH XOAUTH2` (SMTP), never
on a command line and without SASL-IR; `xoauth2` refuses control
characters, spaces and empty parts, so a token cannot forge a field. A
refused or expired token is its own kind: `OAuth` on every result enum, the
app's problem kind `oauth`, never `auth`. A "come back later" answer, IMAP
`BAD`, or SMTP refusing the mechanism before the token is sent is `net`, and
an SMTP server that does not offer XOAUTH2 never sees the token. No token
appears in a `Debug` or an error (`credential::redact`, which also removes
its base64 wire form). `verify_with` verifies with any credential. Two
traps: async-imap 0.11 logs at `log::trace!` every command it sends,
`LOGIN user pass` and the XOAUTH2 line included, and every reply it reads
(`imap_stream.rs`); the app installs no logger, and any future logger must
keep `async_imap` below trace. And
`cargo doc` with `RUSTDOCFLAGS="-D warnings"` fails on three old
intra-doc links (`imap.rs` → private `own_folders`, `smtp.rs` → private
`starttls`, `resolve.rs` → the ambiguous `crate::discover`); CI does not
build docs.
**Sign in with Microsoft (C2/C3, v0.1.39).** Microsoft mailboxes
(Outlook.com, Hotmail, Live, Microsoft 365) sign in with OAuth 2.0 code +
PKCE (`oauth.rs`). The browser, never the webview, opens
`login.microsoftonline.com/common` (with `prompt=select_account`) through
`links::classify`. There is one sign-in at a time, and none within 3 s of
the last (`BEGIN_GAP`); it gives up after five minutes. A one-shot
`127.0.0.1:<any port>` listener takes the one request carrying this
attempt's `state` (32 random bytes, single-use; 404 to everything else, 2 s
to speak, `HEAD_WAIT`). A redirect error shows only its code, and only if
it looks like one. The refresh token lives in the mailbox's keychain entry,
marked `rata-oauth2:<n>:`. Pieces past the first 1 000 characters go under
**their own service** `org.mailrata.desktop.oauth-piece` (`<addr>#2…#8`),
because Windows holds 1 280 characters per entry. In the mailbox service, a
mailbox linked as `<addr>#2` would have been sent piece 2 as its password
(security review M1), so `link()` also refuses anything that is not an
address, and a password replacing a sign-in removes old pieces.
`mailboxes.json` records only `"auth": "oauth"`. Access tokens are kept in
memory and renewed within 2 minutes of expiry (`EARLY`). A rotation the
keychain half-writes puts the old token back (a kill between the two writes
is not covered). The token endpoint is called over reqwest with the
engine's own TLS config (`tls_client_config`), no redirects, 30 s. Every
operation goes through `usable` → `account` → `signed`. A refused token is
renewed and retried once. `invalid_grant` parks the mailbox as "Sign in to
Microsoft again". A network failure or a refused renewal backs the watcher
off and parks nothing. A password never reaches Microsoft: `verify_with`
returns `Verify::Microsoft(label)` before any socket opens when a password
would go to a Microsoft host. A token only goes to `is_microsoft` hosts.
`discover_mailbox` (no password; bridge `/api/link/discover`) needs a
licence and a plausible address, and tells the page when a company domain's
mail is at Microsoft 365, so the form offers **Sign in with Microsoft**
instead of a password box; `link_microsoft`, `cancel_microsoft` and
`microsoft_ready` sit behind `/api/link/microsoft`. The client id comes
from `option_env!("RATA_MS_CLIENT_ID")` (not a secret). A build without it,
and the website, keep C0's words: `MS_HELP`/`MS365_HELP` in the engine and
`MS_NOT_YET` in the page ("…cannot be added to RATA yet…"). Not seen
against a real Microsoft sign-in until the owner's registration (C4) and
B2's Outlook column. A Microsoft 365 domain behind a mail filter such as
Mimecast is found since L1 (below) by its autodiscover CNAME or SPF record;
still not handled: such a domain that publishes neither, or whose SPF names
two providers, and shared or delegated mailboxes.
**Behind a mail filter (L1).** When SRV and the MX rules name no provider
(the MX is a filter, unknown, or missing), `resolve.rs` reads, by DNS only,
a CNAME of `autodiscover.<domain>` to `autodiscover.outlook.com` (Microsoft
365, `Source::Autodiscover`) and the domain's one `v=spf1` record, read
strictly (RFC 7208; only passing includes; one `redirect=` hop, ignored with
`all`; macros skipped; more than one record, an unknown term or two
providers is no evidence), whose includes name Microsoft 365, Google
Workspace, Zoho by region (`zohomail.`/`zoho.`/`one.zoho.com`), Fastmail,
Hostinger, iCloud, Titan, Rackspace, Migadu or Namecheap Private Email
(`Source::Spf`); host, label and help come from that provider's MX rule, so
Microsoft found this way gets Sign in with Microsoft and `verify` keeps a
password away from it. At most three lookups. Order: SRV, MX rules,
evidence, conventional names, except that an unknown MX under the address's
own domain keeps the conventional names first (an on-premises Exchange that
sends through Microsoft 365 has that SPF). `zoho_second_chance` covers
`Source::Spf`. `discover_with<D: Lookup>` takes a scripted DNS in tests.
**Drafts saved to the server (F1, v0.1.41).** In the app the composer's
draft is APPENDed to the mailbox's Drafts folder with `\Seen \Draft` set
(`\Seen` since 0.1.43, BUG-M, so it never comes back unread) when the
composer closes with something new in it and every two minutes while it is
open and edited (`queueDraftSave`, one queue, `DRAFT_SAVING`;
`window.__RATA_DRAFT_EVERY` shortens the interval for tests), never per
keystroke; edits are counted from input events, so a programmatic fill
saves nothing. `compose::render_draft` is the send render plus `Bcc:`,
`X-RATA-Draft: <uuid>` (made once per draft by the page; `draft_id_ok`
takes only 16 to 64 characters of lowercase hex and `-`) and
`X-RATA-Draft-Rev: <n>`, with no dot-stuffing (APPEND stores the bytes as
sent) and To allowed empty; a sent message's render is unchanged. The
new UID comes from `APPENDUID` (RATA sends its own APPEND, since
async-imap's drops the tagged response code), else `UID SEARCH UNDELETED
HEADER X-RATA-Draft "<id>"`, the newest match that is not the copy
before and whose `X-RATA-Draft` header is exactly the id (`newest_carrying`;
HEADER search matches a substring, review P3-3, SEC-5). **The one permanent delete in RATA** (`imap::replace`): the
previous copy is removed only when its uid is not 0, it is not the copy
just saved, Drafts' UIDVALIDITY matches, and `UID FETCH (BODY.PEEK[HEADER.FIELDS
(X-RATA-Draft)])` finds exactly one `X-RATA-Draft` header whose value is
this draft's id; then `UID STORE +FLAGS.SILENT (\Deleted)` and `UID
EXPUNGE <uid>`, with UIDPLUS only. On a server offering `X-GM-EXT-1`
(Gmail), where that expunge would only take the Drafts label off and leave
the copy in All Mail, the proven copy is moved to the declared `\Trash`
with `UID MOVE` instead (review P3-2, SEC-5, v0.1.42); no Trash, or a
refused move, is `Failed` and the copy stays. Without UIDPLUS the copy stays flagged
(`Prior::LeftFlagged`), never a bare EXPUNGE, which would take whatever
anybody else had flagged. Anything else leaves it where it is
(`NotOurs`; `Gone` when it is no longer there, `Failed` when the server
would not flag it). `act` still never deletes permanently. A mailbox with no Drafts
folder keeps drafts local (`NoPlace`, remembered in `DRAFT_NOPLACE` and
not asked again this session); RATA never creates a folder.
`Rata::save_draft` is gated like a refresh (`usable`: licensed, linked,
not parked), since the page saves on a timer, and checks a draft like a
message being sent (`outgoing()`, shared with `send`: every address, 18
MB, 100 people, forwarded files fetched before the save) except that To
may be empty; a half-typed address refuses the save, and on close the
toast says "Not saved to Drafts: …" and the draft stays in the composer.
"Saved to Drafts" is said at most once per close, never for a timer save.
The page files its own copy under Drafts at once (`ownDraft`, which
`heldKnown` skips and `absorbMail` clears when a refresh brings it back;
`dropVanishedDrafts` spares one saved in the last ten minutes), and the
copy before leaves the list without going through `S.gone`. Send waits
for a save in flight and Discard asks first; both move RATA's copy to the
Trash. A draft from another device finished here goes to the Trash once
RATA's copy is saved, and a From change sends the old mailbox's copy
there too (both for the owner to confirm). `Message.draft_id` (Drafts
only) lets `continueDraft` save over RATA's own copy; `X-RATA-Draft-Rev`
is not read back, so a draft continued later starts again at 1. Tested on
a scripted server and real Dovecot (loopback). **Gmail, for B2:** the
move to the Trash (above, review P3-2) has been seen only on a scripted
Gmail server; on a real Gmail account, save a draft twice and check that
All Mail, and RATA's Archive, hold one copy (SMOKE.md X7).
**Unsubscribe (H7, v0.1.43).** `body::read` keeps `List-Unsubscribe` as
`Message.unsubscribe` (`body::Unsubscribe { https, mailto }`,
`serde(default)`): angle-bracketed entries, folded lines joined, a header
over 8 KiB ignored. It holds the first `http(s)://` entry with an authority
and no control or bidi-control character (at most 4 096), and the first
`mailto:` whose percent-decoded address `compose::Address` parses as one
address, rebuilt keeping only `subject` (200) and `body` (2 000), cleaned
and percent-encoded again; a stranger's `to=`, `cc=` and `bcc=` are
dropped, and every other scheme is ignored. The URL rule stays in one
place, the app's `links::classify`: `core::as_links` runs it on every path
messages reach the page (refresh and `answer`), sends the URL in classify's
form (a look-alike host as `xn--`), and drops what fails; `open_link`
checks again when Open is pressed. `List-Unsubscribe-Post` is never acted
on: RATA makes no request of its own to a list's server. The bridge passes
`unsub: {https?, mailto?}` when present. The reading pane shows
**Unsubscribe** for inbox mail only (`unsubOf`, `leaveList`; never Sent,
Spam, Drafts or Archive): a web address goes through the same "Open this
link in your browser?" dialog naming the host as any link (`askToOpen`),
and is preferred when a message has both; a mailto opens the composer from
the mailbox the list wrote to, with the header's subject and body and no
signature, and nothing is sent until Send. Mail stored before the field
shows nothing until it is re-read; opening in full does not refill it.
**Copy diagnostics and Help (H8, v0.1.43).** Settings → **Help &
diagnostics**. **Copy diagnostics** (app only) calls the `diagnostics`
command (`diagnostics.rs`, bridge `/api/diagnostics`) and puts one
plain-text block on the clipboard, always showing it read-only; where the
clipboard is refused the block is selected with "Copy it with Ctrl+C or
Cmd+C". It holds the version and build kind, OS and architecture, which
build keys are present (licence, updater, Microsoft client id), the
licence's plan and day or the reason's name (never the token or the
email), the last refresh, the store's schema and the version of the file
read at start (`store::SCHEMA`, `on_disk`), and per mailbox, numbered and
never by address: IMAP host:port, TLS and how it was found, the SMTP
host:port last used and its mode (or the candidates it will try), how it
signs in, whether and why it is parked, the last error, the last good
refresh, the special folders seen this session, and whether its live
connection is up. The notes are in memory only (`Rata::notes`), and gathering
them dials nothing. `refresh` and `send` fill the last error. Since J1
(#128) `Noted.failed` also keeps the latest failure of each other operation
(`diagnostics::Op`): an action (named, never the destination folder), older
mail and re-read, a message fetch (open in full, save or convert an
attachment, pictures), server search, draft save (no Drafts folder too),
folders (list or read), and the new-mail watch (connect, or connection lost
through `Rata::watch_ended`), each shown as `Last failed <op> (<what>)`. A
later success of the same operation (for actions, the same action) keeps
the line and adds "worked again <when>"; a new failure replaces it. Nothing
is kept for unlicensed, unknown, `needs-confirmation` or an already parked
mailbox, and the live line says when the server has no IDLE. **No names
(SEC-8).** A failure is cleaned as it is kept, so no name is held even in
memory: `kept`, `note_failed` and `note_error` take the names the operation
touched (`diagnostics::Name`: a named folder, a Move's destination, an
attachment's fetched name, a draft's or a sent message's subject and files),
and `without_names` replaces each with `[folder]`, `[file]` or `[subject]`
in every spelling a server could echo (raw, decoded and modified UTF-7,
IMAP-quoted, each level and `A / B`, `safe_file_name`'s form and stem, a cut
start of 6 or more characters), ignoring case; anything else in quotes
becomes `[name]`. It removes too much rather than too little. The refresh
"Last error" cannot name a special folder (a failing one is dropped), and
the send line leaves out the subject and attachment names.
`rata_mail::imap::utf7_imap` is public for this. Every free sentence
goes through `diagnostics::clean`: the mailbox's address becomes `Mailbox
N`, then `credential::said_within` (600) with that mailbox's secrets, read
from the keychain only when it has an error to show; any other address
becomes `[address]` and the home folder `~`, and a last pass turns any `@`
left into ` at `. How Sent, Spam, Drafts and Archive were found is reported
too: `places_in` records `FoundBy` (attribute, one of RATA's fixed names, or
Gmail's All Mail) as `Placed`, which a refresh that listed the folders
returns as `Newest.places` (`None` when the listing failed), and the block
names only RATA's own spelling, never the server's. Trash is not reported. **Open help** (`https://mailrata.org/help`) and **Report a bug** (the
*Beta bug* template) go through `open_link`; the website shows those two
only. **The help page (H2)** is `rata-next/public/help.html`, served at
`/help` (`proxy.js`): installing, linking each provider (with that
provider's own app-password page), what each linking error means, and how
to report a bug. It quotes the engine's and the app's error sentences
exactly, and `tests/help.test.mjs` checks every quote is still in the
source, so **rewording a customer-facing error means updating the help
page too**. It goes live only when mailrata.org is redeployed.
**Connected accounts and Share to Slack (K1, K5, K6; Pro).** Workstream K
in `docs/MVP-PLAN.md` says what each service can be and why. The page
(K1): Settings → Connected accounts, a source switch and cloud folder
browser in Files (Open in Bridge, Add to Files), **Save to ▾** on Files
documents, Bridge output and attachments, and a Share to Slack dialog
(target search, editable text prefilled from `bodyOf`, attachments ticked
by choice, nothing sent until Send), all gated by `can('connect')` (TIERS
`connect`, or the licence plan's own `connect` from Rust). It shows only
when `connections_status` answers, and only entries with `available !==
false`; the contract for the Rust side is the comment in `bridge.js`
(`/api/connections`, `/api/cloud/*`, `/api/slack/*`; refusals
`{service, kind, error}`). Every name, path and error is drawn with
textContent. `lib/plan.js` and the website say nothing of it until the
real connections ship (BUG-D). Settings' sentences are built from
`CONN.list` (`andList`) and never name a service answering `available:
false` (SEC-10). Add to Files refuses a disguised program from a cloud
folder, and ⤓ File asks (`askDisguised`) for one already in Files:
`nameDisguised` in the page mirrors `rata_mail::names::looks_disguised`,
so keep the two lists in step. Once Share to Slack's Send is pressed the
dialog stays (Cancel, ✕, Escape and the backdrop do nothing) until Slack
answers, and the result is always said (SEC-10). **K5 (`cloud.rs`)** is the only real service
so far: iCloud Drive and Creative Cloud Files as the folders their own apps
sync (macOS `~/Library/Mobile Documents/com~apple~CloudDocs`, Windows
`%USERPROFILE%\iCloud Drive` or `iCloudDrive`, `~/Creative Cloud Files…`),
since neither Apple nor Adobe has a public API for a person's files. Ids
are `/`-separated paths relative to the folder, checked name by name (no
`..`, absolute, hidden or Windows device names), every step with
`symlink_metadata`, and the canonical result must stay inside: a link is
never followed. Listings hide dot files and system files and show iCloud
placeholders as `offline`; reads cap at `READ_MAX`, saves at 100 MB through
`write_new` (marked as downloads, since a stranger's attachment may be the
file) and need `confirmed` for a disguised program. The store keeps
`connections` (service → path) and Delete account forgets them. **Share to Slack (K3, `slack.rs`)**
exists only in a build with `RATA_SLACK_CLIENT_ID` (a repository
variable, passed like `RATA_MS_CLIENT_ID`; Copy diagnostics says "Slack
sharing yes/no"); without it Slack answers `available: false` and every
command refuses with `NO_SLACK`. A user token over PKCE, no client secret:
the browser opens `slack.com/oauth/v2/authorize` with the seven user
scopes in `docs/CONNECTIONS-SETUP.md`, a one-shot listener on 127.0.0.1 at
the first free one of `PORTS` (28417 to 28419, which the owner registers
as `http://localhost:<port>`; Slack matches the port, so change both
together), and `oauth.v2.access` with the verifier; shared pieces with
`oauth.rs` (one sign-in at a time, the listener, the HTTPS client),
Microsoft unchanged. The token, refresh token and expiry live in the
keychain under `org.mailrata.desktop.slack` (`workspace`, pieces
`workspace#2…#8`, mark `rata-slack1:`); the store keeps only the team, the
user id, when and whether it is parked. A rotation the keychain refuses is
kept in memory for the session. Disconnect and Delete account empty the
keychain first under the list's lock and bump a Slack generation, so a
sign-in or rotation answering later writes nothing (SEC-7/SEC-8's
pattern); Delete account cancels a waiting sign-in and forgets Slack
before the licence; Disconnect then calls `auth.revoke`, best effort.
`slack_targets`: `conversations.list` (member channels, private, ims) and
`users.list` (no deleted, bots or Slackbot), 200 a page, 15 pages at most.
`slack_share`: `chat.postMessage` (no unfurls, `parse=none`), files by
`files.getUploadURLExternal`, the bytes to `https://*.slack.com` only,
then `files.completeUploadExternal`; at most 10 files, 25 MB, 40 000
characters, no disguised program, text with `& < >` escaped and control,
bidi and invisible characters removed. A revoked token parks Slack
("Connect Slack again"); `token_expired` renews once; a rate limit of 10 s
or less is waited out once. Nothing reads Slack. Not yet seen against a
real Slack app (needs the owner's registration). **Create file (K6, SEC-9)**: the page sends
only `{format, name, where}` (bridge.js drops anything else, and Rust has
no parameter for `data`), and `created.rs` writes RATA's own blank docx,
xlsx or pptx (`Format::blank`, compiled in from `src-tauri/templates/`,
made by `make.py`, deterministic, no author or dates; see its README) or an
empty md, txt or csv into Documents/RATA or a connected folder with
`write_unmarked` (never marked: it is the customer's own new file), records
it in the store's `created` list (500 at most), and opens it with the
computer's app (`open::that_detached`). A page that could hand over the
bytes could have a document with an outside template opened (review F1);
`check_body` is now test-only, the guard on the templates. Making, opening
and reading need a licence whose plan has `connect` (`may_create`; kinds
`unlicensed`, `plan`); listing and forgetting do not, so a lapsed licence
can still tidy up. **The one exception to "RATA never opens a saved
file"**: a file RATA made through `create_file`, named by its random id,
still at its canonical recorded path, with its format's extension, not a
link, and not carrying a download mark (`mark::carries_mark`: ZoneId 3 or
4, or the quarantine download flag; an unreadable mark counts as one), all
re-checked on every open and every read (`created_read`, for Send with
RATA, at `ATTACH_MAX`). No save (`write_new`, `write_unmarked`) takes a
name a `created` record points at, in any case, so a stranger's file
cannot land where RATA made one (F2). Delete account forgets the list (and
the licence, so old ids answer `unlicensed`); the files stay. **The page
(K6):** **Create file** in Files (a dialog: kind, name, where; Documents,
and iCloud Drive or Creative Cloud Files while connected), **Created by
you** (Open, Send with RATA, Share to Slack, Forget; a file no longer
there offers only Forget), shown only when `created_list` answers and
gated by `canConnect()`. Send with RATA attaches by reference (`{created:
id}` in `CMP_FILES`), read through `/api/created/read` in `deliver()` at
send time, never to fill the composer; the reference survives Undo send
and the outbox (the record keeps the id, no bytes), and a failed read is
"Not sent: …" with the draft kept. **Start in your browser** (`LAUNCH`,
`BRAND`, `renderLaunch`, `#launch-sec`, localStorage `rata_launch_open`):
0.1.5's launcher back inside Files, each tile a link to the service's own
page (Google, Microsoft on `*.cloud.microsoft`, Adobe, Apple's iCloud
pages, Slack, and DocuSign, Acrobat Sign and RabbitSign), through
`open_link` in the app and a new tab on the website; addresses checked
2026-10-05, sources in a comment above `LAUNCH`. Slack and RabbitSign show
a letter, not a mark. `#docs-scroll` holds Created by you, the launcher
and `#doc-grid`. SheetJS's .xlsx fails `check_body` (its content types
always declare a macro-enabled `.bin` default), so never hand its output
to it. The demo (`desktop/rata-app/demo`) has sample accounts for
every service. Found on the way: `needPlan` no longer has an em dash, and
`.att-card .sz` uses `--slate` (axe, on `--fill-2`).
**Real providers (BUG-M, v0.1.43).** Linking: a table or known-MX
provider that cannot be reached (no DNS, a blocked port, too many
connections) is named with the reason and "Check your connection and try
again in a few minutes." (`known_host_failed`); the host box (`NeedsHost`)
is only for a domain discovery could not settle. Before 0.1.43 a Gmail
customer offline was asked for a server address. Zoho custom domains sign
in at their own region, read from the MX (`ZOHO_REGIONS`: `imap.zoho.eu`,
`.in`, `.com.au`, `.jp`, `zohocloud.ca`, `.sa`, `.com.cn`, `.com`), SMTP
and the help text follow. A custom domain whose `imap.zoho.<region>`, found
by MX, refuses the password is tried once at `imappro.zoho.<region>`
(Zoho's host for organisation accounts; `discover::zoho_pro`,
`zoho_second_chance`; I2, v0.1.45), never after a network error, for a
personal zoho.com address, a token or another provider; one linked there
sends through `smtppro.<region>`, and a refusal there reports the region
host's words. `verify_with` dials through `verify_found` and a private
`Dial` trait so tests can script it. Not seen against real Zoho. Sending: `smtp_ports`
puts 587 first for `smtp-mail.outlook.com`, `smtp.office365.com` and
`smtp.mail.me.com`, the only table hosts that document 587 alone; every
other host keeps 465 first (`SMTP_PORTS`). `smtp::routes` tries each host's
own ports, the host and port that last worked for this mailbox first
(`WORKED`, in memory, by lowercased address). Before 0.1.43 a Microsoft or
iCloud send waited out 465 on every address first. `Rata::send` goes
through `usable` before anything is dialled, so a mailbox parked for its
password sends nothing.
**Security review (2026-09).** `docs/SECURITY-REVIEW-2026-09.md` holds the
findings, each with a severity; no High. SEC-1 fixed the `cid:`
amplification, the Stripe checkout conflict, `past_due`, the AI fence and
the site's headers; SEC-2 the AI cap; SEC-3 Mark of the Web (#71), with
UI-1's question for disguised programs (#75); SEC-4 (#77, v0.1.41) rows 6
(navigation, *Links*), 7 (`background`, *HTML mail*), 11 (unpaid
checkouts, *Licensing*), 14 (the store, below), the SMTP half of 8
(*Bodies are decoded*) and part of 15 (`RENEW_GRACE_DAYS`); SEC-5 (#84,
v0.1.42) row 5 (the loopback gate, below) and the IMAP half of 8
(*Bodies are decoded*). Part 3 of the review (#82, 2026-09-29) covers the
code added since part 1 (drafts, marks, navigation, the store, checkout,
renewal, the AI cap, deletion): no High, two Medium, five Low. SEC-6
(#83) fixed P3-1 (invisible characters, *Disguised programs*), P3-5 and
P3-6 (*Licensing*), part of P3-4 (the lists), and the checkout-plan bug that
part 3 found on the way (*Licensing*); SEC-5 fixed P3-2 and P3-3 (*Drafts
saved to the server*) and P3-7 (row 8). The store: on
Unix `store.rs` writes `mailboxes.json` 0600 (a fresh temporary file, then
the rename), tightens a file an older build left wider when it opens it,
and makes the app folder 0700 when RATA creates it (an existing one is
left alone); Windows keeps the profile's ACL. Still open: row 15's
in-window renewals (a business decision), the rest of P3-4's suggested
lists (the owner's call), and one Medium waiting on the owner: email
confirmation before paid launch (LAUNCH.md §7, D8). The `loopback-tests`
gate is a `compile_error!` on the feature without `debug_assertions`, and
`desktop/rata-mail/build.rs` also fails any build with the feature when
`PROFILE` is `release`, whatever its assertions, with the same words (CI
looks for them), so `CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS=true` no
longer gets it into a release build. Never enable the feature in
rata-app.
Windows and macOS signing are wired and wait for the owner's
certificates.

**Never tested with a customer's mailbox.** No customer's mailbox has been
opened yet; the engine runs against real Dovecot (IMAP) and GreenMail (SMTP,
and IMAPS on 3993 for a server that declares no special-use folders) on
every PR (`tests/loopback.rs`), but not against Gmail, Outlook or any hosted
provider. Run them locally with `sudo
desktop/rata-mail/tests/loopback/servers.sh start /tmp/rata-lb`, then
`SSL_CERT_FILE=/tmp/rata-lb/ca.crt cargo test --features loopback-tests
--test loopback`. When a real connection fails, the three files to read are
`resolve.rs` (discovery), `imap.rs` (handshake and auth classification) and
`guard.rs` (host refusal). Errors are typed, so the message names the hosts
tried and what each said.

## Working style that earned its place here

- Verify before asserting. Several wrong claims last session came from reasoning
  about artifacts instead of downloading and checking them.
- When CI fails, read the log before theorising. Both release failures were one
  line in the log and neither matched the first guess.
- Prefer one validated push to three speculative ones.
