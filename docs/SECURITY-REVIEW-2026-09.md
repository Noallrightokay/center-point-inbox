# Security review, September 2026 (F4 part 1)

Reviewer: a fresh agent session with no part in writing the code.
Scope: what is on `main` at `f123731` (after PR #66). OAuth sign-in (card C2)
is reviewed separately when its PR opens and is not covered here.

Threat model, from `CLAUDE.md`: a mail password never reaches a server; mail
never reaches a server except text the customer sends to the AI relay; a
formatted message is shown in a frame that can run nothing and reach nothing;
the outbound guard stops a hostile DNS answer or a typed address reaching the
customer's own network.

**Result: no High finding.** Four Medium, eleven Low. Every Medium below has
a proposed patch; none is fixed here, as the card asks. SEC-1 fixed findings
1, 3, 10 and 13 and the `past_due` mismatch; its rows say so. SEC-4 fixed
6, 7, 11 and 14, the SMTP half of 8 and part of 15. The invariants in
Part 4 of `docs/MVP-PLAN.md` all hold on `main` today.

## Findings

| # | Area | Finding | Severity | Evidence | Proposed fix |
|---|---|---|---|---|---|
| 1 | HTML mail (`html.rs`) | One inline picture referenced many times is copied into the page once per reference, while the 4 MiB budget is charged once. A small message can make RATA build a string of gigabytes and abort when opened. | Medium | 3.4 KB of HTML plus one 1 MiB PNG gave 280 MB of output in 11 s (release build); growth is linear, so ~34 KB of HTML gives ~2.8 GB. | **Fixed in this PR (SEC-1).** Every reference is charged; a picture that does not fit in full is left out and the text stays. Test: `a_picture_referenced_many_times_is_charged_each_time`. |
| 2 | Saving files (`core.rs`) | Saved attachments and converted files carry no Mark of the Web (Windows) or quarantine flag (macOS), and executables are saved with no warning. SmartScreen, Office Protected View and Gatekeeper then never look at them. | Medium | `write_new` only opens and writes; no `Zone.Identifier` or `com.apple.quarantine` anywhere in `desktop/`. | **Fixed in PR #71 (SEC-3); the disguised-program question is PR #75 (UI-1).** `write_new` marks every file it saves (attachments and Bridge downloads) through `mark.rs`: `Zone.Identifier` `ZoneId=3` on Windows, `com.apple.quarantine` `0081;<hex time>;RATA;` on macOS, nothing on Linux; no referrer or host, so the sender is never recorded, and a failed mark never fails the save. Tests: `the_windows_stream_says_internet_and_nothing_else`, `the_macos_attribute_is_flags_time_agent`, `on_linux_marking_touches_nothing`. Not yet seen on a real Windows or Mac. The owner chose to ask about disguised programs only: `invoice.pdf.exe` is labelled and asked about before it is saved, and Rust refuses to save or convert it without the answer (`needs-confirmation`); `setup.exe` saves without asking. |
| 3 | Stripe webhook | A checkout keyed by email overwrites an existing customer's row, including `stripe_customer` and `plan`. Anyone can pay once with a paying customer's address, downgrade them at once, then cancel and take their entitlement away; the victim's own subscription events then match no row. | Medium | `app/api/stripe/webhook/route.js` email branch: `upsert` on `email` with the event's `stripe_customer`, no check that the held row belongs to another live customer. | **Fixed in this PR (SEC-1).** A checkout for an address live under another customer is answered 200 with `conflict: true`, logged without addresses, and writes nothing (`checkoutConflict` in `lib/stripe.js`). |
| 4 | Licence issue | Licences are issued to the signed-in address, and sign-ups are auto-confirmed, so whoever registers an address first holds its subscription. Already written up in `rata-next/LAUNCH.md` section 7. | Medium (known) | `app/api/licence/route.js` issues to `user.email`; LAUNCH.md 7 records `mailer_autoconfirm` on. | Owner: configure an SMTP sender in GoTrue, then turn auto-confirm off, before paid launch. Code: also refuse when `email_confirmed_at` is empty. |
| 5 | Outbound guard | The `loopback-tests` gate is tied to `debug_assertions`, not to the release profile. A release build with debug assertions forced on compiles with the loopback exception, in the engine and in the app. | Low | `CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS=true cargo check --release --features rata-mail/loopback-tests` in `rata-app/src-tauri` finishes; without the variable it fails as intended. `release.yml` sets neither. | Make the app refuse to compile with the feature in any profile (patch in [section 3](#3-outbound-guard-and-the-loopback-gate)). |
| 6 | Window navigation (`links.rs`) | `navigation` keeps the window on any `tauri://` host, `http(s)://tauri.localhost` on any port and every OS, any `blob:` origin and any `about:` URL. Wider than the app needs. | Low | Probe: `tauri://evil.example/x`, `http://tauri.localhost:8080/`, `blob:https://evil.example/1` all return `Stay`. Only a script in the app's own page can navigate the window. | **Fixed (SEC-4).** Exactly where the platform serves the app: `tauri://localhost` on macOS and Linux, `http(s)://tauri.localhost` on Windows, no port but the default and no user name; plus `about:blank` and `about:srcdoc`. `blob:` is refused from every origin (the app saves what it makes through `save_file` and never navigates to a blob). Test: `the_app_is_only_where_its_platform_serves_it`. |
| 7 | HTML mail (`html.rs`) | The `background` attribute is not treated as a URL by ammonia, so its scheme and relative-URL rules do not apply: `background="javascript:..."` and `background="/x"` pass through. The frame's CSP stops both. | Low | Sanitiser probe output keeps `<table background="javascript:alert(1)">`; Chromium then refused it under `img-src data:`. | **Fixed (SEC-4).** `attribute_filter` keeps `background` only for an `https://` picture or a carried one (`data:image/{png,jpeg,gif,webp};base64,`, which is what a `cid:` becomes); anything else loses the attribute. Test: `a_background_attribute_keeps_only_a_picture_address`. |
| 8 | Engine errors (`imap.rs`, `smtp.rs`, `credential.rs`) | Sign-in failures keep the server's NO/BAD and SMTP reply text verbatim; `redact` covers only an exact echo of an OAuth token or its XOAUTH2 base64, never a password or the `AUTH PLAIN` base64, and a truncated echo defeats it. SMTP reads a reply line with no length cap. No path formats message text. | Low | `login`: `Trouble::Auth(why)` raw; `smtp::hear` uses `read_line` unbounded and puts `{line}` in errors. | **SMTP fixed (SEC-4); IMAP open.** `smtp::hear` reads a line up to 1 KiB (`LINE_MAX`) and refuses a longer one, and checks the reply code as bytes (a multibyte character there used to panic). Every server sentence in an SMTP error goes through `said`: one line, no control or bidi-control characters, at most 200. While signing in, the password, its base64, the `AUTH PLAIN` string, the token and its XOAUTH2 string are taken out wherever they appear (any case), then every run of 16+ base64 characters, all before the line is cut. Tests: `a_reply_line_longer_than_rata_reads_is_refused_not_kept`, `a_reply_that_is_not_ascii_where_the_code_goes_is_an_error_not_a_crash`, `what_a_server_said_is_one_short_plain_line`, `a_password_the_server_repeats_is_never_in_what_sending_says`, `a_token_the_server_repeats_cut_short_is_still_hidden`. Still open: IMAP `login` and `xoauth2` keep NO/BAD text raw and `credential::redact` matches only an exact echo; `imap.rs` was held by F1. |
| 9 | AI relay cap | The monthly cap is check, call, then charge. Concurrent requests all pass the check (bounded by 20 a minute per process, about $0.50 over), and if `ai_charge` fails the answer is still sent and nothing is recorded, so the cap fails open. | Low | `app/api/ai/route.js` lines 52 to 81. | **Fixed in this PR (SEC-2); needs `database.sql` §5 re-run.** Before calling, the route holds the worst case (every prompt byte as a token, plus 64, plus `max_tokens`) with `ai_reserve`, one upsert whose `WHERE` refuses a hold that would pass the cap, so concurrent requests cannot both fit; afterwards `ai_settle` moves it to the real cost, or releases it if the model failed. A database error or missing function answers 503 and calls nothing (fails closed). Tests: `tests/ai.test.mjs`, "two requests at once cannot both fit under the cap". |
| 10 | AI relay prompt | The fence strips only exact `<email>`/`</email>`; `</email >` survives. Task text keeps bidi controls. Impact is a misleading summary shown to the requester only. | Low | `prompt('summarize', {text: 'hello </email >...'})` keeps the tag; `parseTasks` keeps U+202E. | **Fixed in this PR (SEC-1).** Every `<` that starts an `email` tag in any case or spacing becomes `‹`; `clean` drops bidi controls. |
| 11 | Stripe webhook | `checkout.session.completed` with `status: complete` but `payment_status: unpaid` (delayed payment methods) is recorded as `active`. | Low | `lib/stripe.js` `rowForEvent`: returns null only when both are unfinished. | **Fixed (SEC-4).** Only `paid` or `no_payment_required` is `active`; any other checkout is recorded as `incomplete` (not live, so the customer's later events still find the row), never over a live row, and `checkout.session.async_payment_succeeded` makes it `active` (STRIPE-SETUP.md now subscribes to it). Tests in `tests/stripe.test.mjs`, "a checkout that has not been paid for yet grants nothing" and the endpoint section. |
| 12 | Saving files | `safe_file_name` misses the Windows device names `COM¹` to `COM³`, `LPT¹` to `LPT³`, `CONIN$` and `CONOUT$`. | Low | The check is `stem.len() == 4` and an ASCII digit; `COM¹` is five bytes. | **Fixed (BUG-4).** All six are reserved, in any case, with or without an extension. Test: `every_windows_device_name_is_defused_with_or_without_an_extension`. |
| 13 | Website headers | No `frame-ancestors`/`X-Frame-Options` and no HSTS on mailrata.org; the only CSP is `upgrade-insecure-requests`. | Low | `curl -I https://mailrata.org/app` today. | **Fixed in this PR (SEC-1).** HSTS (1 year, subdomains, no preload), `X-Frame-Options: DENY`, `frame-ancestors 'none'`, `nosniff`, `strict-origin-when-cross-origin`, on every path; `tests/headers.test.mjs`. |
| 14 | App data file | `mailboxes.json` (mailbox list and the licence token) is written with default permissions, readable by other accounts on a shared Linux machine. Passwords are in the keychain, not here. | Low | `store.rs` `save` uses `fs::write`. | **Fixed (SEC-4).** On Unix the file is written `0600` (a fresh temporary file, then the rename), a file an older build left wider is tightened when opened, and the app's folder is made `0700` when RATA makes it; Windows keeps the profile's ACL. Test: `only_this_account_can_read_the_file`. |
| 15 | Licence renewal | An expired licence renews however old it is, and nothing can revoke a leaked one, so one shared token keeps any number of copies licensed while the subscription lives. | Low | `app/api/licence/renew/route.js`. Signature, subscription and address binding are all checked correctly. | **Partly fixed (SEC-4).** A licence expired more than `RENEW_GRACE_DAYS` (90) ago is refused before the database, `reason: 'too-old'`, "Sign in at mailrata.org to get a new one" (`renewable` in `lib/licence.js`). A copy that keeps renewing inside the window is still not stopped; revocation or counting devices remains a business decision. Tests in `tests/licence.test.mjs`, both sides of the boundary. |
| - | Sanitiser, sandbox, frame CSP | Script, SVG and MathML script, mXSS through `<style>`/`<noscript>`, forms, frames, objects, `<base>`, `<meta>` refresh, `<link>`, `javascript:`/`data:`/`blob:`/`file:`/`tauri:` links, `target`, `ping`, non-image `cid:` parts. | None | Sanitiser probe (section 1); frame probe in Chromium with the real `frameDoc`: nothing reached the network with pictures off; with them on, only `https:` pictures, never `@import` or fonts. | - |
| - | Links | `javascript:`, `file:`, `data:`, user@host, IDN, newlines, tabs, backslashes, U+2028, `mailto:` header smuggling. | None | Probe in section 2. | - |
| - | Guard and discovery | IPv4 ranges, every IPv6 spelling of a private address, private names, SRV/MX/typed hosts, DNS rebinding. | None | Section 3. | - |
| - | Licence verification (app and site) | Forgery, key confusion, expiry, plan parsing, renewing another address. | None | Section 6. | - |
| - | Stripe signature, account deletion, updater, CSP exemption, logs | | None | Sections 8 and 9. | - |

## 1. HTML mail: sanitiser and frame

**What it protects.** A formatted message is a stranger's HTML. It must not
run anything, submit anything, navigate anything or fetch anything the
customer did not ask for, and nothing in it may reach the app page, which
can call every Tauri command.

**What I tried.** A throwaway test fed `html::safe` forty hostile inputs
(not committed). Everything that runs or submits was removed:
`<script>`, `<img onerror>`, `<svg onload>`, `<svg><script>`, the MathML and
`<style>` mXSS shape, `<noscript>` breakout, `<form>`, `<textarea>`,
`<select>`, `<template>`, `<iframe>`, `<object>`, `<embed>`, `<base>`,
`<meta http-equiv=refresh>`, `<link rel=stylesheet>`. Links keep only
http, https and mailto: `javascript:` (also with a leading space and with an
encoded tab), `data:`, `blob:`, `file:`, `tauri:` and `https:evil` without
slashes all lost their `href`; `target`, `ping` and `formaction` were
dropped. `<img src="blob:...">` lost its `src`. A `cid:` part that is
`text/html` or `image/svg+xml` is not inlined. A Content-ID containing
quotes cannot break out, because the replacement happens before the
sanitiser runs. ammonia is 4.2.0, after the 4.1.2 fix for RUSTSEC-2025-0071.

Two things pass the sanitiser and are stopped by the frame instead:
`<style>` blocks are kept whole (`@import url(https://...)`,
`background:url(https://...)`, `@font-face`), and the `background`
attribute keeps any value (finding 7). So I checked the frame for real: a
throwaway Playwright script took `frameDoc` out of `app.html`, built the
iframe exactly as `openMail` does (`sandbox="allow-popups"`,
`referrerpolicy="no-referrer"`, `srcdoc`), and loaded the sanitiser's output
with a route handler counting what reached the network (not request events,
per the CLAUDE.md trap).

- Pictures off: nothing reached the network. `javascript:` in `background`
  was refused by `img-src data:`.
- Pictures on (Load images): only `https:` pictures loaded (an `<img>`, a
  `<style>` background, an inline-style background, a `background=`
  attribute). `@import` and `@font-face` were still refused, and a
  protocol-relative `url(//...)` resolved to the app's own scheme and was
  refused.
- A click on a link produced a new-window request and the top page did not
  move.
- `app.html` has exactly one `sandbox=` attribute and it is exactly
  `allow-popups`. `srcdoc` escapes `&` and `"`, which is all a double-quoted
  attribute needs.

With pictures on, CSS in the message can still choose which https pictures
load based on its own markup. It can learn nothing the sender did not
already write, so I did not count it.

**Finding 1, Medium: one picture, many references.** `safe` replaces every
`cid:<id>` in the HTML with a `data:` URL, but charges `data.len()` against
`INLINE_ALL` once per part. 200 references to one 1 MiB PNG (3.4 KB of HTML)
produced 280 MB of output in 11 s in a release build; 50 references gave
70 MB. The output then crosses IPC and becomes an `srcdoc`. A message of
about 1.4 MB (the picture plus ~34 KB of HTML) asks for ~2.8 GB, and Rust
aborts the process when an allocation fails. It needs one click (open,
forward, or Continue on a draft), and the message stays in the list, so the
customer can crash RATA again by opening it again.

Proposed patch (`desktop/rata-mail/src/html.rs`, in `safe`):

```rust
for (cid, mime, data) in inline {
    let image = matches!(mime.as_str(), "image/png" | "image/jpeg" | "image/gif" | "image/webp");
    let reference = format!("cid:{cid}");
    // Every reference is a copy, so every reference is charged.
    let uses = html.matches(&reference).count();
    let cost = data.len().saturating_mul(uses);
    if !image || uses == 0 || data.len() > INLINE_ONE || cost > budget {
        continue;
    }
    budget -= cost;
    let url = format!("data:{mime};base64,{}", crate::words::base64_encode(data));
    html = html.replace(&reference, &url);
}
```

Test first: `a_picture_referenced_many_times_is_charged_each_time`, with one
1 MiB PNG referenced 200 times, asserting the output is smaller than
`INLINE_ALL * 4 / 3 + html.len() + 1024`. It fails today (280 MB).

**Finding 7, Low.** Add to `attribute_filter`:

```rust
if attribute == "background" {
    let v = value.trim_start().to_ascii_lowercase();
    let ok = v.starts_with("https://") || v.starts_with("http://") || v.starts_with("data:image/");
    return ok.then_some(value.into());
}
```

## 2. Links and window navigation

**What it protects.** No outside page may ever load in the window that holds
the bridge, and a link only ever opens in the customer's browser, after the
customer has seen where it really goes.

**What I tried.** A throwaway test in `links.rs` (reverted) ran `classify`
and `navigation` on edge cases:

- `https://exa\nmple.com/a\r\nb` and `https://exa\tmple.com/` become
  `example.com`: the WHATWG parser drops tabs and newlines, and the dialog
  shows the parsed host, so what is shown is what opens.
- `https:\\evil.example\x` is `evil.example`, shown as such.
- `https://mailrata.org\@evil.example/` is `mailrata.org` with path
  `/@evil.example/`; `https://evil.example#@mailrata.org` is `evil.example`.
  Both are shown truthfully. `https://evil.example%2f@mailrata.org/` and
  every user@host form are refused.
- `https://аpple.com/` (Cyrillic а) becomes `xn--pple-43d.com`, so the
  dialog shows the punycode.
- U+2028 in a host is refused; ` javascript:`, `java\nscript:`, `file:`,
  `data:`, custom schemes are refused.
- `http://127.0.0.1:8080/` and `https://[::1]/` are accepted as web links.
  That is what any mail client does; the dialog names the host.
- `from_mail` passes the result to the page as a `serde_json` literal, so
  nothing in a URL becomes code. `mailto:` subjects lose control characters
  and the address is checked.
- `open_link` re-classifies whatever the page asks, so the page can only
  ever open an http(s) URL. A script in the page could skip the dialog; the
  dialog is a courtesy for the customer, not a lock.

**Finding 6, Low.** `navigation` answers `Stay` for any `tauri://` host, for
`http(s)://tauri.localhost` on any port and on every OS, for `blob:` of any
origin and for any `about:` URL. Only a script in the app page can navigate
the top window (the frame has no `allow-top-navigation` and no scripts), so
this matters only after a script got in. On Linux and macOS
`http://tauri.localhost:8080` is a real request to loopback; Tauri's own
origin check (`is_local_url`, tauri 2.12.0) would refuse it IPC, but
`from_mail` and the watch would still `eval` into it. Proposed patch:

```rust
pub fn navigation(url: &Url) -> Navigation {
    let app = |u: &Url| {
        if cfg!(windows) {
            matches!(u.scheme(), "http" | "https") && u.host_str() == Some("tauri.localhost") && u.port().is_none()
        } else {
            u.scheme() == "tauri" && u.host_str() == Some("localhost")
        }
    };
    if app(url) || matches!(url.as_str(), "about:blank" | "about:srcdoc") {
        return Navigation::Stay;
    }
    if url.scheme() == "blob" {
        // blob:<origin>/<uuid>: only the app's own
        return match Url::parse(url.path()) {
            Ok(inner) if app(&inner) => Navigation::Stay,
            _ => Navigation::Refuse,
        };
    }
    /* mailrata.org arm and Refuse as today */
}
```

The existing test `the_window_stays_on_the_app` would need its
`http://tauri.localhost` cases split by OS.

## 3. Outbound guard and the loopback gate

**What it protects.** A socket never opens to loopback, the LAN, link-local
(cloud metadata) or any other non-public address, whatever DNS or the
customer's typed server says.

**What I tried.**

- `guard.rs` parses every address to bytes before judging, so
  `::ffff:7f00:1`, `::7f00:1`, `64:ff9b::/32` (both NAT64 prefixes),
  `2002::/16` (6to4) and the IPv4 ranges (loopback, RFC 1918, CGNAT,
  link-local, 0/8, 240/4, benchmarking, documentation) are all refused; its
  tests cover them. `check_resolved` refuses a name if any answer is
  private. It has no loopback exception at all: the exception is only in
  `check_literal`, and only for the literal `127.0.0.1`.
- Every socket in the engine's non-test code opens in `imap::dial`, which
  IMAP, the IDLE watch and SMTP all call with the addresses `resolve_public`
  returned, so there is one lookup and no rebinding window.
  SRV targets, MX-derived hosts, conventional guesses and a typed server all
  go through it. A typed `imaps://host:993/` is tidied to `host` and then
  judged like any other. Names that are numeric but not strict IPs
  (`127.1`, `0x7f.1`) do not parse as literals, go to the resolver, and
  whatever they resolve to is judged.
- SMTP always has TLS before AUTH: implicit on 465, and STARTTLS on 587 is
  mandatory, with unread bytes before the handshake refused (the STARTTLS
  injection case).

**The feature gate.** `cargo check --release --features loopback-tests` in
`desktop/rata-mail` fails with the `compile_error!`, and so does
`cargo check --release --features rata-mail/loopback-tests` in
`desktop/rata-app/src-tauri` (Cargo lets a package turn on a dependency's
feature that way; the app's own `Cargo.toml` never names it). Both **build**
when `CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS=true` is set, and so would a
`[profile.release] debug-assertions = true` or a custom profile inheriting
from dev. `release.yml` sets none of these, so no shipped build has it
today (finding 5, Low). The exception is narrow even then: only the literal
`127.0.0.1` typed as the server. Proposed patch:

```rust
// desktop/rata-mail/src/guard.rs
/// True only in a build made for the loopback tests.
pub const LOOPBACK_TESTS: bool = cfg!(feature = "loopback-tests");

// desktop/rata-app/src-tauri/src/main.rs
const _: () = assert!(
    !rata_mail::guard::LOOPBACK_TESTS,
    "the app must never be built with rata-mail's loopback-tests feature"
);
```

The existing `compile_error!` stays; the assertion adds a lock that does not
depend on the profile. A test that the app refuses the feature is the build
itself: `cargo check --features rata-mail/loopback-tests` in the app must
fail in every profile.

## 4. The bridge and saving files

**What it protects.** The page can call Rust only through the routes in
`bridge.js` and the commands in `main.rs`. If a script ever ran in the page
(it cannot today: the frame is sandboxed, and the app CSP's `script-src`
carries Tauri's hashes and nonce, which make `'unsafe-inline'` ignored for
injected handlers; I did not re-check that in a packaged build), this is
what it could do.

| Route (bridge.js) | Command | Takes from the page | A hostile page could |
|---|---|---|---|
| `/api/link/mail` POST | `link_mailbox` | email, password, **host** | Link a mailbox at any public host, through the guard. Relinking an existing address to its own server would route later sends there. |
| `/api/link/mail` DELETE | `unlink_mailbox` | email | Remove a mailbox and its keychain password. |
| `/api/sync/mail` | `refresh_mail`, `list_mailboxes` | limit, known, only | Refresh; read what RATA already shows. |
| `/api/mail/watching` | `watching` | nothing | List addresses. |
| `/api/mail/act` | `change_messages` | email, folder, uids, uidvalidity, action | Trash, archive or move mail (never expunge). |
| `/api/mail/older`, `/api/mail/read`, `/api/mail/folder`, `/api/mail/folders` | `older_mail`, `reread_mail`, `folder_mail`, `list_folders` | email, folder, uids | Read mail. A named folder must be in the live LIST. |
| `/api/mail/open` | `open_message` | email, folder, uid | Read one message whole. |
| `/api/mail/attachment` | `save_attachment` | email, folder, uid, index | Save a real attachment into Downloads. |
| `/api/mail/attachment/read` | `read_attachment` | same | Read attachment bytes. |
| `/api/file/save` | `save_file` | name, base64 data | Write any bytes, under a cleaned name, into Downloads. |
| `/api/send/mail` | `send_mail` | the draft, forward reference | Send as the customer, with attachments forwarded from their mailbox. The one real exfiltration path. |
| `/api/ai` | `licence_status`, then `fetch` | the request body | Send text to the relay under the customer's licence. |
| `/api/link/open` | `open_link` | url | Open any http(s) URL, skipping the dialog. |
| `/api/notify` | `notify_mail` | title, body | Show a notification (made plain in Rust). |
| `/api/update/check`, `/api/update/install` | `check_update`, `install_update` | nothing | Install only a signed, newer release. |
| `/api/licence` | `licence_status`, `set_licence` | token or null | Read the licence token; clear it. |
| `/api/links`, `/api/account` | `licence_status` / none | nothing | Nothing. |

No route takes a path, a raw command, or a URL other than `open_link`'s
(re-checked in Rust). The one host is `link_mailbox`'s, which goes through
discovery and the guard. There are no plugin permissions (no
`capabilities/` directory), so the notification and updater plugins cannot
be called from JavaScript. The mail frame cannot reach any of this.

**Names.** `safe_file_name` takes the last path part after `/` and `\`,
drops control, bidi-override and `< > : " | ? *` characters, trims dots and
spaces, prefixes `CON`, `PRN`, `AUX`, `NUL`, `COMn`, `LPTn` (also with an
extension or trailing spaces), and cuts to 150 bytes keeping an extension
of up to 12. `../../.bashrc` gives `bashrc`; `invoice\u{202e}fdp.exe` gives
`invoicefdp.exe`. `write_new` uses `create_new`, so it never follows or
replaces an existing file or symlink. Finding 12 lists the device names it
misses.

**Finding 2, Medium: no Mark of the Web.** Browsers and Outlook tag every
download from the internet, and Windows and macOS protect the customer only
for tagged files: SmartScreen checks an `.exe`, Office opens a document in
Protected View and blocks its macros, Gatekeeper checks an app. RATA's
files are untagged, so an attachment called `invoice.pdf.exe` (Explorer
hides `.exe` by default) or a `.docm` runs with none of those checks once
the customer double-clicks it in Downloads, which is what people do with an
attachment. Proposed patch, in `write_new` after `write_all` succeeds:

```rust
mark_from_internet(&path);

/// Tell the operating system this file came from the internet, as a browser
/// does, so its own checks (SmartScreen, Protected View, Gatekeeper) apply.
fn mark_from_internet(path: &Path) {
    #[cfg(windows)]
    {
        let mut ads = path.as_os_str().to_owned();
        ads.push(":Zone.Identifier");
        let _ = std::fs::write(ads, "[ZoneTransfer]\r\nZoneId=3\r\n");
    }
    #[cfg(target_os = "macos")]
    {
        let value = format!("0081;{:x};RATA;", now());
        let _ = xattr::set(path, "com.apple.quarantine", value.as_bytes()); // xattr crate
    }
}
```

Test first: on Windows CI, save a file and assert the `Zone.Identifier`
stream reads `ZoneId=3`. Separately, the interface could ask before saving
an extension on a short list (`exe scr com bat cmd ps1 vbs js jse wsf hta
lnk msi msix jar app dmg pkg`), which is a UI decision for the owner.

**Finding 12, Low.** In `safe_file_name`:

```rust
let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$")
    || ((stem.starts_with("COM") || stem.starts_with("LPT"))
        && matches!(&stem[3..], "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"));
```

## 5. Engine error text

**What it protects.** Error sentences are shown to the customer, kept as a
mailbox's last error and pasted into bug reports, so they must never carry
a message's contents or a credential.

**What I tried.** Read every `format!`, `to_string()` and `{:?}` on an error
in `rata-mail/src` and `rata-app/src-tauri/src` outside tests. async-imap's
unparsable-reply error quotes the whole buffer (`"{:?} during parsing of
{:?}"`, `imap_stream.rs` line 126); `reason` and `io_reason` replace that
with a fixed sentence, and every async-imap error in `imap.rs` goes through
`reason`, `io_reason`, or is a NO/BAD tagged line. SMTP errors carry the
server's reply text, never the message. **No path formats message text.**
What stays is the server's own words (finding 8, Low):

- `reason` keeps NO/BAD text through `one_line` (200 characters), while
  `login` and `xoauth2` keep NO/BAD text raw. The commit message of #66 says
  "never the server's reply"; for NO/BAD that is not what the code does, and
  it should not claim it.
- `credential::redact` removes an exact echo of the access token and of the
  XOAUTH2 base64. A server that echoes only part of the line (for example
  "line too long: AUTHENTICATE XOAUTH2 dXNlcj1...") defeats it, and a
  password or the `AUTH PLAIN` base64 is never redacted by design.
- `smtp::hear` reads each line with `read_line`, which has no cap; the 8 KB
  limit applies only between lines. IMAP is capped by async-imap at 512 MiB.

Proposed patch: `one_line` every `why` in `login`, `xoauth2`, `hear` and
`starttls`; in `redact`, also replace any run of 16 or more base64
characters in sign-in errors with `[hidden]`; read SMTP lines through
`(&mut io).take(8192).read_line(...)` and fail if no newline arrived.

## 6. Licences

**What it protects.** Only RATA's server can make a licence; a licence says
who paid and for what; renewal gives a licence only to the address that
holds a live subscription.

**What I tried.**

- `licence.rs`: the signature is checked with `verify_strict` over the
  payload as written, before any field is read; a missing or wrong-shape key
  refuses everything (`NoPublicKey`); an RSA key is refused rather than
  misread; the clock is checked last so a forgery is never "expired"; `v`,
  `sub`, `plan` and `exp` are required. The tests cover a token made by
  `licence.js` and by an openssl key. An unknown plan name only ever gets
  what `plan_def` gives unknown plans. Setting the clock back keeps an
  expired licence working offline; that is the offline design.
- `licence.js` `check`: the payload is returned for an expired token only
  after the signature verified, which is what renewal relies on.
- `renew/route.js`: a forged token is refused before any database query;
  the address renewed is the one inside the token; the plan comes from the
  `subscriptions` row, not from the token. A token for one address cannot
  renew another. Finding 15 is the only gap, and it is a business choice.
- `cors.js`: exactly the three app origins, `Vary: Origin`, no credentials.
  CORS is not authentication here, and nothing relies on it being.
- `/api/licence` (issue) uses the Supabase session checked server-side, and
  never an address from the body. Finding 4 is that the session's address
  was never proven.

## 7. The AI relay

**What it protects.** Mail text passes through and is never kept; only paying
customers on the right plan use it; RATA's cost per customer is capped;
text from a stranger cannot steer what the customer is shown.

**What I tried.**

- Licence: `check` must return `ok`, so expired and forged tokens are
  refused; the page's body cannot override the licence (bridge.js sets it
  last).
- Plan gate: `validate` checks the task against `planDef(plan)`; an unknown
  plan is `NO_PLAN`; `__proto__` or `constructor` as a plan find no `ai`
  flag.
- Logs: four `console.warn` lines, each a status, an error name or a
  database error message; no text, no address. Nothing else logs.
- Accounting: `ai_charge` is one atomic upsert, and both functions are
  revoked from `anon` and `authenticated`. Finding 9 is the check-then-charge
  shape. Proposed patch: before calling, `ai_charge(p_micro = worstCase)`
  where `worstCase = ceil(input_chars / 3 * PRICE_IN + max_tokens *
  PRICE_OUT)`, refuse with 503 if that fails or the result exceeds the cap;
  after the answer, `ai_charge(p_micro = cost - worstCase)` (the function
  would need to accept a negative correction, floored at zero).
- Fencing (finding 10): `</email >`, `</email\n>` and look-alike tags pass.
  The worst outcome is a summary or task that says what the sender wants,
  shown only to the person who asked. Proposed patch:
  `new RegExp('<\\s*/?\\s*' + tag + '\\b[^>]*>', 'gi')`, or a random tag per
  request (`email-<8 hex>`) named in the system prompt; and add
  `‎‏‪-‮⁦-⁩` to `clean`.
- Task ids: `parseTasks` keeps only ids that were sent, once each, with a
  120-character task and a strict date. Sound.

## 8. Stripe webhook and account deletion

**What it protects.** Only Stripe can change who is entitled, events cannot
be replayed or applied out of order, and only the account holder can delete
an account.

**What I tried.**

- Signature: HMAC-SHA256 over `t.rawBody` with the raw text read before
  parsing, `timingSafeEqual` with a length check, any of several `v1`, and a
  five-minute tolerance, so a captured delivery cannot be replayed later.
  Replaying a genuine event inside five minutes writes the same values.
- Ordering: the customer branch filters `event_at <= incoming` in the same
  UPDATE, so it is atomic. The email branch reads then writes, so two
  deliveries for one address can race; the harm is one stale write, which
  the next event corrects. Events in the same second can apply in either
  order (`lte`). Both are minor and not listed separately.
- The `.or(...)` filter string is built from an ISO date made from a number,
  so it cannot inject into the PostgREST filter.
- RLS: `subscriptions` is readable only by its own address and writable only
  by the service role; `ai_usage` has no user policy.

**Finding 3, Medium.** The email branch upserts `stripe_customer`, `plan` and
`status` on the address from the checkout. Stripe's checkout lets the buyer
type any address (`prefilled_email` can be edited). So: pay for Base once
with a Pro customer's address; their row now says Base and names the
attacker's Stripe customer; cancel; `customer.subscription.deleted` for the
attacker's customer cancels the victim's row. The victim's own later events
(`customer.subscription.updated` for their customer id) match no row and get
409 until Stripe gives up. It also happens innocently when one person
checks out twice. Proposed patch (`app/api/stripe/webhook/route.js`, email
branch):

```js
const { data: held } = await sb.from('subscriptions')
  .select('event_at,stripe_customer,status').eq('email', row.email).maybeSingle();
if (held && held.stripe_customer && row.stripe_customer
    && held.stripe_customer !== row.stripe_customer
    && LIVE_STATUSES.includes(String(held.status || '').toLowerCase())) {
  /* A second customer for an address that already pays: never replace the
     first. Answered 200 so Stripe stops; the owner reconciles by hand. */
  console.warn('stripe: checkout for an address with a live subscription under another customer');
  return NextResponse.json({ received: true, acted: false, conflict: true, type: event.type });
}
```

Test first, in `rata-next/tests`: a checkout for an address whose row is
live under `cus_A`, arriving from `cus_B`, must leave the row unchanged.

**Finding 11, Low.** In `rowForEvent`:

```js
const paid = ['paid', 'no_payment_required'].includes(o.payment_status);
if (o.status !== 'complete' || (o.payment_status && !paid)) return null;
```

plus a branch for `checkout.session.async_payment_succeeded` that returns
the same row.

**Account deletion.** `DELETE /api/account` needs a Supabase JWT that
`auth.getUser` validates on the server, the address typed back exactly, and
refuses while the subscription is live. Sound. (It answers some errors with
status 200, which is untidy, not unsafe.)

## 9. Everything else

- **Updater.** `requireSignedVersion` is a real option in
  tauri-plugin-updater 2.12.0 (the version in `Cargo.lock`): it reads the
  version from minisign's trusted comment only after the signature
  verified, and refuses a mismatch or a missing version. `release.yml`
  merges only `pubkey` and `createUpdaterArtifacts` through `--config`, so
  `requireSignedVersion` and the https endpoint stay. A build without a key
  registers no updater. None.
- **CSP exemption.** `dangerousDisableAssetCspModification` lists only
  `style-src`. The app CSP keeps `script-src 'self' 'unsafe-inline'`, which
  is safe only because Tauri adds its hashes and nonce to `script-src` in a
  packaged build; `form-action 'none'`, `frame-src 'none'`, `object-src
  'none'`, `connect-src` limited to IPC and `https://mailrata.org`. No
  mail-derived URL is ever put in an `<img>` on the app page (the page's
  `img-src https:` exists for the frame). None.
- **Secrets in logs.** No `println!`, `eprintln!` or `log::` outside tests in
  either crate. `Credential`'s `Debug` hides the secret. The website's
  `/api/config` refuses to publish a service-role key as the anon key.
  `/api/health` compares its token in constant time. None.
- **What leaves the device.** In the app, `config.js` has an empty
  `supabaseUrl`, `apiFetch` goes only to `bridge.js`, and the CSP allows only
  mailrata.org, which is called for renewal and AI. Signatures and per-device
  fields are stripped from the website's cloud copy. None.
- **Website headers** (finding 13): add to `next.config.js`
  `headers()` returning, for `/(.*)`, `Content-Security-Policy:
  frame-ancestors 'none'`, `X-Frame-Options: DENY`,
  `Strict-Transport-Security: max-age=31536000`, `Referrer-Policy:
  strict-origin-when-cross-origin`, `X-Content-Type-Options: nosniff`.
- **App data file** (finding 14): in `store.rs` `save`, on Unix open the
  temporary file with `OpenOptions::new().mode(0o600)` before renaming.

## Found on the way (not security)

- `lib/stripe.js` says `past_due` is deliberately still entitled
  (`LIVE_STATUSES`), but `entitlementsForUser` in `lib/plan.js` accepts only
  `active` and `trialing`. A customer whose card is being retried cannot
  renew or get a licence. One of the two is wrong; the comment says it is
  `plan.js`. **Fixed in this PR (SEC-1):** the list now lives once in
  `lib/plan.js` (`past_due` entitled) and `lib/stripe.js` re-exports it.
- The live mailrata.org is still the old deploy: `/api/ai` and `/account`
  answer 404 today (already known from CLAUDE.md).

## How this was checked

- `cargo test --all-targets` in `desktop/rata-mail`: 215 passed.
- `./sync-ui.sh`, then `cargo test` in `desktop/rata-app/src-tauri`: 63
  passed.
- `npm ci && npm run build && npm test` in `rata-next`: all checks passed;
  `npm audit --omit=dev`: 0 vulnerabilities. `cargo audit` is not installed
  here, so Rust advisories were checked by hand only for ammonia.
- Gate checks: the four `cargo check --release` runs quoted in section 3.
- Throwaway probes, none committed: a sanitiser test in `rata-mail/tests`, a
  `links.rs` test (reverted), a Playwright frame probe served on port 3186,
  and a Node check of `lib/ai.js`.
- Not done: a packaged WebKit build. The frame was checked in Chromium, and
  CLAUDE.md records the same checks passing in the packaged WebKit build for
  0.1.12 and 0.1.14.

## Part 3 (2026-09-29): code added since part 1

Reviewer: a fresh agent session with no part in writing the code (card F4,
part 3). Scope: what merged after part 1 (#68) and was not reviewed with it
or with C2 (#67). That is drafts saved to the server (#78), disguised
programs and marking downloads (#71, #74, #75), exact navigation, the store
and SMTP replies (#77), and the website's checkout, renewal, AI cap,
account deletion and browser banner (#65, #69, #72, #73, #77). Checked on
`main` at `3f110ff`, then merged up to `01d40a7` (#80 and #81 touch none of
it).

**Result: no High.** Two Medium, five Low. No invariant in Part 4 of
`docs/MVP-PLAN.md` is broken. A draft goes only to the customer's own IMAP
server, the one permanent delete is held to RATA's own copy, and nothing
new reaches mailrata.org. Nothing is fixed here. Every Medium and Low has a
proposed patch below.

| # | Area | Finding | Severity | Evidence | Proposed fix |
|---|---|---|---|---|---|
| P3-1 | Disguised programs (`names.rs`) | An invisible character or a look-alike dot defeats both `safe_file_name` and `looks_disguised`. `invoice.pdf<U+200B>.exe` is saved under that name with no question, and Explorer, hiding known extensions, shows it as `invoice.pdf`. U+2060, U+FEFF, U+00AD, U+180E and a tag character (U+E0020) do the same, and so do `invoice<U+2024>pdf.exe`, U+FF0E and U+FE52. Rust's `is_control` covers only category Cc, so format characters (Cf) are kept. The Mark of the Web is still written, so SmartScreen still checks the file. What is lost is BUG-4's clean name and UI-1's question, and one character is enough to lose both. | Medium | A throwaway test through the public API. All ten names above came back `disguised=false`, with the invisible character still in the name. | **Fixed (SEC-6).** The reviewer's patch, widened to every format character (category Cf, Unicode 16) as well as the rest of Default_Ignorable_Code_Point: `safe_file_name` drops them all (`invisible`), and `looks_disguised` splits on eleven look-alike dots too (`looks_like_a_dot`). Test: `nothing_invisible_or_dot_shaped_hides_a_program` (the ten names above, 31 invisible characters, look-alike dots in honest names left alone), failing first; the existing `names` tests still pass. |
| P3-2 | Drafts on Gmail (`imap::replace`) | On Gmail, `UID STORE \Deleted` then `UID EXPUNGE` in `[Gmail]/Drafts` very likely just removes the Drafts label (Gmail's default setting is to archive the message). Each earlier revision would then stay in All Mail with its Bcc line and attachments. A draft being edited is saved every two minutes, so a 10 MB draft edited for half an hour leaves about 150 MB in the customer's quota. Every copy also shows under RATA's own Archive, because `-in:inbox -in:sent -in:drafts` matches it. Discard moves only the latest copy to the Trash, so text the customer threw away stays in the mailbox. | Medium (not seen on real Gmail) | `replace` has no Gmail case. RATA's own `GMAIL_ARCHIVED` query excludes `in:drafts` because Gmail lists drafts in All Mail. Gmail's IMAP settings say an expunge from a folder that is not the message's last visible one only removes that label. CLAUDE.md already lists this as a B2 check. | Patch in [P3-2](#p3-2-gmail-drafts). For B2: save a draft twice on a real Gmail account, then look in All Mail. |
| P3-3 | Drafts, header search fallback (`save_in`) | Without `APPENDUID`, the new copy's UID is the newest match for `UID SEARCH UNDELETED HEADER X-RATA-Draft "<id>"`, and IMAP HEADER search matches substrings. A message whose header merely *contains* the id is then taken as RATA's copy. It is recorded in `CMP_SRV`, listed as the draft, and moved to the Trash when the draft is sent or discarded, while RATA's real copy is left behind. Nothing is expunged by mistake: on the next save, `replace`'s exact header check refuses the adopted UID (`NotOurs`). So the Trash move is the only harm. It needs a server that sends no APPENDUID (one advertising UIDPLUS must send it) and a stranger's message in Drafts carrying this draft's random UUID. | Low | A throwaway test on the scripted Drafts server, changed to match SEARCH by substring. The first save recorded UID 30 (another message) as its copy and correctly replaced 3. The second save left 30 alone (`NotOurs`). Held afterwards: 30, 20 (orphaned) and 21. | After the search, fetch the chosen UID's `X-RATA-Draft` header (the same fetch `replace` does) and accept the UID only if the header is exactly `[draft_id]`. Also consider only UIDs at or above the `UIDNEXT` read before the APPEND. |
| P3-4 | Disguise lists (`names.rs`) | The lists are the owner's choice, but common pairs are missing. Documents: `csv`, `rtf`, `odt`, `html`, `mp4`. Programs: `cpl`, `msc`, `reg`, `iso`, `img`, `vhd`, `one`, `chm`, `appx`/`msix`, `url`, `xll`. So `invoice.pdf.iso` and `report.csv.exe` save without a question. | Low | The same throwaway test: every one came back `disguised=false`. | **Partly fixed (SEC-6), the conservative default.** Added the unambiguous pairs: DOCUMENT `csv rtf html htm`, PROGRAM `cpl reg url iso one`. `setup.exe`, `backup.iso` and other names that say what they are stay unflagged. Test: `the_wider_lists_catch_more_disguises_and_leave_honest_names_alone`. The rest of the suggested list is still the owner's call. |
| P3-5 | Stripe webhook, `incomplete` rows | `checkoutConflict` protects only live rows. Suppose customer V is paying by bank transfer, so V's row is `incomplete` under `cus_V`. Anyone can then pay by card with V's address, and that paid checkout under `cus_A` replaces the row. When V's money arrives, `async_payment_succeeded` is refused as a conflict. V's subscription events match no row: each gets a 409, is retried, and is dropped after three days. If A then cancels, V has paid but is not entitled until the owner reconciles from the conflict log line. The attacker has to pay for a subscription, and the window lasts only as long as the transfer. | Low | `lib/stripe.js` `checkoutConflict`: `if (!LIVE_STATUSES.includes(status)) return false`. | **Fixed (SEC-6).** `checkoutConflict` counts `PENDING` as held, as proposed. Tests: the unit case, and through the endpoint a paid checkout by another customer over an `incomplete` row writes nothing, while the owner's own transfer clearing still makes it active. |
| P3-6 | Account deletion and the AI cap | Deleting an account also deletes `ai_usage`, the month's running total. The relay's only credential is the licence, which stays valid for up to 30 days. So a customer can cancel, delete the account, and spend the cap a second time that month. That is at most `AI_MONTHLY_CAP_USD` extra per paid subscription, since deletion is refused while the subscription is live. | Low | `app/api/account/route.js` calls `wipe('ai_usage', …)`, and `app/api/ai/route.js` checks only the licence. | **Fixed (SEC-6), by refusing rather than keeping.** `blocksDeletion` refuses while a licence the subscription could have issued may still work: until `LICENCE_DAYS` after the row last changed (the later of `event_at` and `updated_at`), with the date. A status that was never live (`incomplete_expired`) issued none and does not block. Keeping this month's usage row would keep an address RATA was asked to forget, with nothing to remove it later; hashing it needs the relay and `database.sql` §5 changed too. `incomplete` now blocks as well (below). Tests in `account-delete.test.mjs`, unit and through the endpoint. |
| P3-7 | IMAP error text (row 8, still open) | The IMAP half of row 8 was waiting because F1 held `imap.rs`. F1 has merged, so it can go ahead now. The new draft code adds no raw server text beyond `one_line` of an APPEND refusal. | Low (row 8) | `append` returns `Appended::Refused(one_line(…))`. | As row 8. SEC-5 is on it. |
| - | Drafts: the permanent delete | The `replace` rule holds, and each of these is refused and tested: UID 0, the UID of the copy just saved (also when a server's `APPENDUID` names the old UID), another UIDVALIDITY, a `prior.draft_id` other than the id being saved, a copy with another id, no id or two ids, and a copy that is gone. `UID STORE` goes by UID (`uid_store`). The only expunge is `UID EXPUNGE <uid>`, never a bare `EXPUNGE`. Without UIDPLUS the copy is left flagged. `act` still never expunges. A hostile message cannot set a draft id: `Message.draft_id` is filled only for the Drafts folder and only through `draft_id_ok`, and otherwise the page makes its own with `crypto.randomUUID`. | None | Read `save_in`, `replace`, `rata_draft_id` and `store`. The scripted-server tests `a_copy_that_is_not_provably_rata_s_is_never_touched` and `a_rebuilt_drafts_folder_or_a_mismatched_reference_leaves_the_copy_alone` cover these cases, and the loopback test runs the same rule against Dovecot. | - |
| - | Drafts: header injection and Bcc | A throwaway render of a draft with CR/LF and `X-RATA-Draft:` in the subject, the In-Reply-To, the attachment name and its MIME type. None of them became a header of its own. The subject lost its line breaks and stayed one `Subject:` line, In-Reply-To stayed one line, the name went inside `name="…"`, and the bad MIME type became `application/octet-stream`. `Address::parse` refuses CR/LF. The draft id must be 16 to 64 characters of lowercase hex and `-`, checked in both the app and the engine. Bcc never reaches a sent message: `render` has no Bcc path, a forward quotes only From, Date and Subject, Reply all reads `toAll` and `cc` but never `bcc`, and `Message.bcc` is filled only for Drafts. When another client sends the draft, handling its Bcc line is that client's job, as with drafts Thunderbird and Apple Mail save (RFC 5322 §3.6.3). | None | The throwaway test's output. Read `render_as`, `forwardMessage` and `replyAllOf`. | - |
| - | Drafts: the page and the gates | Only the composer starts a save. It counts edits from input events on its own fields, and the mail frame runs no script. `Rata::save_draft` checks `usable` (licensed, linked, not parked) before anything else and `draft_id_ok` before rendering. `outgoing()` applies the same checks as sending (addresses, 18 MB, 100 people), and the engine refuses a From that is not the mailbox. An auth failure parks the mailbox, so the timer never sends a refused password again. `X-RATA-Draft-Rev` is written but never read, so starting it again at 1 does no harm. | None | Read `queueDraftSave`, `saveDraft`, `continueDraft`, `core::save_draft` and `outgoing`. Harness: 118 checks passed, the F1 ones among them. | - |
| - | Saving: consent is judged in Rust | The page may send `confirmed`. That is by design: the page is trusted for the click. The name judged is the one Rust fetched itself (`attachment_of`, by folder, UID, UIDVALIDITY and index), and `save_fetched` and `hand_fetched` then use those same bytes. So another index or folder is judged for what it really is. | None | Read `consented`, `attachment_of`, `save_fetched` and `hand_fetched`. | - |
| - | Marking (`mark.rs`) | Both callers pass `write_new` only `safe_file_name` output, which removes `:`, so no stream name can be injected into `file:Zone.Identifier`. The quarantine value `0081;<hex>;RATA;` has the same form Chrome and Mail write. The mark is added just after the file is closed, so the file sits unmarked for a few milliseconds. Neither mark has been seen yet on a real Windows or Mac. | None | Read `mark.rs`, `write_new`, `save_fetched` and `save_file`. | - |
| - | Navigation (`links::navigation`) | Exact for each platform. A user name, a password or a non-default port is refused, and hosts must match exactly (`tauri://LOCALHOST` and `tauri.localhost.` are refused, which fails safe). On Windows `https://tauri.localhost` is also kept, although the app is served over `http` (no `useHttpsScheme`). That address could reach only a local server holding a certificate trusted for that name, and only a script in the app's own page can navigate the window. | None | Read `navigation_on`, `is_app` and their tests. | - |
| - | Store (`store.rs`) | The temporary file's name is predictable (`mailboxes.json.tmp`). But it is removed and then opened with `create_new` (`O_EXCL`), which never follows a symlink, in a folder made `0700`. A race would need write access to the customer's own app folder. | None | Read `save`, `open` and `make_dir`. | - |
| - | SMTP replies (`hear`, `said`) | Each line is capped at 1 KiB before it is kept, the reply code is checked as bytes, and a multi-line reply is capped at 8 KiB. Every error string, connection errors included, goes through `said`. `hide` lower-cases only ASCII, so match offsets stay on character boundaries. | None | Read `smtp.rs` lines 49–125, 242–283 and 478–520. | - |
| - | Webhook, async payments | Every event is signed by Stripe. `async_payment_succeeded` passes the same stale check (`isNewer` against the row's `event_at`, which subscription events also stamp), so it cannot bring back a row that a later `customer.subscription.deleted` ended. A pending checkout never overwrites a live row, and another customer's checkout cannot take over a live one. | None (see P3-5) | Read `rowForEvent`, `checkoutConflict` and the route's email branch. `npm test`: 500 passed. | - |
| - | Renewal `too-old` | The holder can already read `exp` from the token, so the reason reveals nothing new. A signed token without a numeric `exp` cannot be renewed (it fails closed). | None | Read `check`, `renewable` and `renew/route.js`. | - |
| - | `ai_reserve` / `ai_settle` | Both are `security definer` with `search_path = public` pinned and the table named in full. Execute is revoked from `public`, `anon` and `authenticated` and granted only to `service_role`, and RLS is on with no policy. A negative delta cannot take a total below 0, and a settle with no row changes nothing and returns NULL. A positive delta could pass the cap only if the real cost were higher than the worst case, which counts every prompt byte as a token. | None | Read `database.sql` §5 and `app/api/ai/route.js`. | - |
| - | Browser banner | The `localStorage` read and write are wrapped in try/catch, the markup starts hidden, and the app never shows it (harness, licensed and unlicensed). | None | `webNote()`, and the harness. | - |

### P3-1. Invisible characters in file names

With this patch, the ten names below come back `disguised=true` with the
invisible characters gone. All 10 existing `names` tests still pass (tried
here, not committed). Removing U+200C and U+200D also takes them out of
Persian names and emoji sequences. That is acceptable in a file name.

```diff
--- a/desktop/rata-mail/src/names.rs
+++ b/desktop/rata-mail/src/names.rs
@@ pub fn safe_file_name(name: &str) -> String {
         !c.is_control()
             && !matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*')
             && !matches!(*c as u32, 0x200e | 0x200f | 0x202a..=0x202e | 0x2066..=0x2069 | 0x061c)
+            && !invisible(*c)
     }) {
@@ pub fn looks_disguised(name: &str) -> bool {
     let clean = safe_file_name(name);
-    let mut parts = clean.rsplit('.');
+    let mut parts = clean.rsplit(|c: char| c == '.' || looks_like_a_dot(c));
@@
+/// Characters that draw nothing (Unicode Default_Ignorable_Code_Point, less
+/// the Hangul fillers, which `blank` already turns into a space).
+fn invisible(c: char) -> bool {
+    matches!(c as u32,
+        0x00ad | 0x034f | 0x17b4 | 0x17b5 | 0x180b..=0x180f | 0x200b..=0x200d
+        | 0x2060..=0x2065 | 0x206a..=0x206f | 0xfe00..=0xfe0f | 0xfeff
+        | 0xfff0..=0xfff8 | 0x1bca0..=0x1bca3 | 0x1d173..=0x1d17a | 0xe0000..=0xe0fff)
+}
+
+/// Characters drawn as a full stop that Windows does not read as one.
+fn looks_like_a_dot(c: char) -> bool {
+    matches!(c, '\u{2024}' | '\u{fe52}' | '\u{ff0e}' | '\u{0701}' | '\u{0702}'
+        | '\u{a4f8}' | '\u{10a50}' | '\u{2e31}' | '\u{00b7}' | '\u{2027}' | '\u{0387}')
+}
```

The test to add first, in `names.rs`:

```rust
#[test]
fn nothing_invisible_or_dot_shaped_hides_a_program() {
    for name in [
        "invoice.pdf\u{200b}.exe", "invoice.pd\u{200b}f.exe", "invoice.pdf\u{2060}.exe",
        "invoice.pdf\u{feff}.exe", "invoice.pdf\u{ad}.exe", "invoice.pdf\u{180e}.exe",
        "invoice.pdf\u{e0020}.exe", "invoice\u{2024}pdf.exe", "invoice\u{ff0e}pdf.exe",
        "invoice\u{fe52}pdf.exe",
    ] {
        assert!(looks_disguised(name), "{name:?}");
    }
    assert_eq!(safe_file_name("invoice.pdf\u{200b}.exe"), "invoice.pdf.exe");
}
```

### P3-2. Gmail drafts

In `replace`, after the header check and the capability read, and before
the `STORE`:

```rust
    // Gmail: expunging from [Gmail]/Drafts only removes the Drafts label,
    // and the copy stays in All Mail (where RATA's archive would show it).
    // Moving it to Gmail's Trash takes it out of All Mail, and Gmail empties
    // the Trash after 30 days. The check above still decides what is moved.
    if caps.has_str("X-GM-EXT-1") {
        let Some(trash) = destination(session, &Action::Trash).await else {
            return Prior::Failed;
        };
        return match timeout(COMMAND, session.uid_mv(&set, &trash)).await {
            Ok(Ok(())) => Prior::Replaced,
            _ => Prior::Failed,
        };
    }
```

`destination` only runs LIST, so Drafts stays selected. Write the test
first, on the scripted Gmail server (the script of
`gmail_keeps_drafts_in_its_own_drafts_folder`): the earlier copy is moved to
`[Gmail]/Trash`, and `UID EXPUNGE` is never sent. Whether Gmail really
keeps the copy in All Mail is for B2 to check. If it does not, the patch
costs only one Trash entry per save.

### Found on the way (not security)

- **Every Stripe checkout is recorded as Base.** The Stripe API reference
  says a Checkout Session includes `line_items` only when it is expanded
  ("includable (not returned by default)"), and a webhook event is never
  expanded. So `rowForEvent` finds no price in `checkout.session.completed`
  or `…async_payment_succeeded` and writes `plan: 'base'` and
  `domain_addons: 0`. The tests miss this because their fixtures include
  `line_items`. Checked by calling `rowForEvent` with a Pro session that has
  none: it returned `plan: 'base'`. The subscription events do carry
  `items` and set the right plan, but only one that lands after the
  checkout counts, because the stale check refuses an earlier one. Until
  then a Pro buyer is Base. And the new `async_payment_succeeded` arrives
  days after those events, so it turns a paying Pro customer back into
  Base. Fix: a checkout event writes `plan` and `domain_addons` only when
  it carries line items. In the route, write `plan: row.plan ?? held?.plan
  ?? 'base'`, and have `rowForEvent` return `plan: null` when it finds no
  price. Add a test fixture without `line_items`. This needs fixing before
  D6 (Stripe live).
  **Fixed (SEC-6).** The subscription events are the source of truth for
  the plan and the add-ons. A checkout event writes them only when it
  carries line items (never, from Stripe), and otherwise keeps what a
  subscription event of this purchase named; a row whose plan no
  subscription event has named yet is written `incomplete` even when paid,
  so `/account` cannot hand a Pro buyer a Base licence for 30 days in the
  seconds before the subscription event lands. A checkout stamps the row
  with when the buyer opened it (the session's `created`), so no
  subscription event of the purchase is refused as stale because the
  checkout event or `async_payment_succeeded` was raised after it. The
  webhook must now also subscribe to `customer.subscription.created`
  (STRIPE-SETUP.md §3, LAUNCH.md §3). Tests: fixtures in Stripe's real
  shape without `line_items`, and the sequence Pro checkout (unpaid) →
  `customer.subscription.created` (Pro) → `async_payment_succeeded`, which
  ends Pro and active.
- An `incomplete` subscription does not block deleting the account. If the
  account is deleted while a bank transfer is pending, the row comes back
  as `active` when the transfer clears, and Stripe keeps billing an address
  that has no login. `blocksDeletion` could count `incomplete` as well.
  **Fixed (SEC-6):** it does.

### How this was checked

- `cargo test --all-targets` in `desktop/rata-mail`: 246 passed.
- `./sync-ui.sh`, then `cargo test` in `desktop/rata-app/src-tauri`: 111
  passed.
- `npm ci && npm run build && npm test` in `rata-next`: 500 passed, 0
  failed. `npm audit --omit=dev`: 0 vulnerabilities.
- The interface, driven (`ui-harness.mjs` against `desktop/rata-app/ui` on
  port 3207): 118 checks, all passed.
- Throwaway probes, none committed: a `names` test through the public API
  (P3-1, P3-4), a `render_draft` header-injection test, a scripted-server
  test in `imap.rs` with substring SEARCH (P3-3, reverted), the P3-1 patch
  applied and tested (reverted), and a Node call of `rowForEvent` on a
  session without `line_items`.
- Not done: real Gmail (P3-2 is for B2), a real Windows or Mac for the
  marks, and a packaged build. No page code was changed, and no claim here
  depends on the page beyond what the harness checks.
