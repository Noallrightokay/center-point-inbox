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

## Part 2. State of play (checked 2026-09-28, updated 2026-09-30)

### Done, and how it was checked

- **The desktop app** has been built through v0.1.43 (0.1.44 is merged
  for release, below). The feature list is
  `CLAUDE.md` → Status: licence, adding a mailbox, sync, IDLE, folders,
  Sent/Archive/Spam/Drafts, Gmail's archive, compose with Cc/Bcc and
  attachments, reply and forward, HTML mail in a locked-down frame, the
  Format Bridge, notifications, signatures, the updater code, the
  redesign, Sign in with Microsoft (live once [C4](#c4) is done), saved
  files marked as downloads, and RATA's own drafts saved to the mailbox.
- **Releases** v0.1.0 to v0.1.43 are published on GitHub. v0.1.2 onwards are
  pre-releases. v0.1.37 was downloaded and checked: 5 assets, the licence
  public key embedded, no font CDN, and it launches as "RATA 0.1.37 beta".
- **v0.1.38** (Gmail's archive) is [PR #52](https://github.com/Noallrightokay/center-point-inbox/pull/52).
  CI was green on `aebadb1`, and it merged as `8511dad` ([A1](#a1)).
- **v0.1.39** was released and verified: five assets, the licence public
  key present, no font CDN, and it launches as "RATA 0.1.39 beta". It was
  the first run of the install-and-launch smoke test, which passed on
  Linux, macOS (both) and Windows ([run 36490218903](https://github.com/Noallrightokay/center-point-inbox/actions/runs/36490218903)).
- **v0.1.40** was released and verified: five assets, the licence public
  key present in the `.deb` and the Windows exe, no font CDN, and it
  launches as "RATA 0.1.40 beta" (`verify-release.sh`, 2026-09-29). The
  install-and-launch smoke test passed on all four builds: Linux, macOS
  (both) and Windows ([run 36507670335](https://github.com/Noallrightokay/center-point-inbox/actions/runs/36507670335)).
- **v0.1.41** was released and verified: five assets (both dmgs, the
  `.deb`, the `.AppImage` and the Windows `-setup.exe`), and it launches
  as "RATA 0.1.41 beta". The install-and-launch smoke test passed on all
  four builds ([run 36516132763](https://github.com/Noallrightokay/center-point-inbox/actions/runs/36516132763)).
- **v0.1.42** was released and verified ([PR #85](https://github.com/Noallrightokay/center-point-inbox/pull/85),
  merged as `0ca0da8`): five assets, the licence public key present in the
  `.deb` and the Windows exe, no font CDN, and it launches as "RATA 0.1.42
  beta" (`verify-release.sh`, 2026-09-29). The install-and-launch smoke
  test passed on all four builds ([run 36574649823](https://github.com/Noallrightokay/center-point-inbox/actions/runs/36574649823)).
- **Released in 0.1.43** (wave 5a, all of 5b (H3, H6, H9, H12), H4 of 5c, and the pre-launch audit's
  bug cards; [#102](https://github.com/Noallrightokay/center-point-inbox/pull/102),
  merged as `85da389` once the owner's `mint.sh check` said OK, and
  verified 2026-09-30):
  the licence mint script, `infrastructure/licence/mint.sh` ([#90](https://github.com/Noallrightokay/center-point-inbox/pull/90));
  [H1](#h1), the privacy policy and terms ([#92](https://github.com/Noallrightokay/center-point-inbox/pull/92)); [H2](#h2), the help
  page at `/help` ([#91](https://github.com/Noallrightokay/center-point-inbox/pull/91)); [H4](#h4), keyboard shortcuts ([#103](https://github.com/Noallrightokay/center-point-inbox/pull/103)); [H6](#h6), search on the server ([#104](https://github.com/Noallrightokay/center-point-inbox/pull/104)); [H9](#h9), undo send ([#105](https://github.com/Noallrightokay/center-point-inbox/pull/105)); BUG-C, RATA's copy follows the server ([#106](https://github.com/Noallrightokay/center-point-inbox/pull/106)); [H12](#h12), quoted text folded ([#107](https://github.com/Noallrightokay/center-point-inbox/pull/107)); [H3](#h3), address suggestions and named
  addresses in the engine ([#99](https://github.com/Noallrightokay/center-point-inbox/pull/99)); [H7](#h7), Unsubscribe ([#95](https://github.com/Noallrightokay/center-point-inbox/pull/95));
  [H8](#h8), Copy diagnostics and Help in Settings ([#96](https://github.com/Noallrightokay/center-point-inbox/pull/96));
  [BUG-A](#bug-a), the AI relay on long mail ([#94](https://github.com/Noallrightokay/center-point-inbox/pull/94)); [BUG-D](#bug-d),
  words that were not true yet on the site and in the docs ([#97](https://github.com/Noallrightokay/center-point-inbox/pull/97));
  [BUG-L](#bug-l), the licence renewing while RATA is open ([#100](https://github.com/Noallrightokay/center-point-inbox/pull/100));
  [BUG-M](#bug-m), Trash by name, the page never hiding mail the server
  kept, the server's words on a refused password, 587 first for Outlook
  and iCloud, Zoho regions ([#101](https://github.com/Noallrightokay/center-point-inbox/pull/101)); [BUG-R](#bug-r), the release gate
  and a feed file per installation ([#93](https://github.com/Noallrightokay/center-point-inbox/pull/93)); [BUG-S](#bug-s), the
  subscription event that comes before its checkout ([#98](https://github.com/Noallrightokay/center-point-inbox/pull/98)). Each is
  done in code; none is verified in a release yet. The website halves
  (H1, H2, BUG-A, BUG-S, BUG-L's renew route) reach mailrata.org only at
  [D4](#d4), and BUG-S needs `database.sql` §6 run first. Left for
  0.1.44: H5, H10, H11.
- **Merged for 0.1.44** (the rest of wave 5c, which completes
  Workstream H; [PR "Release 0.1.44"](https://github.com/Noallrightokay/center-point-inbox/pulls?q=is%3Apr+%22Release+0.1.44%22),
  not yet merged, released or verified): [H11](#h11), picture attachments
  shown in the message and opened large, the type read from the bytes in
  Rust ([#108](https://github.com/Noallrightokay/center-point-inbox/pull/108));
  [H5](#h5), **In this conversation** under an open message, linked by
  Message-ID, In-Reply-To and the last id of References, never by subject
  ([#110](https://github.com/Noallrightokay/center-point-inbox/pull/110));
  CI's apt steps retry a stalled download and give up after 8 minutes
  instead of hanging a job ([#111](https://github.com/Noallrightokay/center-point-inbox/pull/111));
  [H10](#h10), the accessibility pass: the list a listbox with one Tab
  stop, dialogs trapped and labelled, toasts announced, zoom allowed, and
  axe in the harness failing on any serious or critical rule
  ([#112](https://github.com/Noallrightokay/center-point-inbox/pull/112));
  and this plan's update ([#109](https://github.com/Noallrightokay/center-point-inbox/pull/109)).
  Each is done in code and in the harness; none is seen in a packaged
  build or a release yet. The screenshots in `public/shots` were retaken
  by H10 after the last interface change.
- **Merged for 0.1.42:** D5's public half, `rata-next/scripts/live-check.sh`
  ([#80](https://github.com/Noallrightokay/center-point-inbox/pull/80));
  TIDY-1, a docs sweep against the code ([#81](https://github.com/Noallrightokay/center-point-inbox/pull/81));
  [F4](#f4) part 3, the review of code added since part 1: no High, two
  Medium, five Low ([#82](https://github.com/Noallrightokay/center-point-inbox/pull/82));
  SEC-6, the plan taken from Stripe's subscription events (every checkout
  had been recorded as Base), P3-1, P3-4 (partly), P3-5 and P3-6
  ([#83](https://github.com/Noallrightokay/center-point-inbox/pull/83));
  SEC-5, IMAP sign-in errors redacted (row 8), the loopback gate on the
  release profile (row 5), P3-2 and P3-3
  ([#84](https://github.com/Noallrightokay/center-point-inbox/pull/84)).
- **State at 0.1.42**, against Part 1: unchanged from 0.1.41 below. The
  website half of SEC-6 (and SEC-4) reaches mailrata.org only at
  [D4](#d4), and the webhook must also subscribe to
  `customer.subscription.created` ([D3](#d3), `STRIPE-SETUP.md` §3).
  Still open from the review: row 15's in-window renewals and the rest of
  P3-4's lists (owner decisions), and [D8](#d8).
- **State at 0.1.41**, against Part 1: M1 is code-complete but the site is
  not deployed ([D4](#d4)); M2, M3, M5, M8 and M9 wait on the owner's
  [D1](#d1)–[D5](#d5); M4 waits on signing ([E2](#e2), [E3](#e3)'s
  certificates); M6, M7 and M11 wait on the [B2](#b2) run; M10 waits on
  [E1](#e1).
- **[F1](#f1)** is done ([#78](https://github.com/Noallrightokay/center-point-inbox/pull/78)):
  RATA's own drafts are saved to the mailbox's Drafts folder, replacing
  only RATA's earlier copy. It shipped in 0.1.41. The Gmail All Mail check is
  for [B2](#b2) (SMOKE.md X7).
- **SEC-4** is done ([#77](https://github.com/Noallrightokay/center-point-inbox/pull/77)):
  the review's rows 6, 7, 11 and 14, the SMTP half of 8 and part of 15.
  Still open: the IMAP half of 8, row 5, and row 15's in-window renewals
  (a business decision). It shipped in 0.1.41; the website half reaches
  mailrata.org only when it is redeployed ([D4](#d4)).
- **[F4](#f4)'s Mediums:** 1 and 3 are fixed ([#69](https://github.com/Noallrightokay/center-point-inbox/pull/69)),
  2 is fixed ([#71](https://github.com/Noallrightokay/center-point-inbox/pull/71),
  with [#75](https://github.com/Noallrightokay/center-point-inbox/pull/75)
  asking before a disguised program is saved), and 4 is [D8](#d8).
- **Tests** (counted on the 0.1.44 branch, 2026-09-30). Engine (297, plus
  16 against real Dovecot and GreenMail), shell (130) and website (785)
  suites run in CI (`desktop-ci.yml`, `rata-next-ci.yml`), and from 0.1.43
  also in `release.yml`'s `gate` before anything is built. The **desktop
  UI harness** (358 checks that drive
  the real interface against a fake backend, in `desktop/rata-app/harness/`,
  among them axe-core accessibility scans since H10)
  runs in CI as *Desktop interface, driven* ([A3](#a3),
  [#60](https://github.com/Noallrightokay/center-point-inbox/pull/60)).
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
| **Outlook, Hotmail and Microsoft 365 cannot work.** Microsoft turned off password (Basic) sign-in to Outlook.com over IMAP in September 2024. It requires OAuth 2.0 now, and RATA only does passwords. Exchange Online IMAP Basic auth is gone too. | [Microsoft Support: Outlook.com and Basic authentication](https://support.microsoft.com/en-us/office/outlook-and-other-apps-are-unable-to-connect-to-outlook-com-when-using-basic-authentication-f4202ebf-89c6-4a8a-bec3-3d60cf7deaef), [Deprecation of Basic auth in Exchange Online](https://learn.microsoft.com/en-us/exchange/clients-and-mobile-in-exchange-online/deprecation-of-basic-authentication-exchange-online); `grep -i oauth` finds nothing in the engine | [C1](#c1)–[C4](#c4). **Done in code:** C1 ([#59](https://github.com/Noallrightokay/center-point-inbox/pull/59)), C2+C3 ([#67](https://github.com/Noallrightokay/center-point-inbox/pull/67)), shipped in 0.1.39; live only after the owner's [C4](#c4) |
| The website, the app and BETA.md all **tell people Outlook works**. | `index.html:226`, `app.html:886,2301,3069`, `BETA.md:49` | [C0](#c0): **done** ([#57](https://github.com/Noallrightokay/center-point-inbox/pull/57)) |
| **Installers are unsigned.** macOS signing is wired in `release.yml` but has no certificates. Windows signing is not wired at all. | `release.yml` lines 240–286 | [E2](#e2), [E3](#e3): Windows wiring **done** ([#55](https://github.com/Noallrightokay/center-point-inbox/pull/55)); both wait for the owner's certificates |
| The **UI harness is not in CI**. | workflows list | [A3](#a3): **done** ([#60](https://github.com/Noallrightokay/center-point-inbox/pull/60)) |
| **Nothing is tested against a real IMAP server**, not even a local one: the outbound guard refuses private addresses. | `guard.rs` | [B1](#b1): **done** ([#58](https://github.com/Noallrightokay/center-point-inbox/pull/58)); its findings fixed in BUG-1..3 ([#66](https://github.com/Noallrightokay/center-point-inbox/pull/66)) |
| RATA's **own drafts are not saved to the server**. | CLAUDE.md | [F1](#f1): **done** ([#78](https://github.com/Noallrightokay/center-point-inbox/pull/78)), shipped in 0.1.41; Gmail's All Mail to be checked in [B2](#b2) |
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
cd rata-next && npm ci && npm run build && npm test   # npm test reads the build
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
five events in STRIPE-SETUP.md §3, and the customer portal. **No domain add-on.**

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
Agent (no credentials needed): run `rata-next/scripts/live-check.sh`
([#80](https://github.com/Noallrightokay/center-point-inbox/pull/80)), which
covers the routes, preflight, config shape, headers and landing page below,
then the rest by hand: `/api/config` (keys present, no service-role
key), `/api/health` 200, `OPTIONS` preflight on `/api/ai` and
`/api/licence/renew` from `tauri://localhost` allowed and from
`https://evil.example` not, `/shots/app-light.webp` 200, the landing page
makes no third-party requests (Playwright), and `index.html` copy matches
the app.
Owner: sign up, pay with test card `4242…`, and check the webhook 200, the
`subscriptions` row and the key on `/account`. Paste the key into the current release (v0.1.42 or later):
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
Runs: 0.1.42 folded in #80–#84 (docs notes, the review's P3 rows, the
small follow-ups) and bumped the version; released and verified 2026-09-29.
0.1.43 folded in #90–#101 (H1, H2, H3, H7, H8, BUG-A, BUG-D, BUG-L, BUG-M,
BUG-R, BUG-S and the mint script), added the help, privacy and terms pages
to `sw.js`'s shell, and bumped the version ([#102](https://github.com/Noallrightokay/center-point-inbox/pull/102));
held for the owner's licence signing key, not yet released or verified. It
is the first release through BUG-R's `gate`: watch that run. 0.1.43 released and verified 2026-09-30 ([run 36725730252](https://github.com/Noallrightokay/center-point-inbox/actions/runs/36725730252)): the new `gate` passed first, all four installers were installed and launched, `verify-release.sh v0.1.43` found five assets, the licence key in the `.deb` and the `.exe`, no font CDN, window "RATA 0.1.43 beta".
0.1.44 folded in #108–#112 (H11, H5, the CI apt retries, H10, and the plan
update #109), with DESIGN.md's `--thumb`, `--faint`-on-tint and inset list
focus ring, and bumped the version (PR "Release 0.1.44", 2026-09-30);
engine 297, shell 130, harness 358, website 785, all green on the branch.
The screenshots are H10's, taken after the last interface change. Not yet
merged, released or verified: the PM merges.

#### G3
**Go/no-go.** Who: PM + [owner] · Walk Part 1. Every row needs a link into
`MVP-EVIDENCE.md`. No open High from F4. Backups verified (D7). Then
announce.

### Workstream H: complete and better (wave 5, 2026-09-29)

*State: **complete in code.** Every card, H1 to H12, is merged: H1–H4,
H6–H9 and H12 released in 0.1.43; H5, H10 and H11 merged for 0.1.44 and
not yet released. What is left is proof on real providers ([B2](#b2)) and
a packaged-build look at H5, H10 and H11.*

What a first paying customer expects of a mail client and a paid website,
and what beta support needs, that RATA does not have at 0.1.42. Found by
reading the interface, the website and BETA.md §5 against the code: no
privacy or terms page, no help page, no address suggestions when writing,
no keyboard shortcuts, search that never asks the server, no unsubscribe,
no way to copy diagnostics into a bug report, no undo send, no
conversation view, and thin accessibility markup. Every card is an
agent's; two need the [owner]'s review after. Part 4's rules apply, hot
files included: `app.html` is on nearly every card here, so the PM runs
them in the order under *Waves* and each agent starts from the `main`
that holds the cards before it.

#### H1
*State: **done** in code ([#92](https://github.com/Noallrightokay/center-point-inbox/pull/92)), released in 0.1.43.*
**Privacy policy and terms of service on the website.** Who: agent
(website) · Needs: nothing to start; the [owner] reviews before launch ·
Do: `rata-next/public/privacy.html` and `terms.html` in DESIGN.md's
voice and tokens, true to the architecture: what mailrata.org stores
(the login email, the subscription record with its Stripe customer id,
the AI usage count), what it never sees (mail, mail passwords, OAuth
tokens: they stay on the customer's computer), what the AI relay sends
to Anthropic and that neither RATA nor Anthropic keeps it (CLAUDE.md,
`lib/ai.js`), that Stripe processes payment, what deleting the account
removes and when it is refused (`lib/account.js`), the licence (30 days,
renewed while subscribed), and the plans. Mark every fact only the owner
knows as `[OWNER: legal entity name]`, `[OWNER: postal address]`,
`[OWNER: contact email]`, `[OWNER: governing law]`. Link both pages from
the footer of `index.html`, from `auth.html` beside the sign-up button
("By creating an account you agree to…") and from `account.html`. Add
a LAUNCH.md step, before §6's go-live check, to fill the placeholders,
and make `scripts/live-check.sh` fail while `[OWNER:` is still on the
live pages. Tests in `rata-next/tests`: both pages exist, are linked
from the three places, and claim nothing the code contradicts.
Done when: the pages are live in the repo with every unknown marked
`[OWNER:`, the links are in, live-check.sh refuses a placeholder, and
the suite is green. Not: legal advice; the owner reads it before D6.

#### H2
*State: **done** in code ([#91](https://github.com/Noallrightokay/center-point-inbox/pull/91)), released in 0.1.43.*
**A help page: app passwords per provider, what errors mean, how to
report.** Who: agent (website) · Needs: nothing · Do:
`rata-next/public/help.html`, DESIGN.md's voice: for Gmail, iCloud,
Yahoo, Fastmail, a custom domain and Microsoft, how to link the mailbox
(app password or Sign in with Microsoft), with links to each provider's
own official page for app passwords, drawn from SMOKE.md §1 and BETA.md
§3; the sentences RATA can say when a link fails and what each means
(from `desktop/rata-mail/src/imap.rs`, `smtp.rs`, `resolve.rs`,
`guard.rs` and `core.rs`: certificate not trusted, password refused,
provider needs an app password, no server found, refused to connect to a
local address, Microsoft needs Sign in); how to report a bug (the *Beta
bug* template, what to include, what never to paste: the app password,
the licence key). Link it from `index.html`'s footer and from BETA.md.
Tests: the page exists, is linked, every provider named in SMOKE.md's
grid has a section, every external link is https and points at the
provider's own domain. Done when: the page is in, linked, and the suite
is green. Not: the in-app Help link (H8 adds it).

#### H3
*State: **done** in code ([#99](https://github.com/Noallrightokay/center-point-inbox/pull/99)), released in 0.1.43.*
**Address suggestions in the composer.** Who: agent (interface) · Needs:
nothing · Do: To, Cc and Bcc suggest people RATA has seen (every From,
To and Cc of held mail, plus the People list), as you type, in a
dropdown driven by the keyboard (arrows, Enter, Escape) and the mouse;
several addresses separated by commas; a chosen person is inserted as
`"Name" <addr>` when there is a name, and `core::outgoing` must accept
that form (check `compose.rs` and add a test if it does not). Spam and
drafts never feed the list (`mailRecord` already keeps spam out of
People; keep it that way). No network, no new storage: the list is
built from what is held. Harness checks: typing two letters shows a
match, Enter fills it, a second address follows a comma, Escape closes
without changing the field, nothing is suggested from a spam sender.
Done when: the harness checks pass and `send_mail` accepts what the
composer now writes. Hot file: `app.html` (composer region only).

#### H4
**Keyboard shortcuts.** Who: agent (interface) · Needs: H3 merged (both
touch key handling) · Do: in the list, `j`/`k` and the arrows move, Enter
opens, `x` ticks; on a message, `r` reply, `a` reply all, `f` forward,
`e` archive, `#` or Delete deletes (to Trash, as the button does), `s`
star, `u` unread, `m` move to; anywhere, `c` compose, `/` search, `g i`
inbox, Escape closes the pane or composer, `?` shows the sheet. Never
while an input, textarea or contenteditable has focus. A Settings row
lists them and can turn them off. Set `spellcheck="true"` and
`lang` on the composer's fields on the way, since the webview spells
only when asked. Harness checks for each key, and that typing `j` in
the composer types a `j`. Done when: the harness checks pass and the
sheet matches the code (one table, read by both). Hot file: `app.html`.

*State: **done** in code ([#103](https://github.com/Noallrightokay/center-point-inbox/pull/103)), released in 0.1.43.*

#### H5
*State: **done** in code ([#110](https://github.com/Noallrightokay/center-point-inbox/pull/110)), merged for 0.1.44; not yet released.*
**Conversation view.** Who: agent (Rust engine + app + interface) ·
Needs: H4 merged · Do: the engine keeps the last id of `References`
on `Message` (`references_last`, `serde(default)`, checked like
`message_id`); the bridge passes it; when a message is open, the
reading pane shows **In this conversation**: every other held message
whose `message_id`, `in_reply_to` or `references_last` links it into
the same chain (ids only; never by subject), oldest first, each a line
with sender, date and the first words, and one click opens it. Sent
mail joins the chain (it carries the Message-ID RATA wrote). Mail
stored before the field has no `references_last` and links by the two
ids it has. Engine test on the scripted server; harness checks with a
three-message chain across inbox and Sent, and one that a same-subject
stranger is not pulled in. Done when: those pass. Hot files: `imap.rs`,
`core.rs`, `bridge.js`, `app.html` (reading pane).

#### H6
**Search the server too.** Who: agent (Rust engine + app + interface) ·
Needs: H3 merged (app.html) · Do: `rata_mail::imap::search_folder(
account, folder, query, limit)`: one `UID SEARCH` of `OR OR FROM
<q> SUBJECT <q> TEXT <q>` in the inbox, with the query sent as an IMAP
literal (never inside a quoted string: a `"` or a non-ASCII character
must not be able to change the command), `CHARSET UTF-8` where the
server wants it, refused when the server refuses (the existing
`search()` rule), then the newest `limit` (50) of the UIDs found through
`fetch_uids`. App command `search_mail` (licence, linked, not parked:
`usable`), bridge route `/api/mail/search`. In the app only, a search
that ran locally offers **Search on the server** per linked mailbox;
results arrive like older mail (absorbed and stored, labelled with the
mailbox), and the button says how many came. Not for Gmail's archive
or named folders in this version. Engine tests on the scripted server
including a query with `"`, `\` and an accented letter; a loopback test
against Dovecot; harness check for the button and the label. Done when:
those pass. Hot files: `imap.rs`, `core.rs`, `bridge.js`, `app.html`
(search region).

*State: **done** in code ([#104](https://github.com/Noallrightokay/center-point-inbox/pull/104)), released in 0.1.43.*

#### H7
*State: **done** in code ([#95](https://github.com/Noallrightokay/center-point-inbox/pull/95)), released in 0.1.43.*
**Unsubscribe.** Who: agent (Rust engine + interface) · Needs: nothing
(reading-pane actions region; H11 follows it) · Do: `body::read` keeps
`List-Unsubscribe` as `Message.unsubscribe: Option<Unsubscribe { https:
Option<String>, mailto: Option<String> }>` (`serde(default)`): the
first `https://` URL and the first `mailto:` of the header, each checked
by the same rules as a link (`links::classify` for the URL: http(s)
only, no `user@host` disguise; a mailto whose address parses), and
`List-Unsubscribe-Post` is read but **never** acted on: RATA makes no
request of its own to a stranger's server. The reading pane, for inbox
mail that has one, shows **Unsubscribe**: an https link goes through
the same "Open this link in your browser?" dialog naming the host as
any link; a mailto opens RATA's composer with the address, subject and
body the header gave. Mail stored before the field shows nothing until
it is re-read. Engine tests with the header's real shapes (angle
brackets, several entries, folded, both kinds); harness checks for both
paths and for no button on Sent and Spam. Done when: those pass. Hot
files: `imap.rs` (or `body.rs`), `core.rs`, `bridge.js`, `app.html`
(reading-pane actions).

#### H8
*State: **done** in code ([#96](https://github.com/Noallrightokay/center-point-inbox/pull/96)), released in 0.1.43.*
**Copy diagnostics, and a Help link, in Settings.** Who: agent (Rust +
interface) · Needs: nothing (Settings region) · Do: a Rust command
`diagnostics` that returns one plain-text block: app version, OS and
architecture, whether the build has a licence key, an updater key and
a Microsoft client id; the licence's plan and expiry day (no key, no
email); for each linked mailbox, an index (never the address), the IMAP
and SMTP hosts and ports and TLS mode, the auth kind (password or
Microsoft), whether it is parked and why, the last error sentence
(already through `said`), which special folders were found and by what
(attribute or name), and whether its live connection is up; the last
refresh time; the store's schema version. **Nothing else**: no
addresses, no message text, no paths that carry a user name. A Rust test
builds the block from a state whose last error was made with a password
in it and asserts the password, its base64 and any `@` are absent. In
Settings, a **Help & diagnostics** section: **Copy diagnostics** puts
the block in the clipboard (or shows it selected, where the webview
refuses the clipboard) and says "Paste this into your bug report"; a
**Help** link opens `https://mailrata.org/help` in the browser through
`open_link`; a **Report a bug** link opens the *Beta bug* issue
template. BETA.md §6 gains "Settings → Copy diagnostics" as the first
step (this card may touch BETA.md). Harness check that the block the
fake backend returns is shown and that no `@` appears. Done when: the
tests and harness pass. Hot files: `core.rs`, `bridge.js`, `app.html`
(Settings region), `BETA.md`.

#### H9
**Undo send.** Who: agent (interface) · Needs: H6 merged · Do: Send
closes the composer and shows "Sending in 5 s · Undo" (Settings: 0, 5,
10 s); the message goes to `send_mail` when the count ends, or at once
if the window is closing (`beforeunload`/`pagehide`) or the app is
quitting; **Undo** reopens the composer with the whole draft (To, Cc,
Bcc, subject, text, files, forward references, thread, `CMP_DRAFT`).
RATA's server copy of a draft (F1) is moved to the Trash only after
the real send succeeds, never at Undo. A second Send while one is
counting down queues behind it. Harness checks: Undo restores every
field and sends nothing; the count ending sends once; two sends in a
row send two. Done when: those pass. Hot file: `app.html` (send path).

*State: **done** in code ([#105](https://github.com/Noallrightokay/center-point-inbox/pull/105)), released in 0.1.43.*

#### H10
*State: **done** in code ([#112](https://github.com/Noallrightokay/center-point-inbox/pull/112)), merged for 0.1.44; not yet released.*
**Accessibility pass, checked by axe.** Who: agent (interface + harness)
· Needs: every other H card that touches `app.html` merged (runs last,
alone) · Do: every icon-only button gets a name (`aria-label`), the mail
list is a `listbox`/`option` or `grid` with roving focus and a visible
focus ring, toasts announce (`role="status"`), dialogs are `role=dialog`
with `aria-modal` and a focus trap (the disguised-program question
already traps; reuse it), the reading pane and composer are landmarks,
colour is never the only signal for unread or starred, `prefers-reduced-
motion` turns off what moves. Add `@axe-core/playwright` to the
harness and run it on the inbox, an open message, the composer and
Settings, failing on any serious or critical rule. Done when: axe is
clean at those four points and the harness is green. Hot file:
`app.html` (wide).

#### H11
*State: **done** in code ([#108](https://github.com/Noallrightokay/center-point-inbox/pull/108)), merged for 0.1.44; not yet released.*
**Picture attachments shown in the message.** Who: agent (Rust +
interface) · Needs: H7 merged (same region) · Do: an attachment whose
name ends in png, jpg, jpeg, gif or webp and is under 5 MB shows as a
thumbnail under the message, fetched through `read_attachment` (the
existing path, base64) and shown as `data:image/<type>;base64,` where
the type comes from the file's magic bytes in Rust, never from the
message's declared type or the name; a file whose bytes are not that
picture is listed, not shown. Click opens it large in the pane (still a
`data:` image, no navigation, no new window). Saving is unchanged. The
app CSP already allows `img-src data:`; do not widen it. Rust tests for
the sniffing (PNG, JPEG, GIF, WebP, and a PNG named `.jpg`, and an HTML
file named `.png` refused); harness checks that a picture shows and a
non-picture does not. Done when: those pass. Hot files: `core.rs`,
`bridge.js`, `app.html` (attachments).

#### H12
**Quoted text folded, and small reading polish.** Who: agent
(interface) · Needs: H9 merged · Do: in the Text view, the quoted part
of a reply (lines starting `>` , or from a line matching "On … wrote:"
or "-----Original Message-----" to the end) is folded behind **Show
quoted text**; the signature block after `-- ` is shown lighter; long
URLs in the Text view are cut for display but open whole. Nothing
changes in the formatted (HTML) view. Harness checks with a reply that
has all three. Done when: they pass. Hot file: `app.html` (text view).

*State: **done** in code ([#107](https://github.com/Noallrightokay/center-point-inbox/pull/107)), released in 0.1.43.*

#### Bug cards from the pre-launch audit (2026-09-29)
Ten finders read the MVP journey at 0.1.42 and reported 47 findings. The
PM checked each group below against the code before writing it here;
finder text is in the PM's notes, not the repository. Each card is one
branch and one PR, with a failing test first where the code allows.

##### BUG-L
*State: **done** in code ([#100](https://github.com/Noallrightokay/center-point-inbox/pull/100)), released in 0.1.43.*
**The licence renews while RATA is open, and a bad paste never costs a
good licence.** Who: agent (bridge + shell + interface) · Needs: H7 and
H8 merged (same files) · Found: `renew()` runs only from
`settleLicence()`, once at launch (`bridge.js`), and the page never reads
a refresh's `unlicensed` answer, so a copy started more than a week
before expiry and left open past it stops fetching mail in silence, and
its IDLE connections close (`watchable`). Also: the licence box says RATA
"will renew itself" but never tries; a renewal the app cannot verify
replaces a still-valid token on disk (`renew()` stores it before
checking); a key pasted with a line break inside is "could not be read";
a junk paste replaces an expired but renewable token; the AI relay's
`expired` answer is shown instead of renewing. Do: run the renewal path
on a timer (every six hours, beside the update check) and whenever a
refresh or the relay answers `unlicensed`/`expired`, then fire
`rata-standing`; keep a new token only if it verifies and keep the old
one otherwise (in Rust, `set_licence`); strip all whitespace from a
pasted or presented key (app, and `/api/licence/renew` on the site);
refuse to replace a renewable stored token with a malformed or forged
one; the box retries on `online` and has **Try again**. Harness checks
for each, with the fake backend's clock. Done when: those pass and a
licence left open across its expiry renews with no relaunch.

##### BUG-S
*State: **done** in code ([#98](https://github.com/Noallrightokay/center-point-inbox/pull/98)), released in 0.1.43.*
**From paying to the key on `/account`, without a dead end.** Who:
agent (website) · Needs: H1 merged (same pages) · Found: in the common
event order (`customer.subscription.created` before the checkout) the
subscription event gets 409 and the plan lands only on Stripe's retry,
hours later in test mode, while `/account` waits 20 s and then says the
payment "has not reached us yet" beside two buy buttons; `/api/licence`
cannot tell a paid `incomplete` row from none. The signed-out
"Create an account" link on `/account` drops `next=account` and
`checkout=success`. The home page's and `/account`'s buy links carry no
`prefilled_email`/`client_reference_id` although the licence is keyed on
the email. Settings offers a live Base customer "Switch to Pro" through
the Pro Payment Link, which makes a second Stripe customer that the
webhook refuses as a conflict: a double charge that never applies.
`customer.subscription.created` at the same second as an `updated`
passes the ordering guard on equality and can lower a live row to
`incomplete`. Copy: auth.html promises "connect your email straight
after"; `/account` says changing plan or a reissue stops a leaked key,
which nothing does (review row 15). Do: `/api/licence` answers `pending`
for an `incomplete` row that has a customer, and `/account` keeps
"Setting up your licence" (no buy buttons) for it, polling longer; the
webhook stores a subscription event that finds no row (a
`pending_subscriptions` row keyed by customer, applied when the checkout
writes the row) instead of relying on the retry; signed-out links carry
`next=account` and the just-paid state; every buy link built for a
signed-in person carries both parameters; a live plan's Settings shows
only the portal for switching; `created` uses a strict guard; the two
sentences say what is true. Rewrite LAUNCH.md §6 check 6 to match. Tests for each in `rata-next/tests`. Done when:
those pass and a checkout whose subscription event arrives first shows
the key within the page's wait, with no Stripe retry.

##### BUG-M
*State: **done** in code ([#101](https://github.com/Noallrightokay/center-point-inbox/pull/101)), released in 0.1.43.*
**Delete, sign-in errors and sending on real providers.** Who: agent
(engine) · Needs: H7 merged (`imap.rs`) · Found: Trash is found only by
the `\Trash` attribute, with no name fallback as Sent, Spam, Drafts and
Archive have, so on a server that declares nothing Delete is refused
while the page has already hidden the message for good (`S.gone`); a
refused password at link time drops the server's own sentence (Gmail's
"IMAP access is disabled for your domain" becomes "use an app
password"); any network failure at a known provider (no DNS, blocked
port, "too many simultaneous connections") asks a Gmail customer for a
server address (`NeedsHost`); sending tries 465 first with 10 s per
address, so a Microsoft or iCloud send waits a minute before 587; a
Zoho EU or India custom domain is sent to `imap.zoho.com`. Do: a
`TRASH_NAMES` exact-name fallback ("Trash", "Deleted Items", "Deleted
Messages", "Bin", "INBOX.Trash"), and the page restores a message when
its trash is refused; `refusal` appends "The server said: …" (already
through `said`); a table or known-MX provider that could not be reached
says so and never asks for a host; known submission hosts get their
documented port first, and the port that worked is remembered per
mailbox; Zoho's regional hosts from the MX. Tests on the scripted
server, loopback for Trash by name on GreenMail. Done when: those pass.

##### BUG-A
*State: **done** in code ([#94](https://github.com/Noallrightokay/center-point-inbox/pull/94)), released in 0.1.43.*
**The AI relay on long mail and long answers.** Who: agent (website +
interface) · Needs: nothing · Found: Summarize and Translate send an
opened message whole, and the route refuses any body over 80 000
characters before it cuts the text, so the longest mail (a newsletter
runs to 100 000) gets "more text than RATA sends at once"; a translation
stopped at `max_tokens` is returned as complete; a briefing stopped at
`max_tokens` parses as no tasks, so the page clears every flag and says
all is clear; a deploy without `LICENCE_PUBLIC_KEY` tells licensed
customers they are not licensed. Do: the page cuts to a little over
`LIMITS.text` before sending; the route reads `stop_reason` and sets
`cut` for a translation and answers an error for a cut briefing, and
the page keeps existing flags then; `no-public-key` answers "AI is not
switched on for RATA yet." with a server log line. Tests in
`rata-next/tests/ai.test.mjs` and a harness check. Done when: those pass.

##### BUG-R
*State: **done** in code ([#93](https://github.com/Noallrightokay/center-point-inbox/pull/93)), released in 0.1.43.*
**Release pipeline and update feed.** Who: agent (CI + shell) · Needs:
nothing · Found: `release.yml` builds and publishes with no test having
passed, and `main` has no branch protection; the update feed is rebuilt
from this release's assets only, and the plugin answers
`TargetsNotFound` for a platform missing from it, which `update.rs`
shows as "cannot update itself"; `verify-release.sh` expects "beta" in
every title, wrong for 1.x; Desktop CI does not run when
`rata-next/package*.json` changes (the harness uses its Playwright), and
neither workflow runs when only its own file changes. Do: a `gate` job
in `release.yml` (engine and shell tests, `sync-ui.sh`, the harness)
that `create-release` and `installers` need; the feed keeps a platform's
previous entry when this release has none for it; `TargetsNotFound` is
"no update for this computer yet"; the title check follows the
version; the path filters. Done when: the workflow lints (`actionlint`),
the shell tests pass, and a dry run of the feed step on a fixture keeps a
missing platform.

##### BUG-D
*State: **done** in code ([#97](https://github.com/Noallrightokay/center-point-inbox/pull/97)), released in 0.1.43.*
**Words that are not true yet, on the site and in the docs.** Who:
agent (website copy + docs) · Needs: H1 and H2 merged (`index.html`,
BETA.md) · Found: `manifest.json` (the install dialog's name) still
sells texts, Slack, Discord and an audit trail with em dashes and the
old colours; `app/layout.js` metadata is the old pitch; `plan.js` still
promises Pro "your own @mailrata.org addresses", which do not exist;
the home page's privacy column says only one message reaches the relay
(the briefing sends up to 25) and that the site keeps only email, plan
and key (it keeps synced preferences too); "Cancel any time from your
account" points at a page with no cancel; the site's Linux note omits
making the AppImage executable; the macOS first-open ("right-click,
Open") no longer works on macOS 15 for an unsigned app (System Settings,
Privacy & Security, Open Anyway); `desktop/rata-app/README.md` says
there is no OAuth and lists six commands; STRIPE-SETUP names the wrong
file for `LIVE_STATUSES`; VPS-MIGRATION installs Node 20; SMOKE.md is
written for 0.1.41; `database.sql` names an "Edge Function" writer.
Do: make each true, with a test for the manifest and the plan blurb
beside `app.test.mjs`'s old-feature checks. Done when: the suite is
green and each named line reads true.

Not yet read by a finder that finished: the page-to-Rust contract for
every mail action (the finder stopped at the usage limit). It runs again
as an audit card after wave 5b.

#### Waves
- **5a** (in parallel, no `app.html` region shared): H1, H2, H7, H8.
- **5b**: H3, then H6 and H9 (H6 first); H12 after H9.
- **5c**: H4, then H5 and H11; H10 last, alone.
- BUG cards: BUG-A and BUG-R now, beside 5a; BUG-L and BUG-M after H7
  and H8; BUG-S after H1; BUG-D after H1 and H2. [G2](#g2) releases 0.1.43 after 5a and 5b, and 0.1.44
  after 5c, retaking the screenshots (the composer and reading pane
  change visibly).

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

Wave 5 (2026-09-29): Workstream H in the order under its *Waves*,
with the pre-launch audit's BUG cards; G2 after 5b and after 5c.

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
