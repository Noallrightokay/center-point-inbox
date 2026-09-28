# RATA: the plan to MVP

Written 2026-09-28 by the lead engineer's session, for the project manager
agent that assigns work and the agents that do it. Every task below has a
stable anchor (`#a1`, `#c2`…) so a prompt can link straight to it:
`docs/MVP-PLAN.md#c2`.

**How to read this.** Part 1 says what "done" means. Part 2 is the state of
play, with how each fact was checked. Part 3 lists the integrations. Part 4
is the rules every agent follows. Part 5 is the task cards. Part 6 is the
order to run them in. Part 7 is the prompt for the project manager, and
Part 8 is the template every agent hands back.

A **[owner]** tag means only the project owner can do it: it needs an
account, a payment, a secret or a real mailbox. No agent may do an [owner]
step, ask for its secret, or accept one pasted into chat.

---

## Part 1. What MVP means

The MVP is done when **one real paying customer can go end to end without
help**, and each step has been seen working on a real system (not a mock).
The evidence goes in `docs/MVP-EVIDENCE.md`.

| # | The customer can… | Proven by |
|---|---|---|
| M1 | open mailrata.org and see an honest page (it claims nothing the app cannot do) | [D5](#d5), [A4](#a4) |
| M2 | create an account and pay (Stripe live mode) | [D3](#d3), [D6](#d6) |
| M3 | see their licence key on `/account` straight after paying | [D5](#d5) |
| M4 | download and install on Windows, macOS or Linux without a dead end | [E2](#e2), [E3](#e3), [B3](#b3) |
| M5 | paste the key and have the app accept it | [D1](#d1), [B2](#b2) |
| M6 | link a real Gmail, iCloud, Yahoo, Fastmail and custom-domain mailbox, and Outlook if [C0](#c0)–[C4](#c4) ship | [B2](#b2) |
| M7 | read, send, reply, forward with attachments, archive, delete and move, with the change visible in webmail | [B2](#b2) |
| M8 | use AI summary or translation on Pro | [D5](#d5), [B2](#b2) |
| M9 | keep working offline, and have the licence renew itself | [D5](#d5), [B2](#b2) |
| M10 | receive the next version through the in-app updater | [E1](#e1), [E4](#e4) |
| M11 | never have a mail password or mail leave their machine (except text they send to the AI relay) | [F4](#f4), BETA.md §4 checks in [B2](#b2) |

---

## Part 2. State of play (checked 2026-09-28)

### Done, and how it was checked

- **The desktop app** has been built through v0.1.38. The feature list is
  `CLAUDE.md` → Status: licence, adding a mailbox, sync, IDLE, folders,
  Sent/Archive/Spam/Drafts, Gmail's archive, compose with Cc/Bcc and
  attachments, reply and forward, HTML mail in a locked-down frame, the
  Format Bridge, notifications, signatures, the updater code and the
  redesign.
- **Releases** v0.1.0 to v0.1.37 are published on GitHub. v0.1.2 onwards are
  pre-releases. v0.1.37 was downloaded and checked: 5 assets, the licence
  public key embedded, no font CDN, and it launches as "RATA 0.1.37 beta".
- **v0.1.38** (Gmail's archive) is [PR #52](https://github.com/Noallrightokay/center-point-inbox/pull/52).
  CI was green on `aebadb1`, and it merged as `8511dad` ([A1](#a1)).
- **Tests.** Engine, shell and website suites run in CI (`desktop-ci.yml`,
  `rata-next-ci.yml`). The website suite passed today. The **desktop UI
  harness** (60 checks that drive the real interface against a fake
  backend) lived only in one session's scratch space. It is now in
  `desktop/rata-app/harness/` and passed from there today. It is **not in
  CI** yet ([A3](#a3)).
- **The server side** is in code: licence issue (`/api/licence`), renewal
  (`/api/licence/renew`), the AI relay (`/api/ai`), the Stripe webhook,
  health (`/api/health`), account deletion, CORS for the app's origins.

### Not done

| Gap | Evidence | Task |
|---|---|---|
| **The live website is an old deploy.** `/api/licence/renew`, `/api/ai`, `/api/health` and the new landing page's screenshots all return 404. | Fetched from mailrata.org during this session | [D4](#d4) |
| So renewal, AI and the new landing page have **never worked for anyone**. | same | [D4](#d4), [D5](#d5) |
| **The update feed returns 404.** No updater key in GitHub secrets. | Fetched the `updater` release's `latest.json` | [E1](#e1) |
| **No real mailbox has ever been opened by this code.** | CLAUDE.md, BETA.md; no tester reports (0 open issues) | [B2](#b2) |
| **Outlook, Hotmail and Microsoft 365 cannot work.** Microsoft turned off password (Basic) sign-in to Outlook.com over IMAP in September 2024. It requires OAuth 2.0 now, and RATA only does passwords. Exchange Online IMAP Basic auth is gone too. | [Microsoft Support: Outlook.com and Basic authentication](https://support.microsoft.com/en-us/office/outlook-and-other-apps-are-unable-to-connect-to-outlook-com-when-using-basic-authentication-f4202ebf-89c6-4a8a-bec3-3d60cf7deaef), [Deprecation of Basic auth in Exchange Online](https://learn.microsoft.com/en-us/exchange/clients-and-mobile-in-exchange-online/deprecation-of-basic-authentication-exchange-online); `grep -i oauth` finds nothing in the engine | [C1](#c1)–[C4](#c4). **Done in code:** C1 ([#59](https://github.com/Noallrightokay/center-point-inbox/pull/59)), C2+C3 ([#67](https://github.com/Noallrightokay/center-point-inbox/pull/67)), shipping in 0.1.39; live only after the owner's [C4](#c4) |
| The website, the app and BETA.md all **tell people Outlook works**. | `index.html:226`, `app.html:886,2301,3069`, `BETA.md:49` | [C0](#c0): **done** ([#57](https://github.com/Noallrightokay/center-point-inbox/pull/57)) |
| **Installers are unsigned.** macOS signing is wired in `release.yml` but has no certificates. Windows signing is not wired at all. | `release.yml` lines 240–286 | [E2](#e2), [E3](#e3): Windows wiring **done** ([#55](https://github.com/Noallrightokay/center-point-inbox/pull/55)); both wait for the owner's certificates |
| The **UI harness is not in CI**. | workflows list | [A3](#a3): **done** ([#60](https://github.com/Noallrightokay/center-point-inbox/pull/60)) |
| **Nothing is tested against a real IMAP server**, not even a local one: the outbound guard refuses private addresses. | `guard.rs` | [B1](#b1): **done** ([#58](https://github.com/Noallrightokay/center-point-inbox/pull/58)); its findings fixed in BUG-1..3 ([#66](https://github.com/Noallrightokay/center-point-inbox/pull/66)) |
| RATA's **own drafts are not saved to the server**. | CLAUDE.md | [F1](#f1), waiting on an owner decision |
| **The launch docs are wrong in places** (see [A4](#a4)). | read today | [A4](#a4): **done** ([#56](https://github.com/Noallrightokay/center-point-inbox/pull/56)), with DOC-1 ([#63](https://github.com/Noallrightokay/center-point-inbox/pull/63)) and WEB-1 ([#65](https://github.com/Noallrightokay/center-point-inbox/pull/65)) |
| **No database backups and no uptime monitor.** | LAUNCH.md §8 | [D7](#d7): scripts **done** ([#64](https://github.com/Noallrightokay/center-point-inbox/pull/64)); owner steps pending |
| Signups are auto-confirmed (anyone can claim any address). | LAUNCH.md §7 | [D8](#d8), after launch |
| **Stale [PR #1](https://github.com/Noallrightokay/center-point-inbox/pull/1)** from the deleted architecture is still open. | PR list | [A2](#a2): **done** (closed) |

### Things that are true but easy to get wrong

- The licence **public** key is already a GitHub secret: every release
  since v0.1.3 carries it (checked in v0.1.37). The website's
  `LICENCE_PRIVATE_KEY` must be **the other half of that same pair**, the
  one on the VPS at `/etc/rata/licence.key`. A freshly generated key would
  make every licence the site issues fail in every app already shipped.
- An agent session **cannot** push tags, create releases, dispatch
  workflows or set secrets (all 403). A release happens by merging a
  version bump (see CLAUDE.md → Releases).
- Microsoft 365 **SMTP** Basic auth still works until at least the end of
  December 2026. IMAP Basic auth is already off, and without IMAP there is
  no mailbox.

---

## Part 3. Integrations ("plug-ins"): what connects to what

Every outside service the MVP depends on, who wires it, and how to tell it
is working. "Plug-in done" means the last column passes.

| Integration | What for | Where it is set | Who | Status | Working when… |
|---|---|---|---|---|---|
| **Supabase** (db.mailrata.org) | accounts, `subscriptions`, `ai_usage` | hPanel: `SUPABASE_URL`, `SUPABASE_SERVICE_ROLE_KEY`, `SUPABASE_ANON_KEY` | [owner] | up (LAUNCH.md); `database.sql` §5 not run | `/api/health` with `HEALTH_TOKEN` says database ok; the `ai_usage` table exists |
| **Licence signing** | issuing and renewing licences | VPS `/etc/rata/licence.key`; hPanel `LICENCE_PRIVATE_KEY` and `LICENCE_PUBLIC_KEY`; GitHub secret `RATA_LICENCE_PUBLIC_KEY` | [owner] | GitHub side done; website side unknown | a key from `/account` is accepted by a released app ([D5](#d5)) |
| **Stripe** | payment, then the webhook writes `subscriptions` | Stripe dashboard; hPanel `STRIPE_WEBHOOK_SECRET`, `STRIPE_PRICE_BASE/PRO`, `STRIPE_BASE/PRO/PORTAL` | [owner] | not set up | test payment → webhook 200 → row → licence on `/account` |
| **Hostinger Node.js** | runs rata-next | hPanel | [owner] deploys; an agent may prepare | old deploy live | `/api/licence/renew` answers (not 404) |
| **Anthropic** | the AI relay | hPanel `ANTHROPIC_API_KEY` (+ spend limit in the console) | [owner] | not set | Summarize in the app returns an AI summary |
| **Tauri updater** | self-update | GitHub secrets `TAURI_SIGNING_PRIVATE_KEY`, `_PASSWORD`, `RATA_UPDATER_PUBKEY` | [owner] | not set; feed 404 | the release log says "Signing this build's installers for updates"; `latest.json` exists |
| **Apple signing + notarisation** | macOS opens without a warning | GitHub secrets `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` | [owner] ($99/yr Apple Developer) | wired, no secrets | `spctl -a -vv RATA.app` says accepted, notarized |
| **Windows signing** | no SmartScreen block | GitHub secrets: Azure Artifact Signing `AZURE_TENANT_ID`, `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET`, `AZURE_ENDPOINT`, `AZURE_CODE_SIGNING_NAME`, `AZURE_CERT_PROFILE_NAME` (the last three may be variables), **or** `WINDOWS_CERTIFICATE` + `WINDOWS_CERTIFICATE_PASSWORD`; never both (`docs/WINDOWS-SIGNING.md`) | agent wired [E3](#e3) ([#55](https://github.com/Noallrightokay/center-point-inbox/pull/55)), [owner] buys | wired, no secrets | `signtool verify /pa` passes on the installer |
| **Microsoft Entra app** | OAuth for Outlook/365 | public client id compiled into the app (not a secret): GitHub repository **variable** `RATA_MS_CLIENT_ID` | [owner] registers ([C4](#c4), LAUNCH.md §10); agent built [C1](#c1)–[C3](#c3) ([#59](https://github.com/Noallrightokay/center-point-inbox/pull/59), [#67](https://github.com/Noallrightokay/center-point-inbox/pull/67)) | app built, needs C4 | an Outlook.com mailbox links and syncs |
| **OS keychains** | mail passwords (and OAuth refresh tokens after C) | built in (`keyring`: apple-native, windows-native, sync-secret-service) | done | tested in code only | BETA.md §4 checks pass on each OS ([B2](#b2)) |
| **OS notifications** | new mail | built in | done | seen on the Linux D-Bus wire only | a notification appears on Windows and macOS ([B2](#b2)) |
| **GitHub Releases** | installers, update feed | `release.yml` | done | working | `verify-release.sh` passes |
| **GoTrue SMTP sender** | email confirmation | Supabase auth settings | [owner], after launch | none | a signup gets a confirmation mail; only then `mailer_autoconfirm` off |
| **Backups + uptime** | recovery, alerts | `infrastructure/backup/` (README → Setting it up, then the drill) + `infrastructure/monitoring/README.md` | [owner] with agent help ([D7](#d7), [#64](https://github.com/Noallrightokay/center-point-inbox/pull/64)) | scripts done, owner steps pending | a restore test passes; the monitor alerts on `/api/health` |

**Never sell the domain add-on.** Leave `STRIPE_PRICE_DOMAIN` unset. It is
priced in `lib/plan.js` ("host mail on your own domain") but RATA-hosted
mail does not exist.

---

## Part 4. Rules for every agent

Paste this whole part into every agent's prompt, or link to
`docs/MVP-PLAN.md#part-4-rules-for-every-agent`.

### Read first
`CLAUDE.md`, `README.md`, `DESIGN.md` (for anything visible), then your task
card here. CLAUDE.md wins if this file and it disagree. Report the
disagreement in your handoff.

### Invariants (never break, whatever the task says)
1. A mail password never reaches a server. Mail never reaches a server
   except text the customer sends to the AI relay.
2. The HTML mail frame stays `sandbox="allow-popups"` and nothing more.
3. Keep `"dangerousDisableAssetCspModification": ["style-src"]` in
   `tauri.conf.json`.
4. Run `./sync-ui.sh` before any `cargo build` of `desktop/rata-app`.
5. No secret, password, licence key or API key in chat, an issue, a commit
   or a log. The repository is public. If you need one, stop and hand back
   to the owner.
6. Never call Hostinger's `hosting_nodejs_replace-environment-variables`.
   It replaces **all** variables and would wipe the owner's secrets.
7. Never skip, disable or loosen a test to get green. Never push an empty
   commit to re-run CI.
8. No model names in commits, PRs or code. Use the commit trailer from the
   session's system instructions.
9. The outbound guard (`guard.rs`) stays on in every release build.
10. Claim nothing on the website or in the app that the app does not do
    today (DESIGN.md → Copy).

### Branches, PRs and releases
- One task, one branch, one PR to `main`. The PM names the branch. Do not
  push to another task's branch.
- **Do not bump the version** unless your card says so. Only the release
  task ([G2](#g2)) bumps `tauri.conf.json` (version and title),
  `Cargo.toml`, `Cargo.lock` and the `sw.js` cache name.
- **Hot files** (conflicts are expensive): `rata-next/public/app.html`,
  `desktop/rata-app/ui-src/bridge.js`, `desktop/rata-app/src-tauri/src/core.rs`,
  `desktop/rata-mail/src/imap.rs`, and the three docs `CLAUDE.md`,
  `README.md`, `BETA.md`. The PM runs at most one open task per hot file.
  Put any CLAUDE.md, README or BETA note in your handoff's "Docs note" and
  the release task folds it in. The exception is a task whose card names
  those docs.
- After your PR merges, `git fetch origin main` before starting anything
  else.

### How to prove your work
Run everything that applies, and paste the tail of each into the handoff:

```bash
# engine
cd desktop/rata-mail && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test --all-targets
# shell (needs libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev patchelf)
cd desktop/rata-app && ./sync-ui.sh && cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
# website
cd rata-next && npm ci && npm test && npm run build
# the interface, driven (after sync-ui.sh)
(cd desktop/rata-app/ui && python3 -m http.server 3181 --bind 127.0.0.1 &) ; node desktop/rata-app/harness/ui-harness.mjs http://127.0.0.1:3181
# a packaged build, for anything visible (tauri dev differs in the ways that ship bugs)
cd desktop/rata-app/src-tauri && cargo build --release --features tauri/custom-protocol   # then launch under Xvfb
```

"It should work" is not evidence. If you could not run something, say so
and say why.

---

## Part 5. Task cards

Card format: **Who** (agent or [owner]) · **Needs** (tasks that must be done
first) · **Touches** · **Do** · **Done when** · **Hand back**.

### Workstream A: repository and pipeline

#### A1
**Merge v0.1.38 and verify the release.** Who: the originating session, or
any agent once the owner says go · Needs: owner's go-ahead · Touches:
nothing.
Do: merge [PR #52](https://github.com/Noallrightokay/center-point-inbox/pull/52)
with the head SHA `aebadb1ebaa222a2078d2b7da7ead690e6ce4f53` pinned. Wait for
`release.yml`. Then run
`PUB_PEM=<licence public key file> desktop/rata-app/harness/verify-release.sh v0.1.38`.
Done when: 5 assets; `key-pem=1 key-raw=1 id≥1 title≥1 fonts=0` for both
binaries; the window is named "RATA 0.1.38 beta".
Hand back: the verify output.

#### A2
**Close stale PR #1.** Who: agent, after the [owner] agrees · Needs: — ·
Touches: GitHub only.
Do: close [PR #1](https://github.com/Noallrightokay/center-point-inbox/pull/1)
with one comment: it belongs to the deleted .NET/Kubernetes architecture
(`064674b`). Do not delete its branch.
Done when: the PR is closed.

#### A3
**Run the UI harness in CI.** Who: agent · Needs: this plan merged ·
Touches: `.github/workflows/desktop-ci.yml`, `desktop/rata-app/harness/`.
Do: add a job to `desktop-ci.yml`. It runs after `sync-ui.sh`, sets up
Node 22, runs `npm ci` in `rata-next` and
`npx playwright install --with-deps chromium`, serves `desktop/rata-app/ui`
on 127.0.0.1, and runs `ui-harness.mjs`. The job fails if the harness exits
non-zero. Add `desktop/rata-app/harness/**` to the workflow's path filters.
Then, in its own commit on the same branch, break one assertion on purpose,
show the job goes red, and revert.
Done when: the job is green on the PR, and the deliberate red run is linked
in the handoff.
Docs note: README's test section gains the harness.

#### A4
**Make the launch docs true.** Who: agent · Needs: — · Touches:
`rata-next/LAUNCH.md`, `infrastructure/backup/README.md`, `BETA.md`
(named, so this card may edit it), `rata-next/lib/plan.js` (one string).
Fix these, each checked against code:
1. LAUNCH.md §2 is titled "The encryption key" and says to make
   `LICENCE_PRIVATE_KEY` with `openssl rand -base64 32`. That is wrong. It is
   an Ed25519 PEM, and it must be **the existing key** on the VPS
   (`/etc/rata/licence.key`), the pair of the public key already in shipped
   apps. Point to LICENSING.md. Also check that LICENSING.md line 28's
   `require()` of an ES module actually runs on Node 22. Fix it if not.
2. LAUNCH.md §3.3 redirects to `/app?checkout=success`. STRIPE-SETUP.md (and
   the reasoning there) says `/account?checkout=success`. Make LAUNCH match.
3. LAUNCH.md §6 smoke-tests linking mailboxes on the website, and the Base
   mailbox limit there. The website links no mailboxes any more. Rewrite §6
   as the [D5](#d5) list.
4. LAUNCH.md's opening ("364 checks", "multi-mailbox linking, credential
   encryption") describes an older site. Replace it with today's facts.
5. `infrastructure/backup/README.md` is written around `TOKEN_ENC_KEY`,
   which no longer exists. Rewrite it for what is in the database now
   (accounts, subscriptions, ai_usage) and the licence key.
6. `lib/plan.js:194` says "one Center Point inbox" (the old brand). Make it
   "RATA". Run `npm test`.
7. BETA.md: the "Check your version" example says 0.1.6; make it generic.
8. Add to LAUNCH.md: "Do not create the domain add-on price" (Part 3).
Done when: every item is fixed; `npm test` passes; the PR body lists each
item with the line that proves it.

### Workstream B: proving it works on real mail

#### B1
**Integration tests against a real IMAP/SMTP server in CI.** Who: agent
(Rust) · Needs: — · Touches: `desktop/rata-mail/` (a new test module and
feature), `desktop-ci.yml`.
Why: every test so far uses a scripted server. The first real server that
behaves differently will be a customer's.
Do:
- Add a cargo feature `loopback-tests`, off by default. With it, and only
  in `cfg(test)`, `guard.rs` also accepts `127.0.0.1`, and TLS accepts a
  test CA passed by the test. It must be a **compile error** to build with
  that feature without `debug_assertions`. Add a CI step that tries it and
  expects failure.
- In CI, run Dovecot and an SMTP sink (for example GreenMail, which does
  both, in a service container) with a self-signed cert.
- Cover: link (discovery by explicit host), the first sync of
  INBOX/Sent/Drafts/Archive/Junk, an incremental refresh, older mail,
  read/star/delete/archive/move, send with an attachment and Cc/Bcc (check
  the Bcc is absent from the headers the sink received), IDLE wake on an
  APPEND, and UIDVALIDITY change handling.
Done when: the new job is green, and each covered path is listed with its
test name. Any behaviour that differed from the scripted server is written
up as a bug card for the PM.
Security review: the feature gate goes through [F4](#f4) before merge.

#### B2
**Live provider smoke test.** Who: **[owner]** runs it; an agent writes the
checklist first · Needs: [A1](#a1), [D1](#d1)–[D5](#d5) (for renewal and
AI; the rest can run with a key minted on the VPS) · Touches:
`docs/SMOKE.md` (the agent), `docs/MVP-EVIDENCE.md` (results).
Agent part: write `docs/SMOKE.md`. It is a checklist the owner runs on their
own machines, one column per provider: Gmail, iCloud, Yahoo, Fastmail, a
Hostinger custom-domain mailbox, and Outlook.com (expected to fail until C
ships, which proves the finding). Rows: link; the provider it named; first
sync counts per folder; open an HTML message; save an attachment; send to
self with an attachment; reply; forward; archive, then check webmail;
delete, then check webmail's Trash; move to a folder; mark read, then check
webmail; new mail arriving within a minute (IDLE); notification;
Summarize (Pro); BETA.md §4 keychain and `mailboxes.json` checks; Remove
deletes the keychain entry; relaunch keeps mail; offline start works.
Every failure row records **the exact error text** and version, and never a
password, a licence key or message content.
Owner part: run it on at least Windows and macOS (Linux optional) and fill
in `MVP-EVIDENCE.md`. File each failure as a *Beta bug* issue.
Done when: every row has a result. Each failure becomes a bug card: the PM
assigns it with the error text, and the fixing agent writes a failing test
that reproduces it first.

#### B3
**Packaged-build smoke on all three OSes in CI.** Who: agent · Needs: — ·
Touches: `release.yml` (a post-build step) or a new workflow.
Do: after each platform's installer is built, install it on the runner and
launch it headless (Windows: `windows-latest`, silent NSIS install; macOS:
mount the dmg and open the app). Check that the process stays up for 15 s
and that the window title is "RATA <version> beta".
Done when: all four builds run the check. Where a runner cannot show a
window, say so and assert what can be asserted (the process lives, and the
log is clean).

### Workstream C: Microsoft mailboxes (OAuth 2.0)

Microsoft mailboxes are a large share of the market, and today RATA cannot
open one. Decision for the owner: **ship MVP with Microsoft marked "not yet"
([C0](#c0) only), or hold MVP for C1–C4.** Recommendation: do C0
immediately, start C1–C3 in parallel with everything else, and ship when
they are done. They need no server: a desktop app uses OAuth with PKCE, and
the refresh token lives in the keychain like a password. (Google's OAuth
path for Gmail is different: the full-mail scope is restricted and needs a
paid security assessment. Gmail stays on app passwords for the MVP.)

#### C0
**Stop claiming Outlook works, today.** Who: agent · Needs: — · Touches:
`rata-next/public/index.html`, `app.html` (three strings; hot file, take
the lock), `BETA.md`, `desktop/rata-mail/src/discover.rs` (help text).
Do: remove Outlook from "works with" lists. In the app, when discovery
lands on `outlook.office365.com`, say plainly before asking for a password
that Microsoft mailboxes are not supported yet and why. Remove BETA.md's
Outlook app-password row.
Done when: `grep -ri outlook` finds nothing that promises support; tests
and harness pass; there is a harness check for the new message.

#### C1
**XOAUTH2 in the engine.** Who: agent (Rust) · Needs: — · Touches:
`desktop/rata-mail/src/imap.rs`, `smtp.rs`, `lib.rs`.
Do: IMAP `AUTHENTICATE XOAUTH2` and SMTP `AUTH XOAUTH2` as an alternative
credential (`Credential::Password | Credential::OAuth{user, access_token}`).
Classify an expired or rejected token as its own error kind, never `auth`,
so the app refreshes instead of asking for a password.
Done when: scripted-server tests cover success, an expired token and a
refused token, for both IMAP and SMTP. Clippy and fmt are clean.

#### C2
**The sign-in flow in the app.** Who: agent (Rust/Tauri) · Needs: C1,
the [owner]'s Entra client id · Touches: `src-tauri/src/` (new `oauth.rs`),
`vault.rs`, `core.rs` (hot).
Do: authorization code + PKCE with a loopback redirect
(`http://127.0.0.1:<random port>`). The system browser opens the Microsoft
page through the existing `open_link` rules. Scopes are
`https://outlook.office.com/IMAP.AccessAsUser.All`,
`https://outlook.office.com/SMTP.Send` and `offline_access`. The refresh
token goes in the keychain entry where a password would be. Access tokens
are kept in memory only. Refresh on expiry. When a refresh is refused,
park the mailbox with "Sign in to Microsoft again".
Invariant check: no token is written to `mailboxes.json`, logs or the page.
Tokens go to Microsoft's endpoints only.
Done when: unit tests cover the PKCE pieces and the refresh logic, and the
[owner] links a real Outlook.com mailbox (a B2 row).

#### C3
**Interface for C2.** Who: agent (front end) · Needs: C2 · Touches:
`app.html` (hot), `bridge.js` (hot).
Do: in Add mailbox, a Microsoft address shows "Sign in with Microsoft"
instead of the password field, and the parked state shows a re-sign-in
button. Follow DESIGN.md. Add harness checks.
Done when: the harness passes with the new checks, and there are packaged
build screenshots.

#### C4
**Owner registration.** Who: **[owner]** · Needs: — .
Do: in Microsoft Entra, register an app as a public client (mobile and
desktop), personal plus work accounts, with the loopback redirect and the
delegated IMAP and SMTP permissions above. The client id is not a secret.
Give it to the agent doing C2. Work tenants may need admin consent. Note it
in BETA.md.

### Workstream D: the website and its services

#### D1
**[owner] Environment variables in hPanel.** Set everything in LAUNCH.md
§4 **as corrected by [A4](#a4)**. `LICENCE_PRIVATE_KEY` is the VPS key, and
`LICENCE_PUBLIC_KEY` is its public half, the same one as the GitHub secret.
Add `HEALTH_TOKEN` too. Never through chat.

#### D2
**[owner] Database.** Run `rata-next/database.sql` in the Supabase SQL
editor (safe to re-run). Check that `ai_usage` exists and that
`subscriptions` has `domain_addons` and `event_at`.

#### D3
**[owner] Stripe in test mode.** Follow STRIPE-SETUP.md: Base $12.99 and
Pro $23.99 monthly, payment links redirecting to
`https://mailrata.org/account?checkout=success`, a webhook with exactly the
three events, and the customer portal. **No domain add-on.**

#### D4
**Deploy rata-next.** Who: [owner], or an agent through the Hostinger
tools with the owner's explicit go-ahead for this deploy · Needs: D1, D2,
A4 merged · Do: build `npm run build`, start `npm start`, Node 22, from
`main`. If an agent deploys, it uses the build/start operations only
(Rule 6) and polls the build status.
Done when: `/api/licence/renew` answers something other than 404.

#### D5
**Live smoke test.** Who: agent for the public checks, [owner] for the rest
· Needs: D1–D4.
Agent (no credentials needed): `/api/config` (keys present, no service-role
key), `/api/health` 200, `OPTIONS` preflight on `/api/ai` and
`/api/licence/renew` from `tauri://localhost` allowed and from
`https://evil.example` not, `/shots/app-light.webp` 200, the landing page
makes no third-party requests (Playwright), and `index.html` copy matches
the app.
Owner: sign up, pay with test card `4242…`, and check the webhook 200, the
`subscriptions` row and the key on `/account`. Paste the key into v0.1.38:
it must be accepted. Pro: Summarize gives an AI summary. Renewal: follow
BETA.md's steps, or mint a 6-day key on the VPS and watch the app renew it.
Done when: every line is in `MVP-EVIDENCE.md`.

#### D6
**[owner] Stripe live mode.** Repeat D3 in live mode and swap the variables.
Buy Base with a real card, then refund it.

#### D7
**Backups and monitoring.** Who: agent writes, [owner] installs · Touches:
`infrastructure/backup/`.
Do: bring `backup.sh` and its `selftest.sh` up to date with today's schema
(after A4). Write the cron line and the restore drill. Point an uptime
monitor at `/api/health`.
Done when: the selftest passes, and the owner confirms the first nightly
backup and one restore drill.

#### D8
**After launch: email confirmation.** [owner] configures an SMTP sender in
Supabase auth; once a signup receives a confirmation mail, turn
`mailer_autoconfirm` off. **Not on launch day.**

### Workstream E: distribution

#### E1
**[owner] Updater key.** LAUNCH.md §9: generate the key on the owner's own
machine and add the three GitHub secrets. Back up the key offline.

#### E2
**[owner] macOS signing.** Apple Developer Program, a Developer ID
Application certificate, and the six secrets `release.yml` already reads.
Done when a release's dmg passes `spctl -a -vv` (a B3/verify step).

#### E3
**Windows signing.** Who: agent wires it, [owner] buys · Touches:
`release.yml`.
Do: add an optional signing step (Azure Trusted Signing or a PFX through
secrets) that is skipped cleanly when its secrets are absent, like the
macOS one, and signs both the NSIS installer and the exe.
Done when: a run without secrets is still green, and the owner's run with
secrets passes `signtool verify /pa`.

#### E4
**The first self-update.** Needs: E1 and two releases after it.
Owner: install release N by hand, publish N+1, and see "RATA N+1 is ready".
Restart and check the version in the title. Agent: check `latest.json`
names N+1 for every platform key.

### Workstream F: product gaps

#### F1
**RATA's drafts saved to the server.** Who: agent (Rust + front end) ·
Needs: **the [owner]'s decision.** When a draft is saved again, what happens
to RATA's previous copy on the server?
(a) Permanently delete RATA's own old copies only (normal mail-client
behaviour, but it is RATA's first permanent delete).
(b) Move them to Trash (safe, but Trash fills up).
(c) Keep drafts local (no work; drafts do not follow you to your phone).
Recommendation: (a), limited to messages RATA itself APPENDed (checked by
a RATA-set header and the UID it recorded).
Do: IMAP APPEND to Drafts on close and after an idle pause, handle the
changed UID, and implement the old-copy rule. Tests on the scripted server
and B1.

#### F2
**What the website's `app.html` is for.** Who: [owner] decides, agent does
· The website still serves the interface in a browser, where it can reach
no mail. Checkout already sends people to `/account`. Options: (a) make
`/app.html` in a browser a short page, "RATA runs on your computer", with
Download and Log in; (b) keep it as a demo with sample mail; (c) leave it.
Recommendation: (a). The desktop build is untouched either way (it builds
from the same file, so the gate is `window.__RATA_NATIVE__`).

#### F3
**Getting the licence into the app.** Optional for the MVP · Today the
customer copies the key from `/account` and pastes it in. Check the
licence box's "Sign in at mailrata.org" link opens `/account` in the
browser. If it opens the home page instead, point it at `/account`.
Anything bigger (deep links) is after the MVP.

#### F4
**Security review before paid launch.** Who: a fresh agent with no stake in
the code · Needs: B1 and C2 PRs open (review them), then the whole tree ·
Scope: `html.rs` + ammonia config + frame CSP; `links.rs`; `guard.rs`
(and B1's feature gate); `bridge.js` (routes the page can call);
`safe_file_name` / `write_new`; licence verify and renew; `/api/ai`
(prompt fencing, cap, CORS); the Stripe webhook signature and ordering;
OAuth (C2).
Done when: there is a findings list with a severity for each. Every High
has a fix PR with a test.

### Workstream G: release and launch

#### G1
**[owner] Version for launch.** 0.x releases are published as
pre-releases (`release.yml`). Decide whether the MVP ships as `1.0.0`
(a full release) or stays `0.x` beta. Recommendation: `1.0.0` only once
every Part 1 row has evidence.

#### G2
**Release captain (repeats).** Who: one agent at a time · Do: after a batch
of merges, fold the handoffs' docs notes into CLAUDE.md, README.md and
BETA.md. Bump the version in all four places and merge. Run
`verify-release.sh`. Retake `public/shots` if the interface changed
visibly (DESIGN.md → Website).
Done when: the release is verified, and the docs name the version.

#### G3
**Go/no-go.** Who: PM + [owner] · Walk Part 1. Every row needs a link into
`MVP-EVIDENCE.md`. No open High from F4. Backups verified (D7). Then
announce.

---

## Part 6. Order of work

```
Now, in parallel (no owner input needed):
  A3 harness in CI      A4 docs true       C0 stop Outlook claim (lock app.html)
  B1 real-server tests  B3 packaged smoke  C1 XOAUTH2 engine   E3 Windows signing wiring
  B2-agent: write SMOKE.md                 D7-agent: backup scripts

Owner, in parallel (this is the critical path):
  A1 go-ahead on #52 → D1 env → D2 db → D3 Stripe test → D4 deploy → D5 smoke
  E1 updater key   E2 Apple account   C4 Entra app   F1 / F2 / G1 decisions

Then:
  C2 (needs C1 + C4) → C3 → B2 Outlook row
  B2 owner run (needs A1 + D5)  → bug cards → fixes → G2 release
  F4 security review (needs B1, C2 open) → fixes
  E4 first self-update (needs E1 + two releases)
  D6 Stripe live → G3 go/no-go
```

Hot-file locks for the first wave: `app.html` goes to C0, then F2, then C3.
`imap.rs` goes to C1; B1 adds its own test module and touches `imap.rs`
only if it must. `core.rs` is free until C2.

---

## Part 7. Prompt for the project manager

> You are the project manager for RATA's MVP. The plan is
> `docs/MVP-PLAN.md` in `Noallrightokay/center-point-inbox` (read it all,
> with `CLAUDE.md`). Your job is to dispatch the task cards in Part 5 to
> agents, in the order in Part 6, and to hold the gates.
>
> For each dispatch:
> 1. Pick a card whose "Needs" are met and whose hot files are unlocked.
>    Never dispatch an [owner] card to an agent. Put it on the owner's list
>    instead, with the card link.
> 2. Give the agent a branch name `claude/<card-id>-<short-slug>` and a
>    prompt that is exactly: "Do task <ID> of docs/MVP-PLAN.md
>    (link: docs/MVP-PLAN.md#<id>). Follow Part 4 in full. Hand back using
>    Part 8." Add only context that is not in the file: the owner's
>    decision for F1/F2/G1, the Entra client id for C2, or the error text
>    for a bug card.
> 3. Record the lock on any hot file the card touches, and release it when
>    the PR merges.
>
> When a handoff arrives:
> - Reject it if the evidence is missing (test output tails, links), if it
>   claims something without showing it, or if the version was bumped
>   without G2. Send it back with what is missing.
> - Queue its "Docs note" for the next G2.
> - Turn each "Found on the way" item into a new bug card (ID `BUG-n`,
>   same format, with the exact error text), rather than widening the
>   current task.
>
> Keep a status table (card, agent, branch, PR, state, blocker) and post it
> to the owner after every change of state. Everything the owner must do
> goes in one list, ordered by the critical path in Part 6, each item with
> its "Done when".
>
> Never: ask an agent or the owner to put a secret in chat; merge a red PR;
> dispatch two open tasks on one hot file; let "should work" count as
> evidence. The MVP is done only at G3.

---

## Part 8. Handoff template

Every agent ends with this, in the PR body and as its final message:

```
## Handoff: <card ID> <title>
Branch / PR: <branch> / <PR link>          State: ready | blocked | partial

What changed:        <files, one line each, why>
Evidence:            <commands run + last lines of output; links to CI runs>
Done-when check:     <each criterion from the card → met / not met + proof>
Not done / skipped:  <and why>
Found on the way:    <bugs or wrong docs outside this card, with exact error text>
Docs note:           <what CLAUDE.md / README.md / BETA.md should say, for G2>
Unblocks:            <card IDs>
Needs from owner:    <decisions or [owner] steps, never a secret's value>
```
