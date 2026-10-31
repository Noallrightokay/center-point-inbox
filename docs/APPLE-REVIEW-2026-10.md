# RATA for iPhone: an audit against Apple's App Review rules

Written 2026-10-31, before the first iPhone submission. It reads the open
branches as they stood that day, so line numbers are those heads'; check a
finding against the code before acting on it. `docs/APP-STORES.md` is the
plan; this is the audit of the plan and the code against Apple's rules as
published on 2026-10-31. The multiseat and 2.5.19 items in (j) were
checked again the same day against https://developer.apple.com/news/.

## How this was done

- Apple's pages were fetched today and quoted word for word. The App Review
  Guidelines page says **"Last Updated: October 16, 2026"**. That is newer
  than the reading in `docs/APP-STORES.md`, which used the June 8, 2026
  version. The October changes are in section (j).
- Code was read from the branch heads as fetched today:
  `claude/ios-app` e0e2e51 (#166), `claude/iap-native` cc3fc3a (#172),
  `claude/store-page` eae4bd1 (#165), `claude/store-pages` 1c4e653 (#162),
  `claude/iap-server` 29fcc71 (#167), `main` fad014d.
- Evidence is given as `branch:path:line`. The same line comes back from
  `git show origin/<branch>:<path>`.
- The store-safe pages (`/in-app/help`, `/privacy`, `/terms`) were built
  locally from #162's `lib/in-app.js`, to read what a reviewer would see.
- What I could not do: run anything on a phone or in a Simulator, see CI
  results (the GitHub GraphQL API is refused here), or read App Store
  Connect. I did check the live site: `https://mailrata.org/` is the
  deploy of 2026-09-11, and `/privacy.html`, `/terms.html`, `/help`,
  `/in-app/*` and `/api/health` all answer **404** today.

## The short version

No branch, alone or together, is ready to submit. Two things would get the
first submission rejected for certain:

1. **The only way to unlock RATA on the iPhone is still a licence-key box.**
   Apple names licence keys as a banned unlock. There is no purchase screen,
   no Restore and no Manage subscription in the page yet, though the
   StoreKit side (#172) is built.
2. **The links Apple checks do not work.** The live privacy policy and terms
   are 404, the pages carry `[OWNER: …]` placeholders, and the licence, AI
   and purchase servers are not deployed.

Most other findings are wording: "beta" in the in-app terms and help, a
default "HIPAA" profile, brand logos in Apps, and a deletion flow that does
not mention Apple billing.

---

## (a) 2.2: "beta" in the app, and a 0.1.x version

**Apple's words** (https://developer.apple.com/app-store/review/guidelines/):

> 2.2 Beta Testing: "Demos, betas, and trial versions of your app don't
> belong on the App Store – use TestFlight instead."

> 2.1(a): "Submissions to App Review, including apps you make available for
> pre-order, should be final versions with all necessary metadata and fully
> functional URLs included; placeholder text, empty websites, and other
> temporary content should be scrubbed before submission."

On the version number, Apple's `CFBundleShortVersionString` page
(https://developer.apple.com/documentation/bundleresources/information-property-list/cfbundleshortversionstring)
says: "The required format is three period-separated integers, such as
10.14.1. The string can only contain numeric characters (0-9) and
periods."

**What this means.** A version like 0.1.52 is valid. No rule bans a
leading zero. The risk is the **word "beta"** anywhere a reviewer reads it,
because 2.2 says betas go to TestFlight.

**Verdict: not met** (the word is still shown); version number: met.

**Evidence**
- On an iPhone the window title (`RATA 0.1.52 beta`,
  `ios-app:desktop/rata-app/src-tauri/tauri.conf.json:12`) is not shown, so
  the title itself is harmless. The iOS build takes `version` 0.1.52 from
  the same file (line 4); `tauri.ios.conf.json` does not override it.
- The in-app terms the store build links to say "A beta, as it is" and
  "RATA is in beta"
  (`store-pages:rata-next/public/terms.html:207-209`; the store copy keeps
  both, checked by building it).
- The in-app help says "Your version is in the window's title bar, as
  `RATA <version> beta`" and "Open an issue with the Beta bug template"
  (`store-pages:rata-next/public/help.html:214` and `:484`, kept in the
  store copy).
- The iOS build on #166 still uses the direct interface, whose Settings say
  "Opens the Beta bug form on GitHub"
  (`ios-app:rata-next/public/app.html:1217`). #165 swaps this sentence in
  the store build (`store-page:desktop/rata-app/sync-ui.sh:216-220`), so
  this goes once #165 is in the iOS build.

**Fix**
- In the store copies, drop the "A beta, as it is" section and the
  "beta" title-bar sentence, and give the version from Settings instead.
- Give the store build its own version, 1.0.0, in `tauri.ios.conf.json`
  (`"version": "1.0.0"`), as D9 already recommends.

---

## (b) 5.2.1: other companies' marks (the Apps view)

**Apple's words** (guidelines page, as above):

> 5.2.1: "Don't use protected third-party material such as trademarks,
> copyrighted works, or patented ideas in your app without permission, and
> don't include misleading, false, or copycat representations, names, or
> metadata in your app bundle or developer name."

> 2.3.10: "don't include names, icons, or imagery of other mobile platforms
> or alternative app marketplaces in your app or metadata, unless there is
> specific, approved interactive functionality."

> 4.2.2: "Other than catalogs, apps shouldn't primarily be marketing
> materials, advertisements, web clippings, content aggregators, or a
> collection of links."

Apple's own trademark rules
(https://www.apple.com/legal/intellectual-property/guidelinesfor3rdparties.html)
say: "You may not use the Apple Logo or any other Apple-owned graphic
symbol, logo, or icon … except pursuant to an express written trademark
license from Apple." Word marks may be used "in a referential phrase".

**Is Apps in the store build? Yes.** It is in the rail and the phone's
bottom bar on every plan, with no store check
(`store-page:rata-next/public/app.html:995` and `:1250`; `renderApps` at
`:5288` has no `STORE_BUILD` test).

**Verdict: unclear, leaning to risk.** Naming Google Docs or Word as a link
is referential and normally fine. Drawing their logos is the risk.

**Evidence** (`store-page:rata-next/public/app.html:5144-5232`):
- Google Docs, Sheets, Slides, Forms and Drive use Google's own marks
  (`s:'official'`, from simple-icons; that set's CC0 licence covers the
  drawing, not the trademark).
- Word, Excel, PowerPoint, OneNote, OneDrive, Acrobat, Express, Acrobat
  Sign and DocuSign are drawn look-alikes in each brand's colour.
- Apple's Pages, Numbers and Keynote are drawn look-alikes too. Apple's
  rules above forbid Apple-owned icons "or an alteration thereof" in a
  product name; in an app Apple reviews, imitations of Apple's own app
  icons are the least safe part.
- None of the services is "Sign in with" or an approved integration on a
  phone: every tile is a web link.

**Fix (pick one)**
- Simplest: hide Apps in the iPhone build. Each of these services has its
  own iPhone app, and Apps adds little on a phone.
- Or: in the store build, draw no brand marks. Use a plain neutral glyph or
  the first letter, and keep the names as plain text ("Opens Google Docs in
  Safari").
- Do not show Apps in App Store screenshots either way (2.3.7 bans packing
  metadata with "trademarked terms").

---

## (c) A default privacy-check profile named "HIPAA"

**Apple's words** (guidelines page). No guideline names HIPAA. The ones that
apply:

> 2.3.1(a): "marketing your app in a misleading way, such as by promoting
> content or services that it does not actually offer … is grounds for
> removal of your app from the App Store".

> 2.3.7 (subtitles): "should not include inappropriate content, reference
> other apps, or make unverifiable product claims."

> 5.1.1(ix): "Apps that provide services in highly regulated fields (such
> as banking and financial services, healthcare, …) or that require
> sensitive user information should be submitted by a legal entity that
> provides the services, and not by an individual developer."

> 1.4.1 (Medical Apps) is about apps "that could be used for diagnosing or
> treating patients". RATA is not one; 1.4.1 does not apply.

**Verdict: not a stated rule breach, but a real review risk.** The app
implies a compliance it does not provide.

**Evidence** (`store-page:rata-next/public/app.html`):
- `:1496` new workspaces start with `profile:'HIPAA'`.
- `:1464` HIPAA's description is "PHI protection for covered entities".
- `:1003` the rail says "DLP profile HIPAA".
- The composer badge reads "DLP · HIPAA" and a hit says
  "SEND BLOCKED — HIPAA profile" (`:6554`, `:6685`).
- SOC2 and GDPR profiles sit beside it (`:1465-1466`).
- The demo's persona is a doctor (`main:desktop/rata-app/demo/demo-backend.js:106`
  starts in HIPAA too).
- The same app sends a message's text to Anthropic on request. Telling a
  "covered entity" its mail is HIPAA-protected while it can do that is
  exactly what a reviewer, or the FTC, would question.
- The website and help do not claim HIPAA (searched), so the claim is only
  in the interface.

**Fix**
- Start on the General profile.
- Rename the profiles for what they check, not for a law: "Health
  identifiers", "Payment and ID numbers", "EU personal data". Say plainly
  that they are a local check before sending, not compliance.
- Never put HIPAA, SOC 2 or GDPR compliance in the App Store name,
  subtitle, description or screenshots.
- If RATA ever targets healthcare, submit as a company (5.1.1(ix)); D11
  needs a legal entity anyway.

---

## (d) 3.1.2(c): what the purchase screen and the metadata must show

**Apple's words**

> 3.1.2(c) (guidelines page): "Before asking a customer to subscribe, you
> should clearly describe what the user will get for the price. … Ensure
> you clearly communicate the requirements described in Schedule 2 of the
> Apple Developer Program License Agreement."

> Schedule 2, §3.8(b)
> (https://developer.apple.com/support/terms/apple-developer-program-license-agreement/;
> Schedule 2 last updated October 13, 2026): "You clearly and conspicuously
> disclose to users the following information regarding Your auto-renewing
> subscription: Title of auto-renewing subscription, which may be the same
> as the in-app product name; Length of subscription; Price of
> subscription, and price per unit if appropriate; Links to Your Privacy
> Policy and Terms of Use must be accessible within Your Licensed
> Application."

> Subscriptions page (https://developer.apple.com/app-store/subscriptions/):
> "the following details must be included in your subscription's sign-up
> screen: Subscription name and duration, and the content or services
> provided during the subscription period; Full renewal price, shown
> clearly and prominently, and localized in available currencies; A way for
> current subscribers to sign in or restore purchases. Please note that
> your app and App Store metadata must include links to your Terms of Use
> and Privacy Policy." And: "the amount that will be billed must be the
> most prominent pricing element in the layout."

> The EULA (App Store Connect Help,
> https://developer.apple.com/help/app-store-connect/manage-app-information/provide-a-custom-license-agreement/):
> "Apple provides a standard end-user license agreement (EULA) that applies
> in all countries and regions. If you don't provide a custom EULA, the
> standard EULA is applied to your app and the license agreement link isn't
> shown on your App Store product page."

Because the standard EULA's link is not shown on the product page, the
usual practice is to link Apple's standard EULA
(https://www.apple.com/legal/internet-services/itunes/dev/stdeula/, live
today) in the App Description and on the purchase screen. That last step
comes from developers' rejection reports in Apple's forums, not from an
Apple rule I could quote; the rule I can quote is "your app and App Store
metadata must include links to your Terms of Use".

Apple's sources do not ask for a renewal sentence by name. "Auto-renewing",
"length" and the full renewal price are what is required. A sentence such
as "Renews monthly at £x until cancelled in Settings › Apple Account ›
Subscriptions" covers all three and is what reviewers expect.

**Verdict: not met.** There is no purchase screen at all.

**Evidence**
- `ios-app:docs/APP-STORES.md:194-197`: "Still to come: the purchase screen
  and Settings rows in the page ("Choose your plan", Restore purchases,
  Manage subscription)".
- No page on any branch calls `store_products`, `store_buy`,
  `store_restore` or `store_manage` (searched all five trees; nothing in
  `iap-native:desktop/rata-app/ui-src/bridge.js`).
- What is ready: StoreKit hands over `displayName`, `description`,
  `displayPrice`, the period and any introductory offer
  (`iap-native:desktop/rata-app/plugins/storekit/store/Sources/RATAStore/Store.swift:50-59`,
  `:122-142`); the Rust commands exist
  (`iap-native:desktop/rata-app/src-tauri/src/appstore.rs:749-820`) and are
  registered (`iap-native:desktop/rata-app/src-tauri/src/lib.rs:461-465`).
  No price is hard-coded. Good.
- The store build hides every price and says "… is not in your plan." with
  no way on (`store-page:rata-next/public/app.html:4326`), which was
  written for Google Play's consumption-only plan, not Apple's A1.
- `ios/RATA.storekit` has placeholder descriptions ("Everything in RATA.
  Placeholder price, for testing in Xcode only."); fine for Xcode, but the
  App Store Connect descriptions must be specific.

**Fix: the purchase screen ("Choose your plan")**
- One card per product, from StoreKit: name, "1 month", the full
  `displayPrice` as the biggest price on the card, and any intro offer's
  terms.
- What each plan gives, in plain words, true **on the iPhone**: Base "up to
  two mailboxes, translation"; Pro "more than two mailboxes, side by side,
  summaries and the briefing". Say the monthly AI allowance exists. Do not
  list Create file, connected accounts or Share to Slack, which are off on
  a phone (`ios-app:desktop/rata-app/src-tauri/src/created.rs`,
  `cloud.rs`: `cfg!(desktop)`).
- One line under the buttons: "Payment is charged to your Apple Account.
  The subscription renews each month at this price unless you cancel it at
  least 24 hours before the end of the period, in Settings › Apple Account
  › Subscriptions."
- Links: Terms of Use (EULA) and Privacy Policy, both working.
- Restore purchases on the same screen, and "Already subscribe at
  mailrata.org? Sign in" (see (e)).
- In the store build, "not in your plan" should lead to this screen, so an
  upgrade from Base to Pro is one tap (3.1.2(b): "Users should have a
  seamless upgrade/downgrade experience").

**Fix: the EULA.** Either use Apple's standard EULA and link it (purchase
screen, Settings, App Description), or paste RATA's terms into App Store
Connect as a custom EULA. RATA's terms lack the clauses of Apple's
"Instructions for Minimum Terms of Developer's End-User License Agreement"
(no "third-party beneficiary" or "Apple is not responsible" wording in
`iap-server:rata-next/public/terms.html`), so the standard EULA plus a link
to RATA's own terms is the easier path.

**Fix: App Store Connect metadata.** Privacy Policy URL (required), the
EULA choice, the Terms link in the description, a review screenshot for
each subscription, and a description of each plan in each language.

---

## (e) 3.1.1 Restore and the unlock; 3.1.3(b) multiplatform sign-in

**Apple's words** (guidelines page):

> 3.1.1: "If you want to unlock features or functionality within your app,
> (by way of example: subscriptions, …), you must use in-app purchase. Apps
> may not use their own mechanisms to unlock content or functionality, such
> as license keys, augmented reality markers, QR codes, …"

> 3.1.1: "you should make sure you have a restore mechanism for any
> restorable in-app purchases."

> 3.1.3(b) Multiplatform Services: "Apps that operate across multiple
> platforms may allow users to access content, subscriptions, or features
> they have acquired in your app on other platforms or your web site, …
> provided those items are also available as in-app purchases within the
> app."

> 3.1.3: "Apps in this section cannot, within the app, encourage users to
> use a purchasing method other than in-app purchase, except for apps on
> the United States storefront and as set forth in 3.1.1(a) and 3.1.3(a)."

> 2.1(b): "If you offer in-app purchases in your app, make sure they are
> complete, up-to-date, visible to the reviewer and functional."

> Subscriptions page: "A way for current subscribers to sign in or restore
> purchases" on the sign-up screen.

> Account-deletion page (below): "Use showManageSubscriptions in iOS 15 and
> iPadOS 15, or later, or provide this link to let people manage their
> subscriptions: https://apps.apple.com/account/subscriptions/."

### The unlock: not met (the most likely rejection)

- The store build's first screen asks for a RATA "device account"
  (email, password, name), then says "Then paste the licence key from your
  mailrata.org account, and add your mailboxes."
  (`store-page:rata-next/public/auth.html:139-147`, `:250`).
- With no licence, a modal titled "Your licence key" with one input and
  "Use this licence" covers the app, and its note says "Paste the licence
  key from your mailrata.org account."
  (`store-page:desktop/rata-app/ui-src/bridge.js:1181-1203`). That is the
  only unlock. #172 adds StoreKit but changes no page, so the iOS build
  still shows this box.
- Settings, with no licence: "No licence on this computer yet. Paste the
  licence key from your mailrata.org account."
  (`store-page:rata-next/public/app.html:6051`).
- Several refusals send the customer to "Paste a new licence key from your
  mailrata.org account" (`store-page:desktop/rata-app/ui-src/bridge.js:1036-1039`).
- Outside the US, a box that can only be filled by buying on mailrata.org
  is also an in-app push to another purchase method (3.1.3).

**Fix**
- In the iPhone build, never show the key box. With no licence, show the
  purchase screen: the two plans, Restore purchases, and "Sign in with your
  mailrata.org account" for people who already subscribe there.
- Website subscribers sign in with their mailrata.org email and password,
  in the app, through a store-safe endpoint that answers with a licence.
  That is 3.1.3(b)'s "access … subscriptions … acquired … on your web
  site". The Ed25519 licence can stay as the internal token; it just must
  not be something the customer types.
- Words on the iPhone never mention a key: "Sign in to use the plan you
  have at mailrata.org", not "paste".
- Split the store flavour into Apple and Google: Google Play (G1) sells
  nothing in the app; Apple (A1) must sell. One `store` flavour cannot be
  both.

### Restore: not met in the page, ready underneath

- Rust and Swift have it: `AppStore.sync()` then the current entitlements
  (`iap-native:…/Store.swift:285-295`), `store_restore`
  (`iap-native:desktop/rata-app/src-tauri/src/appstore.rs:777-790`).
- No button calls it. The help on #167 already tells people to "Press
  **Restore purchases**" (`iap-server:rata-next/public/help.html:228`,
  `:243`, `:250`), so merging that help before the button would make the
  help describe a button that is not there (2.3).

**Fix:** a Restore purchases button on the purchase screen and in
Settings › Plan, only on the customer's tap (as the Swift comment already
says).

### Manage subscription: not met in the page, ready underneath

- `AppStore.showManageSubscriptions(in:)` is wired
  (`iap-native:desktop/rata-app/plugins/storekit/ios/Sources/StoreKitPlugin.swift:107-114`).
- The store build's Settings say "Your plan is managed in your
  mailrata.org account." (`store-page:rata-next/public/app.html:1224`).
  That is wrong for an App Store subscriber.

**Fix:** for an `apple:` licence, Settings › Plan shows the plan, its
renewal date and **Manage subscription** (Apple's sheet). Only for a
website licence say "managed at mailrata.org", with no link outside the US
storefront.

### 3.1.3(b) wording: met in plan, not in code

- Both plans the website sells, Base and Pro, are the two IAP products
  (`iap-native:desktop/rata-app/src-tauri/src/appstore.rs:56-59`;
  `iap-server:rata-next/lib/plan.js:163`). Enterprise and the domain
  add-on are not on sale (`sellable: false`, `DOMAIN_ADDON` "not on sale"),
  so nothing sold on the web is missing from IAP. Good.
- A hand-minted Enterprise licence (`mint.sh` takes any plan) would unlock
  on the iPhone something not sold there. Keep the iPhone build to Base and
  Pro licences, or mint no Enterprise licences.
- The path itself is the licence key, which (above) is the problem.

### The iOS build is not the store flavour yet

- #166's iOS jobs run `./sync-ui.sh` with no `RATA_DISTRIBUTION`
  (`ios-app:.github/workflows/ios.yml:117`, `:310`, `:525`; `iap-native`
  the same at `:104`, `:330`, `:517`), and #166's `sync-ui.sh` has no
  flavours. Its config points at `https://mailrata.org/#pricing` and the
  billing portal (`ios-app:desktop/rata-app/sync-ui.sh:96-98`), and its
  page says "… comes with RATA Pro, $23.99 a month. Upgrade in Settings."
  (`ios-app:rata-next/public/app.html:4334`) and shows "Manage billing"
  (`:1206`).
- #165 does hide all of that in a store build: no prices, Stripe or portal
  (`store-page:rata-next/public/app.html:6044-6052`), no update rows
  (`:5601-5628`, `:8547`), no update routes
  (`store-page:desktop/rata-app/ui-src/bridge.js:999-1002`), and
  store-safe links swapped in the markup
  (`store-page:desktop/rata-app/sync-ui.sh:96-117`, `:209-240`).

**Fix:** after #165 merges, the iOS jobs must build with
`RATA_DISTRIBUTION=store` (or the new Apple flavour), and `check-app.sh`
should fail an iOS build whose `config.js` is not the store one.

---

## (f) 5.1.1(v): account deletion when App Store buyers have no account

**Apple's words**

> 5.1.1(v) (guidelines page): "If your app doesn't include significant
> account-based features, let people use it without a login. If your app
> supports account creation, you must also offer account deletion within
> the app. Apps may not require users to enter personal information to
> function, except when directly relevant to the core functionality of the
> app or required by law."

> Reference page
> (https://developer.apple.com/help/app-review/guideline-reference/5-1-1-account-deletion):
> - "If people need to visit a website to finish deleting their account,
>   include a link directly to the page on your website where they can
>   complete the process."
> - "If my app links out to the default web browser for account creation,
>   does it still need to offer account deletion within the app? Yes."
> - "If a person has an auto-renewable subscription, notify them that
>   billing will continue through Apple and ask them to cancel their
>   subscription before continuing."
> - "you can offer to schedule account deletion for a later date, aligned
>   with the subscription's expiration, as long as there's also an option
>   to delete the account immediately."
> - "Apps not operating in highly regulated industries should not require
>   people to make a phone call, send an email, use a chat support flow, or
>   go through other support flows."

### Does it apply to an App Store buyer with no account?

On mailrata.org, no. The rule is triggered by account **creation**, and an
App Store buyer creates none there (`iap-server:rata-next/public/privacy.html`,
"You need no account at mailrata.org to subscribe there"). Nothing on the
server names them, so there is nothing to delete there.

**But the iPhone app does create an account:** the "device account"
(email, password, name, optional company) on its first screen
(`store-page:rata-next/public/auth.html:101-147`). That triggers both
halves of 5.1.1(v).

**Verdict**
- In-app deletion of the device account: **met.** Settings › Delete
  account removes every mailbox, keychain entry, the licence and the local
  store (`store-page:rata-next/public/app.html:6178-6265`, through
  `forget_everything`).
- "Let people use it without a login" and "may not require users to enter
  personal information": **not met.** The device account gates the app but
  carries no account features; it is a local profile. Reviewers reject
  sign-up walls like this.
- Telling an App Store subscriber that Apple billing continues: **not
  met.** The delete wording says "Your mailrata.org account and
  subscription are managed and deleted at mailrata.org."
  (`store-page:rata-next/public/app.html:6170`), which is wrong for an
  `apple:` subscriber, and nothing offers Apple's subscription sheet.

**Fix**
- Drop the device-account step in the iPhone build. Make the profile
  silently on first run (no email or password) and go straight to "Add a
  mailbox". The mailbox's own sign-in is the third-party login 5.1.1(v)
  and 4.8 allow.
- Keep Delete account (call it "Remove RATA's data from this iPhone"). For
  an `apple:` licence, say first: "Your App Store subscription keeps
  billing through Apple. Cancel it first." with a **Manage subscription**
  button (`store_manage`).

### mailrata.org accounts signed in from the app

Once website subscribers can sign in from the app (see (e)), Apple's FAQ
above means in-app deletion is expected too, and a direct link is enough.

**Verdict: partly met.**
- The store build shows "Delete your mailrata.org account", linking
  straight to `https://mailrata.org/in-app/delete-account`
  (`store-page:rata-next/public/app.html:1238`, `:5674`, `:6176-6177`;
  `store-page:desktop/rata-app/sync-ui.sh:114`). Good.
- That page is not live (404 today).
- It refuses deletion "while a subscription is still live" and for 30 days
  after it ends (`store-pages:rata-next/lib/account.js:43-71`;
  `store-pages:rata-next/public/in-app/delete-account.html:120`). Apple
  allows a scheduled deletion only "as long as there's also an option to
  delete the account immediately". This is D4, still open.
- Its fallback is "write to [OWNER: contact email]"
  (`store-pages:rata-next/public/in-app/delete-account.html:133`): a
  placeholder, and an email flow Apple says not to require.

**Fix:** decide D4 as the plan recommends: delete at once and revoke the
account's licences (or schedule it, with a "delete now" choice). Deploy the
page. Fill in the contact address.

---

## (g) 4.8: Sign in with Apple

**Apple's words** (guidelines page):

> 4.8: "Apps that use a third-party or social login service … to set up or
> authenticate the user's primary account with the app must also offer as
> an equivalent option another login service …"

> "Another login service is not required if: Your app exclusively uses your
> company's own account setup and sign-in systems. … Your app is a client
> for a specific third-party service and users are required to sign in to
> their mail, social media, or other third-party account directly to access
> their content."

**Verdict: met.** Sign in with Microsoft is for a mailbox, which is the
mail-client exception. On the iPhone it is off anyway: the Microsoft,
Google and Slack client ids are `None` on a phone
(`ios-app:desktop/rata-app/src-tauri/src/oauth.rs`, `google.rs`,
`slack.rs`: `crate::desktop_only(...)`). The mailrata.org sign-in is the
company's own system. It stays met as long as no Google or Microsoft
sign-in ever creates the RATA account itself.

---

## (h) 2.1: demo access for review

**Apple's words** (guidelines page):

> 2.1(a): "include demo account info (and turn on your back-end service!)
> if your app includes a login. If you are unable to provide a demo account
> due to legal or security obligations, you may include a built-in demo
> mode in lieu of a demo account with prior approval by Apple."

> "Make sure your app has been tested on-device for bugs and stability
> before you submit it".

> Before You Submit: "Provide App Review with full access to your app. If
> your app includes account-based features, provide either an active demo
> account or fully-featured demo mode, plus any other hardware or resources
> that might be needed to review your app".

> 2.3.1(a): "All new features, functionality, and product changes must be
> described with specificity in the Notes for Review section".

App Store Connect Help
(https://developer.apple.com/help/app-store-connect/manage-submissions-to-app-review/submit-an-in-app-purchase/;
read through search results, not fetched whole): the first subscription
group must be submitted with a new app version, with a subscription in
that same submission.

**Verdict: not met yet; the plan is right.** What a reviewer needs:

1. **A mailbox.** Notes must give a dedicated test mailbox, its address and
   app password, filled with harmless mail. Not Outlook.com or Microsoft
   365: those cannot be added on the phone (Microsoft sign-in is off,
   above), so a reviewer given one would see "cannot be added to RATA yet".
   A Fastmail or other IMAP host with app passwords is safest.
2. **A plan.** Reviewers buy in the sandbox. #167 accepts sandbox purchases
   on a production server ("App Review and TestFlight buy in the sandbox",
   `iap-server:rata-next/lib/apple.js:186-208`). A sandbox licence lasts
   the period plus one hour (`iap-server:rata-next/public/terms.html:161`).
   Apple's sandbox renews a monthly subscription every 5 minutes, at most
   12 times (App Store Connect Help, "Manage Sandbox Apple Account
   settings", via search). So a reviewer who comes back hours later finds
   RATA locked. That is fine once the purchase screen exists: they can
   subscribe again. Today it would show the key box.
3. **Do not hand the reviewer a licence key.** The plan says "a reviewer's
   licence (`mint.sh`)" (`ios-app:docs/APP-STORES.md:91`). Asking a
   reviewer to paste a key shows them the banned unlock. Let them buy in
   the sandbox instead, or, if a minted licence is needed, deliver it
   without a key box.
4. **The back end on:** `/api/licence/apple`, `/api/ai`,
   `/api/licence/renew` and the App Store Server Notifications URL. Today
   mailrata.org is the 2026-09-11 deploy and `/api/health` is 404.
5. **Notes for Review**: the AI relay (what is sent, to whom), what is off
   on the phone and why, and where Privacy, Terms, Restore and Delete are.
6. **Tested on a device**: `ios-app:docs/APP-STORES.md:5` says "Nothing
   here has run on a phone yet."

There is no built-in demo mode in the iPhone app (the clickable demo is a
web build, `desktop/rata-app/demo/`). A demo mode would need Apple's prior
approval; the test mailbox is the better route.

---

## (i) 5.1.2(i): consent before mail goes to a third-party AI

**Apple's words** (guidelines page):

> 5.1.2(i): "You must clearly disclose where personal data will be shared
> with third parties, including with third-party AI, and obtain explicit
> permission before doing so."

> 5.1.1(ii): "Apps must also provide the customer with an easily accessible
> and understandable way to withdraw consent."

> 5.1.1(i): the privacy policy must "Confirm that any third party with
> whom an app shares user data … will provide the same or equal protection
> of user data as stated in the app's privacy policy", and "Explain its
> data retention/deletion policies and describe how a user can revoke
> consent".

> App Privacy Details (https://developer.apple.com/app-store/app-privacy-details/):
> "'Collect' refers to transmitting data off the device in a way that
> allows you and/or your third-party partners to access it for a period
> longer than what is necessary to service the transmitted request in real
> time."

**Verdict: words partly met; mechanism not met on an iPhone.**

**Evidence**
- The notice names Anthropic and what is sent: "Summaries, translation and
  task flags are done by RATA's AI service, run by Anthropic. The text of
  the message — or, for the briefing, the start of your recent mail — is
  sent to it to be answered, and RATA does not keep it. Carry on?"
  (`store-page:rata-next/public/app.html:8302`).
- It is asked with the browser's `confirm()` (`:8305`). #165's own note
  says WKWebView "answers the one with Cancel" (`:2070-2073`), and #165
  converted every other `confirm()` but this one. On an iPhone, then, AI
  can never be turned on: Summarize and Translate, a paid feature, fall
  back to "Made on this computer, without AI" (`:8283`). That is a
  functional failure under 2.1, and a paid feature that does not work.
- There is no way to withdraw: `S.settings.aiOk` is set once (`:8306`)
  and nothing in Settings clears it.
- "RATA's AI service, run by Anthropic" is not quite true: RATA runs the
  relay; Anthropic runs the model.
- The privacy policy says "Anthropic handles the text under its own terms
  and privacy policy" (store copy built from
  `store-pages:rata-next/public/privacy.html`). It does not confirm "the
  same or equal protection", and does not say what Anthropic keeps (D5).
- #161 (`claude/assist-v1`, not in the iOS line) has a better consent, per
  mailbox, with a "Use RATA's AI" switch in Settings
  (`origin/claude/assist-v1:rata-next/public/app.html:8281-8282`, `:8390`).

**Fix**
- Use #161's in-page consent in the iPhone build, or at least turn this
  `confirm()` into #165's `ask()`. Name it: "Send this message to
  Anthropic's AI model through RATA's relay?", with "Send" and "Don't
  send".
- A Settings switch to turn AI off, which also clears the consent.
- Settle D5, then state Anthropic's retention, and say in the privacy
  policy that Anthropic protects the data at least as the policy says.
- App Privacy label: if Anthropic keeps the text past the request (D5),
  that is "collected" under Apple's definition, as "Emails or Text
  Messages" or "Other User Content", even though RATA keeps none.

---

## (j) What changed at Apple in 2026 that matters here

From https://developer.apple.com/news/ (read today):

- **Guidelines, October 16, 2026** (https://developer.apple.com/news/?id=92th7e5i):
  - New **2.5.19**: "Don't include any hidden, dormant, or undocumented
    features in your app; your app's functionality should be clear to end
    users and App Review." Apple's note (https://developer.apple.com/news/?id=vvzx6cul)
    says it targets "adding features or functionality that weren't included
    in the version of the app that was submitted to App Review".
    **RATA: met, with care.** Features off on a phone are off at compile
    time (`cfg!(desktop)`), the `smoke` self-test is refused in a release
    build (`ios-app:.github/workflows/ios.yml:139`), and nothing is
    switched on from a server. Keep it so: never turn on connected
    accounts or Create file on iOS by a licence field or server flag
    without a new review, and list in Notes what is off on the phone.
  - 3.1.1(a) clarified ("links may be provided to a permitted
    destination"). Matters only if the owner takes option A2.
  - New 2.3.14 (creative assets): no "specific or discounted pricing
    information" in them.
- **DPLA, October 13, 2026** (https://developer.apple.com/news/?id=17mwd36z):
  Schedule 2 §3.8(c) now covers Group Purchases.
- **Multiseat subscriptions** (September 16 and October 22, 2026): "Starting
  today, multiseat purchases are enabled by default for subscriptions in
  App Store Connect." Volume Purchasing launched October 22; Group
  Purchases "this winter". **RATA: risk.** #167 handles Family Sharing
  (`iap-server:rata-next/lib/apple.js:345`) but has no word of multiseat
  or group purchases, and §3.8(c) says "You must fulfill the offer during
  the entire subscription period". **Fix:** turn multiseat off in each
  subscription's Purchase Options until the server handles it.
- **Age rating, social media** (July 9, 2026): "beginning in September 2026,
  responses will be required when submitting new apps or updates". RATA
  has no social feed: answer No.
- **SDKs**: built with Xcode 26 and the iOS 26 SDK since April 28, 2026;
  the iOS 27 SDK from April 2027. "Since September 9, 2026 … must target
  iOS 13 or later" (RATA targets 16.4: fine). "Starting April 2027, any
  apps or games submitted will need to include screenshots for iPhone
  Duo."
- **Texas SB 2420** (June 3, 2026): age assurance and parental consent for
  minors' downloads and IAP; "Developers can request age category data …
  through the Declared Age Range API". Still the lawyer's question in
  APP-STORES.md.
- **EU** (August 18, 2026): from October 1, 2026, "Allowing apps to offer
  alternative payment options alongside Apple In-App Purchase". Optional.

---

## Other checks on the branches

| Topic | Apple's words | Verdict | Evidence | Fix |
|---|---|---|---|---|
| Live links and placeholders | 2.1(a), quoted in (a); 5.1.1(i): "All apps must include a link to their privacy policy in the App Store Connect metadata field and within the app in an easily accessible manner." | **Not met** | Live `mailrata.org/privacy.html`, `/terms.html`, `/help`, `/in-app/*`: 404 today. Store copies carry `[OWNER: …]`: privacy 3, terms 5, help 1 (`store-pages:rata-next/public/terms.html:127,153,186,204,206`; same in `iap-server`). | Settle D11, fill every placeholder, deploy #162 and #167, run `scripts/live-check.sh`. |
| Privacy and Terms in the app | 5.1.1(i) as above | **Met in #165** | Settings rows "Privacy policy" and "Terms of service", swapped to `/in-app/…` in the store build (`store-page:rata-next/public/app.html:1236-1237`; `sync-ui.sh:221-222`). | Add both links to the purchase screen. |
| What the privacy policy says | 5.1.1(i): "Identify what data, if any, the app/service collects" | **Partly met** | #167 adds an iPhone section and the App Store record (`iap-server:rata-next/public/privacy.html`). #162's store copy is built from main's page, which says "RATA is a desktop mail client" and "on your computer" throughout. | Merge #167's words before #162's copy is made; describe the iPhone app as well. |
| Self-update | 2.5.2: apps may not "download, install, or execute code which introduces or changes features" | **Met** | iOS builds use `--no-default-features` (no `self-update`): `ios-app:.github/workflows/ios.yml:122,558`; `"updater": null` in `tauri.ios.conf.json:17`; the store page drops update rows and routes (above). | Keep `RATA_DISTRIBUTION=store` in the iOS build, so the page's update row does not offer GitHub releases. |
| Background modes | 2.5.4: "Multitasking apps may only use background services for their intended purposes" | **Met** | No `UIBackgroundModes` in `prepare.sh` or anywhere (searched). IDLE runs only while RATA is in front. | If D7 adds `BGAppRefreshTask`, declare only `fetch` and use it only for mail. |
| Privacy manifest | Required-reason APIs need an approved reason (upcoming-requirements page) | **Met as far as I can tell** | `ios-app:desktop/rata-app/ios/PrivacyInfo.xcprivacy:21-39`: no tracking, File Timestamp `C617.1`. `required-reasons.sh` checks the device build's symbols in CI. `NSPrivacyCollectedDataTypes` is empty "once given". | Fill collected types to match the App Privacy answers (likely Purchases for the App Store record; Email or Other User Content if Anthropic keeps text; Email for signed-in website users). I could not see whether #172's Swift adds a required-reason API; its CI would say. |
| Encryption export | App Store Connect Help (https://developer.apple.com/help/app-store-connect/reference/app-information/export-compliance-documentation-for-encryption): "Your app uses an industry standard algorithm, not provided within the Apple operating system: Upload your French encryption declaration … only required if you're distributing your app on the App Store in France." | **Met in approach** | `ios-app:desktop/rata-app/ios/prepare.sh:27-54,156-160` leaves `ITSAppUsesNonExemptEncryption` out until France is decided. The reading matches Apple's table. | Owner decides France, then set the key. |
| Camera and microphone | iOS ends an app that uses them without purpose strings | **Met** | `prepare.sh:161-164`. | None. |
| Links in formatted mail | 2.1(a): "We will reject … binaries that crash or exhibit obvious technical problems." | **Not met** | A tap on a link in a formatted message "does nothing yet" on a phone (`ios-app:desktop/rata-app/src-tauri/src/lib.rs:374-388`, `TODO(STAGE 3 item 6)`). | Do item 6 before submitting; reviewers tap links. |
| On-device testing | 2.1(a): "Make sure your app has been tested on-device" | **Not met** | `ios-app:docs/APP-STORES.md:5`. Also untested: iPhone app in iPad compatibility mode, an IPv6-only network (2.5.5). | TestFlight on real iPhones, an iPad, and Apple's NAT64 test network. |
| Mailbox Microsoft users | 2.3.1(a), quoted in (c) | **Watch** | Microsoft mailboxes cannot be linked on a phone. | Do not claim Outlook or Microsoft 365 support in the iPhone listing. |
| Other platforms named | 2.3.10, quoted in (b) | **Minor** | The deletion page says "the App Store or Google Play" (`store-pages:rata-next/public/in-app/delete-account.html`). | Say "the App Store" in Apple's copy. |
| "Center Point inbox" | 2.3.8: "ensure your metadata … are similar to avoid creating confusion" | **Minor** | The store plan line says "in one Center Point inbox" (`store-page:rata-next/public/app.html:6050`). | Say "in one inbox"; one product name everywhere. |

---

## What would most likely cause a rejection, in order

1. **3.1.1 / 2.1(b): no IAP to see, and a licence-key box as the only
   unlock.** Certain. Build the purchase screen with Restore, Manage
   subscription and a mailrata.org sign-in; never show the key box on an
   iPhone.
2. **2.1(a) / 5.1.1(i): dead links and placeholders.** Certain while the
   privacy policy and terms are 404 and carry `[OWNER: …]`, and while the
   licence, AI and purchase servers are not deployed.
3. **The iOS build is not the store build.** If #166 were submitted as it
   is, it would show prices, "Upgrade", the Stripe billing portal and
   GitHub links. Merge #165 and build iOS with `RATA_DISTRIBUTION=store`.
4. **3.1.2(c): the purchase screen's required content.** Name, length,
   full renewal price (biggest), what each plan gives on the iPhone,
   renewal wording, and working Terms of Use (EULA) and Privacy links; the
   same links in the App Store metadata.
5. **5.1.1(v): a sign-up wall.** A local account with email, password and
   name before anything works. Drop it on the iPhone. Also tell App Store
   subscribers, before deleting, that Apple billing continues.
6. **5.1.2(i) / 5.1.1(ii) / 2.1: AI consent.** `confirm()` is
   auto-cancelled on iOS, so the paid AI features never work there, and
   consent cannot be withdrawn. Use an in-page consent and a Settings
   switch; fix the privacy policy's Anthropic sentence (D5).
7. **2.2: "beta".** "A beta, as it is" in the in-app terms, "RATA
   `<version>` beta" and "Beta bug template" in the in-app help. Remove
   them and ship 1.0.0.
8. **2.1(a): obvious problems.** Links in formatted mail do nothing on a
   phone; nothing has run on a device yet.
9. **5.2.1: brand logos in Apps.** Google's official marks and look-alikes
   of Microsoft's, Adobe's and Apple's app icons. Hide Apps on the iPhone
   or draw no logos.
10. **2.3.1 / 5.1.1(ix): "HIPAA".** Default to General, rename the
    profiles, and claim no compliance anywhere.
11. **5.1.1(v) and D4: the 30-day deletion block** on mailrata.org
    accounts, once they can sign in from the app.
12. **Not a rejection, but a money risk: multiseat** is on by default.
    Turn it off until the server handles group and volume purchases.

## Where I could not tell

- Whether Apple's reviewers will accept drawn look-alike logos (b): Apple
  publishes no rule for other companies' marks beyond 5.2.1's general one.
- The exact App Store Connect wording on the first subscription group and
  sandbox renewal rates: read through Apple's search results, not fetched
  whole.
- The standard-EULA link in the description is developers' reported
  practice; Apple's quotable rule is "your app and App Store metadata must
  include links to your Terms of Use and Privacy Policy".
- CI status of the five pull requests, and anything that only shows on a
  device (StoreKit sheets, the Local Network prompt, iPad layout).
