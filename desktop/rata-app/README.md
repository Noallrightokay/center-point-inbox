# RATA for the desktop

The app that replaced the server.

RATA used to read everybody's mail from a VPS, which meant holding everybody's
mail password. That is a thing worth attacking, and it is a thing worth
attacking *once* — one break, every customer. This app holds nobody's: it runs
on the machine that owns the mailbox, the passwords are in that machine's own
credential store, and what is left on a server is the website, the payment
link, the licence signature, and the AI relay, which passes on the text of a
message only when its customer asks for a summary, a translation or a
briefing.

```
desktop/
  rata-mail/          the mail layer — DNS, IMAP, SMTP, the outbound guard
  rata-app/
    src-tauri/        the shell: vault, store, core, commands, licence, links,
                      oauth (Sign in with Microsoft), watch (IDLE), notify,
                      update, mark (saved files marked as downloads)
    ui-src/           bridge.js — the seam between the interface and Rust
    ui/               assembled at build time, never committed
    harness/          the interface driven against a fake backend, the
                      website's screenshots, release checks
```

## Where things live on a customer's machine

| What | Where | Why there |
|---|---|---|
| Mail passwords, and a Microsoft mailbox's refresh token | The OS credential store — Keychain, Credential Manager, Secret Service | Encrypted at rest, tied to the login session, and already trusted with these same passwords by every mail client they have used |
| Mailbox list | `mailboxes.json` in the app's data directory (0600 on Unix) | Metadata only. There is a test asserting no password ever reaches it |
| Messages | The webview's IndexedDB, one record per message | The same code as the website's page, which keeps nothing of the mail on a server |

**There is no fallback to a file for passwords.** If the credential store cannot
be reached, linking fails and says so. The tempting alternative — an encrypted
file with the key beside it — sounds like resilience and is a plaintext password
with extra steps.

## Building

```bash
# Linux only
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev patchelf

./sync-ui.sh                        # assemble ui/ from rata-next/public
cd src-tauri

# The licence key must be compiled in. A build without it refuses every
# licence — which is the safe direction to fail in, and not one you want to
# discover after shipping.
export RATA_LICENCE_PUBLIC_KEY="$(cat /path/to/licence.pub)"
# Optional: Microsoft's public OAuth client id turns on Sign in with Microsoft.
# export RATA_MS_CLIENT_ID=...
# Optional: the Slack app's public client id turns on Share to Slack (Pro).
# export RATA_SLACK_CLIENT_ID=...
# Optional: Google's Desktop app client id and secret (both) turn on Google
# Drive (Pro). Google treats a desktop client's secret as not secret.
# export RATA_GOOGLE_CLIENT_ID=... RATA_GOOGLE_CLIENT_SECRET=...

# A packaged build, the one to test anything visible in. Without the
# custom-protocol feature a release build looks for a dev server instead of
# the interface compiled into it, and `tauri dev` differs from the shipped app
# in exactly the ways that ship bugs (the CSP, the asset protocol).
cargo build --release --features tauri/custom-protocol
cargo tauri build                   # installers: .deb/.AppImage, .dmg, .exe
```

`licence.pub` is the public half of the pair in
[`rata-next/LICENSING.md`](../../rata-next/LICENSING.md). It is not a
secret — it can only check signatures, not make them — so it belongs in the
build, in CI, and anywhere else convenient. The private half never leaves the
server.

Installers must be built on the platform they target — a Linux machine cannot
produce a .dmg or a Windows installer, let alone sign one. That is three build
machines, or
three CI runners.

## Releasing

`.github/workflows/release.yml` is the three build machines. **Bump the
version and merge to `main`**: when `main` carries a version that has not been
released, the workflow builds the installers into a draft, publishes it once
they are attached, and that creates the `v<version>` tag. Pushing a `v*` tag by
hand also works; running it by hand from the Actions tab builds installers to
try and releases nothing.

It builds four: Linux (.deb and .AppImage), macOS on Apple silicon, macOS on
Intel, and Windows (an NSIS `-setup.exe`). A 0.x version is published as a
pre-release, because it is a beta.

Linux builds on ubuntu-22.04 rather than the newest runner on purpose: an
AppImage linked against a newer glibc will not start on an older distribution,
and running anywhere is the only reason to ship an AppImage.

### Secrets it wants

