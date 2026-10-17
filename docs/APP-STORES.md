# RATA in the app stores

A short record for whoever works on this next: what the owner has decided,
the store rules that shape the design, what is being built and what only
the owner can do. **Nothing here has run on a phone yet.** Every claim
about a mobile build comes from reading the code and the stores' rules,
not from a device.

It is drawn from two documents outside the repository: the research and
plan sent to the owner on 2026-10-14 (99 store rules, each checked against
its source by a second reader; 96 confirmed and 3 corrected, all as read
on 2026-10-14, with Apple's guidelines last updated June 8, 2026), and the
in-app purchase design of 2026-10-15. The direct downloads (`.dmg`,
`.exe`, `.deb`, AppImage) and the website's Stripe checkout are not
affected by any of it.

## What the owner has decided

- **Order, decided 2026-10-17:** the first public launch is the desktop
  direct download (the `.dmg`, `.exe`, `.deb` and AppImage, sold through
  Stripe on mailrata.org). The App Store builds come after it: the iOS App
  Store first, then Google Play. When a Mac App Store build ships is still
  open (D10 below). Nothing in this file holds the desktop launch up.
- **Store identifiers (D9), decided 2026-10-17:** the iPhone and Mac App
  Store builds share the bundle id `org.mailrata.app`, so they are one app
  record in App Store Connect and one subscription covers both (Universal
  Purchase: Apple has every platform of a record use the iOS app's bundle
  id, and one purchase covers the app and its in-app purchases on each).
  The direct-download desktop app keeps `org.mailrata.desktop`, which
  `tauri.conf.json` names today. The owner makes the App Store Connect
  app record with `org.mailrata.app`. Two things follow: the Tauri CLI
  (2.12.0) warns, but does not refuse, when an identifier ends in `.app`,
  the extension of a macOS app bundle, which concerns only a Mac App Store
  build, and none has been made yet; and the in-app purchase
  server must be told the id (`APPLE_BUNDLE_ID`, below), since #167 as
  written defaults to `org.mailrata.desktop`. #170's `RATA_IOS_BUNDLE_ID`
  already defaults to `org.mailrata.app`. The store version (1.x, no
  "beta") is still open under D9.
- **Apple: In-App Purchase (option A1), decided 2026-10-15.** Base and Pro
  are sold inside the iOS app as two monthly auto-renewable subscriptions
  in one subscription group, "RATA":

  | Product id | Plan | Group level |
  |---|---|---|
  | `org.mailrata.pro.monthly` | Pro | 1 (highest) |
  | `org.mailrata.base.monthly` | Base | 2 |

  - Buying needs no mailrata.org account. People who already subscribe on
    mailrata.org sign in to it from the app and use their plan there.
  - Prices come from StoreKit only; the app never hard-codes one.
  - No free trial. Introductory offers and Family Sharing are the owner's
    choice in App Store Connect and need no code.
  - The licence stays what it is: the Ed25519 token from `lib/licence.js`,
    checked offline by the app and renewed by it. For an App Store purchase
    its `sub` is `apple:<originalTransactionId>`, not an address; the
    server keeps no name, address or Apple ID for an App Store buyer.
  - Later, not now: linking an App Store purchase to a mailrata.org account
    so the same plan works on a computer (the schema leaves room for it).
- **Google Play: consumption-only (option G1).** The Android app signs in
  with a plan bought on mailrata.org and sells nothing. No Play Billing, no
  fee, but nothing reachable from the app may lead to a purchase, the
  help, privacy and account pages included.
- **Not decided:** a "Buy on mailrata.org" link on the US App Store only
  (option A2, which the plan recommends once A1 works), and any EU link or
  other processor (A3, not worth it at RATA's size).

## The rules that shape the design

Source links are at the end. Apple numbers are App Review Guideline
sections; Google numbers are Play Console Help answers.

| What | Apple | Google Play | What RATA does about it |
|---|---|---|---|
| What unlocks the paid app | 3.1.1: features and subscriptions are unlocked by In-App Purchase, and licence keys are named as a banned unlock; restorable purchases need Restore. 2.4.5(vi): no licence screen or required key on the Mac App Store. | 9858738: in-app purchases go through Play billing; 10281818: a consumption-only app may unlock access bought elsewhere if nothing can be bought in it. | The key box is not the unlock in an iOS build: a StoreKit purchase, or signing in to mailrata.org. On Android no rule bans a key, but the same sign-in serves both stores. |
| Plans bought on the website | 3.1.3(b): usable in the app only if the same plans are also sold by IAP. | 10281818: a user may log in and use content paid for elsewhere. | Website subscribers sign in; on iOS the same two plans are on sale. |
| Buy links, website prices, billing portal | 3.1.1(a), 3.1.3: none outside the US storefront (US allowed since 2025-05-01, beside IAP). | 9858738: nothing may lead to another payment method; 10281818: plain words without a link are allowed. | Store flavour: no Stripe links, prices or portal; "Not in your plan" with no price. |
| Pages the app links to | Covered by 3.1.1(a). | 10281818: help, privacy and account pages may be linked only if they never lead on to a payment. | Store-safe copies at `/in-app/…` with no site navigation and no link to pricing, `/account`, `/app` or Stripe. |
| Subscription terms | 3.1.2(a): at least 7 days, available on all the user's devices. | 9900533: price, period, renewal and whether a subscription is needed, shown without extra steps. | The purchase screen shows StoreKit's price, the renewal sentence, Terms and Privacy, Restore. |
| No self-update | 2.5.2; 2.4.5(iv) and (vii) on the Mac. | 9888379 (updates only through Play); 12085295 (`REQUEST_INSTALL_PACKAGES`). | A `self-update` Cargo feature, off in store builds; no update rows in the page. |
| Account deletion | 5.1.1(v) and its reference page: delete in the app (or link straight to the step), immediately available, and tell an IAP subscriber that Apple billing continues. | 10144311, 13327111: start deletion in the app and on the web; say any step needed first. | A public deletion page on mailrata.org; the 30-day refusal after a subscription ends conflicts with both (D4). |
| Privacy policy | 5.1.1(i): linked in App Store Connect and in the app. | 10144311: linked in Play Console and in the app. | Privacy and Terms rows in Settings; the `[OWNER: …]` placeholders filled (D11). |
| Mail text to a third-party AI | 5.1.2(i) (names third-party AI since 2025-11-13): say where it goes, get explicit permission. 5.1.1(ii): consent can be withdrawn. | 10144311 (applies to third-party AI since 2026-07-15): prominent disclosure, consent by an affirmative action. 13985936: report offensive AI output in the app (14094294 leaves limited-scope email AI out "at this time"). | An in-page consent naming Anthropic and each feature, a Settings switch to withdraw it, and "Report this answer" that opens RATA's own composer. |
| Store privacy forms | App Privacy details ("collect" counts third-party partners). | Data safety, 10787469. | Drafts of both answers for the owner; the "Emails" answer depends on D5. |
| iOS privacy manifest | `PrivacyInfo.xcprivacy`; required-reason APIs refused without a reason since 2024-05-01. | n/a | File timestamps (`std::fs::metadata`), confirmed by a symbol scan of the built binary. |
| Encryption | `ITSAppUsesNonExemptEncryption`; a non-OS standard algorithm needs the French declaration if sold in France. | n/a | rustls does the TLS, not the OS: answer the questionnaire, then set the key. |
| Sign-in to other services on a phone | ASWebAuthenticationSession; no loopback listener on iOS. | Google OAuth blocks loopback and custom-scheme redirects for mobile clients. | The loopback sign-ins (Microsoft, OneDrive, Google Drive, Slack) do not work on a phone until stage 3's mobile sign-in. |
| Tokens of other services | 5.1.1(v): stay on the device, revocable in the app. | n/a | Keychain only; never a token on mailrata.org. |
| Background mail | 2.5.4: background modes only for their purposes. | 13392821 and the foreground-service rules; Doze exemptions are not for email. | Mail is fetched while RATA is in front, plus best-effort background refresh (D7). No push: it would put mailbox credentials on a server. |
| Toolchain and targets | Xcode 26 and the iOS 26 SDK since 2026-04-28; the iOS 27 SDK and iOS 15 or later from April 2027. | Target API 36 from 2026-08-31 (11926878); `.aab`; Play App Signing. | A library target with a mobile entry point; iOS 15 or later. |
| Review access | 2.1(a): a demo account or an approved demo mode; 2.3.1(a): new features described specifically. | 9859455: sign-in details; 14151465: a new personal account runs a 12-tester, 14-day closed test first. | A reviewer's licence (`mint.sh`), a test mailbox, notes on the AI relay. |
| More than a website | 4.2. | n/a | No Install button or service worker in the app; safe areas; touch-sized controls. |
| Age rating and US state laws | 2.3.6; Texas SB 2420 from 2026-06-04 (Declared Age Range for IAP buyers there). | 16569691: Texas in effect; Age Signals offered, not required. | Answer the questionnaire; the Texas question goes to a lawyer. |
| Sign in with Apple | 4.8: not required for a company's own accounts or a mail client. | n/a | Holds while nobody can create the RATA account with Google or Microsoft. |

Re-verified on 2026-10-17 against Apple's own pages: 2.5.5 reads "Apps
must be fully functional on IPv6-only networks." Apple's *Supporting
IPv6-only Networks* says that "as of June 1, 2016, all apps submitted to
the App Store must support IPv6-only networks", that a server on the
wider internet "will be accessible to IPv6-only devices via DNS64/NAT64",
and that forming an IPv6 address by putting an IPv4 address after
`64:ff9b::/96` yourself is "not a general solution". Its older guide,
*Supporting IPv6 DNS64/NAT64 Networks*, sets a DNS64/NAT64 test network
made on a Mac beside the networks service providers supply, and says
"these networks (as well as the one used during App Review) do allow for
direct IPv6-to-IPv6 connectivity". Neither page says in so many words
that review runs on an IPv6-only network; what is certain is the
requirement.

Flagged by the code audit but not re-verified against a source: 2.2 (the
"beta" label belongs in TestFlight), 5.2.1 (other companies' marks in the
Apps view), the "HIPAA" default privacy-check profile, and 3.1.2(c) (Terms
and Privacy links in the binary). Check each before submitting. Payment
rules in the US are still moving: re-read Apple 3.1.1(a) and Google's
answer 15582165 just before launch.

## The stages, and what is in flight

**Stage 1: changes to the existing code that need no owner decision.** A
store build flavour (no self-update; no prices, buy links or billing
portal); in-page dialogs instead of `confirm()` and `prompt()`, which
Apple's webview answers with Cancel by itself (read in wry's source, not
yet seen on a Mac); the AI consent rewritten and made withdrawable;
"Report this answer"; Privacy and Terms rows in Settings; store-safe help,
privacy, terms and account-deletion pages; phone fixes (safe areas, no
Install button, notification permission asked only when turned on); the
outbound guard on IPv6-only networks (done: #164, merged 2026-10-16, on
main and first in the next release); and groundwork for mobile.

**Stage 2: the owner's decisions** (below).

**Stage 3: the mobile builds.** The lib target, an Android keychain
(keyring keeps nothing on Android), DNS on iOS through the system (the
engine's half done: #163, merged 2026-10-16; what it leaves is below),
`rustls-platform-verifier` set up on Android, links through
`tauri-plugin-opener`, links in formatted mail on mobile, saving through
pickers and share sheets, background refresh, mobile sign-in, the Android
manifest and the iOS project, touch sizes, CI jobs behind `gate`, and
testing on real devices and on an IPv6-only network.

**Stage 4: the owner's accounts and forms** (below, and
`docs/OWNER-CHECKLIST.md` §9).

**Merged, 2026-10-16** (on main, in no release yet; both change the
desktop app too, from the next release):

- #164 (`claude/nat64-guard`), stage 1. The outbound guard judges an
  address in NAT64's well-known prefix `64:ff9b::/96` by the IPv4 address
  in its last 32 bits, so a mail host with only IPv4 can be reached from
  an IPv6-only network with DNS64. The rest of `64:ff9b::/32`, the
  local-use `64:ff9b:1::/48` among it, stays refused; an operator's own
  NAT64 prefix is judged as ordinary IPv6. 0.1.52 and earlier refuse all
  of `64:ff9b::/32`.
- #163 (`claude/ios-dns`), stage 3, the engine's half of DNS on iOS. SRV,
  MX and TXT through dns_sd, addresses through getaddrinfo (which makes the
  NAT64 address on an IPv6-only network), no CNAME question ever, and
  nothing asked under `local` or the other private zones. Every address is
  still judged by the guard, which on every system now also refuses the
  zones `local`, `internal` and all of `arpa` themselves (0.1.52 and
  earlier do not). Its tests run on a Mac and in an iOS Simulator
  (CI job *Mail layer on macOS*), not on a phone. In the Simulator the
  engine's test program runs as a bare process (`xcrun simctl spawn`),
  outside any app's sandbox; the lookups from a sandboxed app are #166's
  Simulator smoke (open, below).

What they leave for stage 3, before submission:

- A test on a real IPv6-only network with NAT64, on a device, since 2.5.5
  requires an app to be fully functional on IPv6-only networks (above).
  GitHub's runners have no such network.
- An IPv4 address typed as the server is dialled as typed, never
  synthesised to NAT64, so on an IPv6-only network it cannot be reached
  (names are fine). Probably fixed by handing the literal to getaddrinfo on
  iOS too (`resolve_public`), and not visible on any runner.
- TXT over TCP through dns_sd has never been seen: an answer too big for one
  UDP reply goes over TCP, and the macOS runners' DNS never answers one.
  L1's SPF evidence for such a domain depends on it; check it on a device.
- iOS's Local Network prompt has never been seen. `.local` is no longer
  asked, but whether anything else in the app raises the prompt shows only
  in the app on a device.

**In flight, 2026-10-17** (open):

| Branch | Pull request | What |
|---|---|---|
| `claude/store-page` | #165 | Stage 1: the store build of the interface: in-page dialogs, no buy links, Privacy and Terms in Settings, safe areas. Both steps pushed; not to be merged until the lead says so (its description). |
| `claude/store-pages` | #162 | Stage 1: store-safe help, privacy and terms at `/in-app/…`, and a public account-deletion page. |
| `claude/ios-app` | #166 | Stage 3: the shell as a library a phone can start, the `self-update` feature, single instance limited to Windows and Linux, links through the opener plugin, and what needs a computer switched off on a phone; then the Xcode project, made afresh each run (`tauri ios init`, `ios/prepare.sh`; nothing under `gen/apple` committed), `tauri.ios.conf.json`, a `smoke` self-test feature, and `.github/workflows/ios.yml`: *iOS app in the Simulator* (the self-test, three runs and screenshots) and *iOS app for a device, unsigned* (a release build checked against the privacy manifest). Both steps in, checks green, ready for review. |
| `claude/ios-testflight` | #170 | Stage 3: signed builds to TestFlight (`.github/workflows/ios-testflight.yml`, job *iOS app to TestFlight*, its steps in `desktop/rata-app/ios/testflight.sh`) once the owner sets the four App Store Connect secrets (`APPLE_API_KEY_ID`, `APPLE_API_ISSUER_ID`, `APPLE_API_KEY_P8`, `APPLE_TEAM_ID`), all or none; with none set it says TestFlight is not set up yet and ends green, and with all four set it also requires the real `RATA_LICENCE_PUBLIC_KEY`. The bundle id comes from the repository variable `RATA_IOS_BUNDLE_ID`, default `org.mailrata.app` (D9's decided id, so the owner need not set it); `tauri.conf.json` keeps `org.mailrata.desktop`. Stacked on #166 (its diff includes #166's): merge #166 first. No upload has run. |
| `claude/iap-server` | #167 | The in-app purchase server on mailrata.org: Apple's signed purchases checked with Apple's own library against Apple's committed root certificates, `POST /api/licence/apple`, App Store Server Notifications V2 at `/api/apple/notifications`, `database.sql` §7 (`apple_subscriptions`), and renewal of an `apple:` licence. As written its `APPLE_BUNDLE_ID` defaults to `org.mailrata.desktop`, which is no longer the store's id (D9): the default needs changing to `org.mailrata.app` before it merges, or the owner sets `APPLE_BUNDLE_ID=org.mailrata.app`, or every purchase is refused as another app's. |
| `claude/iap-native` | #172 | Stage 3: the StoreKit 2 bridge, a local Tauri plugin for iOS only (`desktop/rata-app/plugins/storekit/`), called only from Rust (`appstore.rs`: products, buy, restore, manage and renewal through `/api/licence/apple`, each transaction finished only after the licence is kept), and `ios/RATA.storekit` for testing in Xcode. Stacked on #166. |

Still to come: the purchase screen and Settings rows in the page
("Choose your plan", Restore purchases, Manage subscription), after #165
and the assistant's own AI consent dialog (#161, `claude/assist-v1`) have
merged.

D6, which features ship on phones first: the plan recommends mail,
Microsoft mailbox sign-in and AI. #166 as pushed switches off everything
that needs a computer or a loopback sign-in, Microsoft mailbox sign-in
included, until mobile sign-in exists.

## The owner's decisions still open

| | Decision | Why it matters | The plan's recommendation |
|---|---|---|---|
| D4 | Account deletion policy | Both stores want deletion that can start at once; the website refuses it while a subscription is live and for 30 days after (`blocksDeletion`), to protect the AI allowance. | Delete at once and revoke the account's outstanding licences instead; say IAP billing continues through Apple. Changes `lib/account.js`, the pages and `tests/legal.test.mjs`. |
| D5 | What Anthropic keeps of AI text | Settles the consent wording, the privacy policy and the "Emails" answer on both forms. | Zero data retention, or a clear statement of Anthropic's retention. |
| D7 | New mail in the background on phones | Phones suspend RATA; there is no push without credentials on a server. | IDLE while in front, plus `BGAppRefreshTask` (iOS) and WorkManager (Android, 15 minutes or more). No promise of instant mail. |
| D8 | Phone backups | Backups would copy mail and the list off the device. | Exclude them: `isExcludedFromBackup` and keychain items "this device only" on iOS; no backup on Android. |
| D9 | The store version (the identifiers were decided on 2026-10-17: `org.mailrata.app` for the iPhone and Mac App Store builds, `org.mailrata.desktop` for the direct download; above) | "beta" belongs in TestFlight. | Ship stores at 1.x; a 1024 px icon without alpha. |
| D10 | When the Mac App Store build ships | Sandboxed, no licence screen; customers moving from the `.dmg` relink. Under D9 it shares the iPhone app's record and `org.mailrata.app`, so one subscription covers both. | Last, or never: the `.dmg` already serves Macs. |
| D11 | Legal entity, address, contact, governing law | Fills `privacy.html` and `terms.html`; Apple's EU trader status publishes them. | Needed before any store submission. |

## The owner's steps in App Store Connect

In order (also `docs/OWNER-CHECKLIST.md` §9):

1. The Apple Developer Program, then the Paid Apps agreement, banking and
   tax in App Store Connect.
2. The Small Business Program, for the lower commission.
3. The app record, with the bundle id `org.mailrata.app` (D9, decided
   2026-10-17), and its numeric App ID for `APPLE_APP_ID`.
4. The subscription group "RATA" and the two products above, with prices,
   names and descriptions in each language, and the review screenshot
   each product needs.
5. App Store Server Notifications V2, for Production and Sandbox, to
   `https://mailrata.org/api/apple/notifications`.
6. A sandbox tester.
7. `APPLE_BUNDLE_ID` (`org.mailrata.app`) and `APPLE_APP_ID` on the server.
8. Run `database.sql` §7.
9. Redeploy mailrata.org.
10. The App Privacy ("nutrition label") answers.
11. From TestFlight: a purchase, a restore, an upgrade and a cancellation.

Steps 7 to 9 wait for the in-app purchase server (#167) to merge; its
`LAUNCH.md` steps are then the full ones. Before the
first submission, also: the Digital Services Act trader details (shown
publicly; without them RATA is not on the EU store), the age-rating
questionnaire, the encryption questions, the privacy policy and support
addresses, screenshots and a description that claims nothing RATA does
not do, and Notes for Review with a reviewer's licence and a test mailbox.
Optional: the mail-client entitlement, so RATA can be the default mail
app on iOS. Separately, for the `.dmg`: a Developer ID G2 certificate
before the current one's sub-CA expires on 2027-02-01.

## The owner's two rules still hold

Every design above keeps mail and mail credentials on the customer's
device. Store pressure must not bring in a push relay, server-held tokens
or stored AI reports: "Report this answer" is an email from the customer's
own mailbox for that reason, and the in-app purchase server keeps nothing
personal for an App Store buyer.

## Sources

Apple, all read 2026-10-14:

- App Review Guidelines (3.1.1, 3.1.1(a), 3.1.2(a), 3.1.3, 3.1.3(b),
  3.1.3(f), 2.1(a), 2.3.1(a), 2.3.6, 2.4.5, 2.5.2, 2.5.4, 4.2, 4.8,
  5.1.1, 5.1.2(i)): https://developer.apple.com/app-store/review/guidelines/
- The US storefront change, 2025-05-01: https://developer.apple.com/news/?id=9txfddzf
- Third-party AI in 5.1.2(i), 2025-11-13: https://developer.apple.com/news/?id=ey6d8onl
- Account deletion (5.1.1(v)): https://developer.apple.com/help/app-review/guideline-reference/5-1-1-account-deletion
- Apps in the EU: https://developer.apple.com/support/apps-in-the-eu/
- App privacy details: https://developer.apple.com/app-store/app-privacy-details/
- Privacy manifests: https://developer.apple.com/documentation/bundleresources/privacy-manifest-files
- Required-reason APIs: https://developer.apple.com/documentation/bundleresources/describing-use-of-required-reason-api
- Encryption export: https://developer.apple.com/documentation/security/complying-with-encryption-export-regulations
  and https://developer.apple.com/help/app-store-connect/reference/app-information/export-compliance-documentation-for-encryption
- Upcoming requirements (SDK, quarantine, trader status): https://developer.apple.com/news/upcoming-requirements/
- Submitting (target iOS versions): https://developer.apple.com/app-store/submitting/
- US state age laws: https://developer.apple.com/news/?id=sg176nne
- Age ratings: https://developer.apple.com/help/app-store-connect/reference/app-information/age-ratings-values-and-definitions/
- The Developer Program License Agreement: https://developer.apple.com/support/terms/apple-developer-program-license-agreement/

Apple, read 2026-10-17:

- App Review Guidelines 2.5.5, same address as above.
- Supporting IPv6-only Networks: https://developer.apple.com/support/ipv6/
- Supporting IPv6 DNS64/NAT64 Networks (archived guide, section
  "Limitations of Local Testing"): https://developer.apple.com/library/archive/documentation/NetworkingInternetWeb/Conceptual/NetworkingOverview/UnderstandingandPreparingfortheIPv6Transition/UnderstandingandPreparingfortheIPv6Transition.html
- Universal Purchase, "Add platforms" in App Store Connect Help (the same
  bundle id as the iOS app on each platform of one record): https://developer.apple.com/support/universal-purchase/

Google Play, all read 2026-10-14 (`https://support.google.com/googleplay/android-developer/answer/<n>`):

- Payments: 9858738. Consumption-only apps: 10281818. Subscriptions:
  9900533. US programs: 15582165, 16671517, 16470497. Billing choice:
  17161464.
- Device and Network Abuse: 9888379. `REQUEST_INSTALL_PACKAGES`: 12085295.
- User Data and account deletion: 10144311, 13327111. Data safety:
  10787469.
- AI-generated content: 13985936, 14094294.
- Target API level: 11926878. Foreground services: 13392821. Permissions:
  9888170. All files access: 10467955.
- Sign-in details for review: 9859455. Closed testing: 14151465. Texas
  and age laws: 16569691. Developer registration: 17134731.
- Google OAuth for native apps: https://developers.google.com/identity/protocols/oauth2/native-app
- The new fee model: https://android-developers.googleblog.com/2026/06/play-expanded-billing.html
