# RATA for the desktop

The app that replaced the server.

RATA used to read everybody's mail from a VPS, which meant holding everybody's
mail password. That is a thing worth attacking, and it is a thing worth
attacking *once* — one break, every customer. This app holds nobody's: it runs
on the machine that owns the mailbox, the passwords are in that machine's own
credential store, and the only thing left on a server is the landing page, the
payment link and a signature.

```
desktop/
  rata-mail/          the mail layer — DNS, IMAP, SMTP, the outbound guard
  rata-app/
    src-tauri/        the shell: vault, store, core, commands
    ui-src/           bridge.js — the seam between the interface and Rust
    ui/               assembled at build time, never committed
```

## Where things live on a customer's machine

| What | Where | Why there |
|---|---|---|
| Mail passwords | The OS credential store — Keychain, Credential Manager, Secret Service | Encrypted at rest, tied to the login session, and already trusted with these same passwords by every mail client they have used |
| Mailbox list | `mailboxes.json` in the app's data directory | Metadata only. There is a test asserting no password ever reaches it |
| Messages | The webview's own storage, as on the web | Unchanged from the website |

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
cargo build --release
cargo tauri build                   # installers: .deb/.AppImage, .dmg, .exe
```

`licence.pub` is the public half of the pair in `LICENSING.md`. It is not a
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
| A Windows code-signing certificate | SmartScreen warns before the installer runs. Setting one up is in Tauri's Windows signing documentation. |

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

`.github/workflows/desktop-ci.yml`, in two jobs. **Mail layer** needs nothing
installed and answers in about a minute; **Desktop shell** installs the system
webview and runs `sync-ui.sh` first, which makes that script's three anchor
checks against `rata-next/public` part of CI rather than something that fails on
a release machine. Both run `cargo fmt --check`, `cargo clippy -- -D warnings`
and `cargo test`.

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

Six commands, and nothing else. No filesystem plugin, no shell plugin, no
arbitrary HTTP. A script that somehow reached a rendered message can ask to
refresh the mail; it cannot ask to read `~/.ssh`.

```
licence_status  set_licence  link_mailbox  list_mailboxes
unlink_mailbox  retry_mailbox  refresh_mail  send_mail
```

Verified rather than assumed: `ui-src/probe.html` calls each one and also asks
for a command that does not exist. Copy it over `ui/index.html`, run the app,
and read the answers — the last one should say `Command read_file not found`.

## The licence

Signed, not looked up. The app reads mail straight from the customer's mail
server, so making it ask us for permission would mean their mail stops working
when we do — and would be useless on a train. Instead the server issues a small
Ed25519-signed token and the app checks it locally, with no network.

`src/licence.rs` is the other half of `rata-next/lib/licence.js`, and there is a
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
than from Rust, so the app needs no HTTP client and the single address it may
contact is one line of the content security policy.

So a cancelled subscription keeps working for up to a month. That is the
deliberate trade, and `LICENCE_DAYS` is the one place to change it.

## What is not done yet

- **Mail is IMAP with an app password, and nothing else.** There is no OAuth
  sign-in (it needs a server to receive the redirect), and the interface no
  longer offers one. Every IMAP mailbox, Gmail and Outlook included, links with
  an app password.
- **Installers are not signed.** The release workflow builds all four; Windows
  and macOS warn on first launch.
- **No mailbox has been opened for real.** The container this was written in
  blocks 993, 465 and 587, so every network path is tested up to the socket and
  no further. Renewal has not been exercised against a live server either: the
  endpoint has tests, but no app has renewed against a deployed `mailrata.org`.
- **The interface has no licence screen of its own.** `bridge.js` puts up a box
  asking for the key when there is no usable one. That is desktop-only on
  purpose — the website has no key to type, and giving `app.html` a field that
  appears in one build of two is how one interface becomes two — but it is a
  plain box rather than a designed screen.