| Secret | Without it |
|---|---|
| `RATA_LICENCE_PUBLIC_KEY` | **The build fails, deliberately.** An installer with no licence key refuses every licence, and that is the worst possible thing to hand somebody who has just paid. It is the public half and is not secret; it lives in Actions secrets so it cannot be changed by accident. |
| `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` | macOS shows *"RATA is damaged and can't be opened"* — Gatekeeper's words for an unsigned app, and indistinguishable from a broken download to the person reading it. Needs a paid Apple Developer account. |
| Six `AZURE_*` values (Artifact Signing), or `WINDOWS_CERTIFICATE` and `WINDOWS_CERTIFICATE_PASSWORD` | SmartScreen warns before the installer runs. Never both sets, and half a set fails the Windows job; see `docs/WINDOWS-SIGNING.md`. |
| `RATA_UPDATER_PUBKEY` and `TAURI_SIGNING_PRIVATE_KEY` (+ `_PASSWORD`) | The app registers no updater and never checks for a new version. Half the pair fails the build. |
| `RATA_MS_CLIENT_ID` (a repository **variable**, not a secret) | The build has no Sign in with Microsoft; Microsoft mailboxes cannot be added. |
| `RATA_SLACK_CLIENT_ID` (a repository **variable**, not a secret) | The build has no Share to Slack; Settings shows no Slack row and the page offers no Share to Slack. |
| `RATA_GOOGLE_CLIENT_ID` and `RATA_GOOGLE_CLIENT_SECRET` (both repository **variables**: Google treats a desktop client's secret as not secret) | The build has no Google Drive; Settings shows no Google Drive row. With only one of the two, the same. |

The build works without the signing secrets and the installers install; they
just arrive looking untrustworthy, which for a product whose pitch is "your mail
stays yours" is worth more than the certificates cost.

Signing is all-or-nothing, and set up that way after it bit: the Apple variables
are only put into the environment when a certificate is actually configured. An
unset secret becomes an empty string, an empty string is still a *set* variable,
and Tauri's bundler reads `APPLE_CERTIFICATE` with `std::env::var` — `Ok("")`
reads as "sign this", and it hands nothing to `security import`. Both macOS jobs
compiled cleanly and then died at bundling.

## CI

`.github/workflows/desktop-ci.yml`, in four jobs:

- **Mail layer** needs nothing installed and answers in about a minute.
- **Mail layer against real servers** starts Dovecot and GreenMail on the
  runner (`rata-mail/tests/loopback/servers.sh`) and runs the loopback tests
  with `--features loopback-tests`, after checking that feature cannot build
  for release.
- **Desktop shell** installs the system webview and runs `sync-ui.sh` first,
  which makes that script's anchor checks against `rata-next/public` part of
  CI rather than something that fails on a release machine.
- **Desktop interface, driven** runs `harness/ui-harness.mjs` against the
  assembled `ui/` under the app's own CSP, with a fake backend, and the update
  feed's rules (`harness/feed.test.cjs`).

The Rust jobs run `cargo fmt --check`, `cargo clippy --all-targets -- -D
warnings` and `cargo test --all-targets`. `release.yml` also installs and
launches each installer on its own runner before keeping it
(`harness/smoke-installed.sh`, `.ps1` on Windows).

The toolchain is pinned rather than `stable`. With `-D warnings`, an unpinned
toolchain means a new clippy lint reddens an unrelated pull request on the day
it ships, which teaches everyone to ignore the colour.

Some of the mail tests resolve real names — `anthropic.com` has no mail server
of its own and its MX is the case the whole discovery module exists for, so a
test that stubbed DNS would be testing the stub. They assert on the shape of the
answer rather than on a specific record.

## One interface, not two

`ui/` is generated and gitignored. `sync-ui.sh` copies `rata-next/public` and
applies exactly three differences, each of which fails loudly if its anchor
moves:

1. `bridge.js` is injected after `config.js`.
2. `config.js` is replaced with a device one — no Supabase, so `app.html` falls
   into the on-device account mode it already has.
3. The fonts are checked, not stripped. They used to be stripped, because a
   local-first mail app that contacts Google on every launch tells Google when
   its customer opens their mail — they are vendored now, in
   `rata-next/public/fonts`, so there is nothing to remove. A page that
   reintroduced a Google link would put that request back into an app with no
   network, where the content security policy would block it silently, so the
   script fails on one instead.

Run `sync-ui.sh` **before** `cargo build`, not after. Tauri compiles the web
assets into the binary, so building first produces an app carrying whatever was
in `ui/` last time — which looks exactly like a change that did not take.

The interface reaches the outside world through one function, `apiFetch`, and
`bridge.js` answers it. That is the entire desktop-specific frontend.

## The attack surface from the page

Thirty commands, the list in `main.rs`'s `generate_handler!`, and
nothing else. No filesystem plugin, no shell plugin, no arbitrary HTTP. The
notification and updater plugins are called from Rust only; the page holds no
plugin permission. A script that somehow reached a rendered message can ask to
refresh the mail; it cannot ask to read `~/.ssh`. The commands that write a
file (`save_attachment`, `save_file`) choose the Downloads folder and clean the
name in Rust, and `open_link` hands only http and https addresses to the
browser.

```
licence_status   set_licence      link_mailbox     link_microsoft
cancel_microsoft microsoft_ready  discover_mailbox list_mailboxes
unlink_mailbox   forget_everything refresh_mail    send_mail
save_draft       change_messages  older_mail       reread_mail
list_folders     folder_mail      notify_mail      watching
open_message     save_attachment  open_link        save_file
read_attachment  read_pictures    search_mail      diagnostics
check_update     install_update
```

Verified rather than assumed: `ui-src/probe.html` calls a few of them and also
asks for a command that does not exist. Copy it over `ui/index.html`, run the app,
and read the answers — the last one should say `Command read_file not found`.

## The licence

Signed, not looked up. The app reads mail straight from the customer's mail
server, so making it ask us for permission would mean their mail stops working
when we do — and would be useless on a train. Instead the server issues a small
Ed25519-signed token and the app checks it locally, with no network.

`src-tauri/src/licence.rs` is the other half of `rata-next/lib/licence.js`, and there is a
test here that verifies a token produced by that file. Two implementations of
one signature check that disagree is precisely the bug that locks paying
customers out, and neither codebase's own tests would find it.

**What a licence gates.** Linking a mailbox, refreshing, and sending. There is
no free tier — someone without a subscription is not on a cheaper plan, they are
unsubscribed — so all three stop. What does *not* stop is reading what is
already on the machine: the account list, the stored passwords and the
downloaded mail all stay exactly where they are, because they are theirs.

**Thirty days offline**, then it renews itself. The old token is the credential
for renewal — it is signed with a key only the server holds, so presenting one
proves where it came from, and an expired one is accepted because renewing an
expired licence is the whole job. The request is made from `bridge.js` rather
than from Rust, and `https://mailrata.org` is the one outside address in the
content security policy's `connect-src`. In Rust, only the updater (GitHub's
release page, `update.rs`) and Sign in with Microsoft (Microsoft's token
endpoint, `oauth.rs`) make HTTP requests.

So a cancelled subscription keeps working for up to a month. That is the
deliberate trade, and `LICENCE_DAYS` is the one place to change it.

## What is not done yet

- **Two ways to sign in to a mailbox.** An IMAP mailbox links with its
  address and an app password. Microsoft mailboxes (Outlook.com, Hotmail, Live,
  Microsoft 365) use **Sign in with Microsoft** instead (`oauth.rs`: OAuth 2.0
  code with PKCE, the redirect caught by a one-shot listener on `127.0.0.1`, so
  no server is needed), and only in a build carrying `RATA_MS_CLIENT_ID`; it
  has not yet been tried against a real Microsoft sign-in. Google sign-in does
  not exist: Gmail links with an app password.
- **Installers are not signed** until the owner adds the certificates. The
  release workflow builds all four; Windows and macOS warn on first launch.
- **No customer's mailbox has been opened.** The engine runs against real
  Dovecot and GreenMail in CI, but not against Gmail, Outlook or any hosted
  provider (`docs/SMOKE.md` is that checklist). Renewal has not been exercised
  against a live server either: the endpoint has tests, but no app has renewed
  against a deployed `mailrata.org`.
- **The interface has no licence screen of its own.** `bridge.js` puts up a box
  asking for the key when there is no usable one. That is desktop-only on
  purpose — the website has no key to type, and giving `app.html` a field that
  appears in one build of two is how one interface becomes two — but it is a
  plain box rather than a designed screen.
