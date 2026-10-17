//! Buying RATA in the iPhone app: In-App Purchase, through StoreKit 2.
//!
//! App Store rule 3.1.1 lets an iPhone app sell a paid plan only through
//! Apple's In-App Purchase, never by a licence key. So RATA for iOS buys and
//! restores its subscriptions with StoreKit (the Swift plugin in
//! `plugins/storekit`), and mailrata.org turns what StoreKit says the
//! customer is entitled to into RATA's ordinary licence
//! (`/api/licence/apple`, which checks Apple's signature on each
//! transaction). The licence is the one `licence.rs` checks offline, and it
//! is kept the way the licence box keeps one, through `Rata::set_licence`:
//! a key that does not work never replaces one that does (`outweighs`), and
//! nothing begun before a Delete account is kept after it (the `epoch`).
//!
//! A purchase, a restore, a renewal and each transaction StoreKit hands over
//! by itself (`Transaction.updates`: a renewal, an Ask to Buy approved, a
//! purchase on another device) go the same way, [`AppStore::licence`]:
//!
//! 1. what StoreKit verified, each as Apple signed it, goes to
//!    `/api/licence/apple` over HTTPS with the mail connections' TLS, as
//!    `oauth.rs` calls out (20 seconds, no redirects, no cookies);
//! 2. the licence that comes back is kept through `set_licence`;
//! 3. only then is each transaction sent finished, so a purchase whose
//!    licence did not arrive, or was not kept, is handed over again by
//!    StoreKit at the next launch;
//! 4. the page is told with `rata-standing`, as bridge.js's renewal tells it.
//!
//! A refusal from mailrata.org keeps whatever licence RATA has and says the
//! server's sentence. Nothing here is a mail credential or mail, and nothing
//! personal reaches mailrata.org: Apple's transaction ids, the product and
//! its dates, which the server keeps (database.sql §7) and no more.
//!
//! On iOS RATA also renews this way by itself, quietly: at launch and every
//! six hours while its licence is due ([`due`], bridge.js's own rule), and on
//! each transaction from the listener. When StoreKit has nothing, nothing
//! changes, and bridge.js's renewal through `/api/licence/renew` stands as
//! it is on a computer.
//!
//! The page asks through the `store_*` commands, which answer
//! `{ available: false }` on a computer and on Android: neither is built
//! with the plugin (Cargo.toml), and neither sells through Apple.

// Off iOS only the commands' answers and the tests use this module; the rest
// is RATA for iOS's.
#![cfg_attr(not(target_os = "ios"), allow(dead_code))]

use std::future::Future;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::core::{Rata, Standing};

/// The App Store's products, each with the plan it sells, highest first:
/// one subscription group, "RATA", where Pro is level 1 and Base level 2, so
/// that Apple handles Base to Pro as an upgrade. `APPLE_PRODUCTS` in
/// rata-next/lib/plan.js holds the same pairs (a test here reads it), and
/// ios/RATA.storekit the same products for testing in Xcode.
pub const PRODUCTS: [(&str, &str); 2] = [
    ("org.mailrata.pro.monthly", "pro"),
    ("org.mailrata.base.monthly", "base"),
];

/// Where StoreKit's transactions become a licence.
pub const LICENCE_URL: &str = "https://mailrata.org/api/licence/apple";

/// How long mailrata.org may take to answer, as bridge.js gives a renewal.
const WAIT: Duration = Duration::from_secs(20);

/// At most this many transactions in one request, each at most
/// [`JWS_MOST`] long: mailrata.org refuses more (`APPLE_MAX_TRANSACTIONS`,
/// `APPLE_MAX_JWS` in rata-next/lib/apple.js).
const SENT_MOST: usize = 10;
const JWS_MOST: usize = 16 * 1024;

/// The most of mailrata.org's answer that is read: a licence and a sentence
/// are a few hundred bytes.
const ANSWER_MOST: usize = 64 * 1024;

/// How often RATA for iOS looks at its licence while it runs, as bridge.js
/// does (`RENEW_EVERY` there).
pub const RENEW_EVERY: Duration = Duration::from_secs(6 * 3600);

/// Said when StoreKit lists nothing RATA sells, in mailrata.org's words for
/// the same thing (`APPLE_SAID.nothingLive`).
pub const NOTHING: &str = "There is no active RATA subscription on this Apple account.";
/// Said when mailrata.org could not be reached, or answered with nothing it
/// meant.
pub const UNREACHABLE: &str = "RATA could not reach mailrata.org to turn your subscription into a licence. The subscription is safe with Apple, and RATA tries again by itself.";
/// Said when StoreKit could not verify what the App Store answered.
pub const UNVERIFIED: &str = "The App Store's answer about this purchase could not be verified, so RATA did not use it. Try Restore purchases.";
/// Said for a plan the App Store does not sell.
pub const NO_SUCH_PLAN: &str = "The App Store does not sell that plan.";
/// Said for a purchase StoreKit ended without its own sentence.
pub const NOT_BOUGHT: &str = "The App Store could not complete the purchase.";

/// The plan a product sells, if RATA sells it.
pub fn plan_of(product: &str) -> Option<&'static str> {
    PRODUCTS
        .iter()
        .find(|(id, _)| *id == product)
        .map(|(_, plan)| *plan)
}

/// The product that sells a plan.
pub fn product_for(plan: &str) -> Option<&'static str> {
    PRODUCTS.iter().find(|(_, p)| *p == plan).map(|(id, _)| *id)
}

// ------------------------------------------------------------- StoreKit

/// One transaction StoreKit verified, as the Swift side hands it over:
/// Apple's signed record of it (`jws`), which goes to mailrata.org, and the
/// id it is finished by. The Swift side also says its original id and
/// whether it was revoked, which mailrata.org reads from the record itself.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Signed {
    pub id: String,
    pub product: String,
    pub jws: String,
}

impl Signed {
    /// Whether this is worth sending: a product RATA sells, an id of
    /// Apple's kind (digits), and a signed record of a JWS's shape and size.
    fn worth_sending(&self) -> bool {
        plan_of(&self.product).is_some()
            && (1..=20).contains(&self.id.len())
            && self.id.bytes().all(|b| b.is_ascii_digit())
            && (1..=JWS_MOST).contains(&self.jws.len())
            && self.jws.matches('.').count() == 2
            && self
                .jws
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    }
}

/// A subscription period: `unit` is day, week, month or year.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Period {
    pub unit: String,
    pub value: u32,
}

/// An introductory offer the customer can still take.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Intro {
    pub display_price: String,
    pub period: Period,
    pub period_count: u32,
    /// free-trial, pay-as-you-go or pay-up-front.
    pub payment_mode: String,
}

/// A product as StoreKit lists it, with the price in the customer's own
/// currency: RATA never has a price of its own on iOS.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Listed {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub display_price: String,
    #[serde(default)]
    pub period: Option<Period>,
    #[serde(default)]
    pub intro: Option<Intro>,
}

/// A product with the plan it sells, as the page gets it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Offer {
    pub plan: &'static str,
    #[serde(flatten)]
    pub product: Listed,
}

/// How a purchase ended, as StoreKit said. Only `Bought` carries a
/// transaction, and only a verified one: an unverified answer is never
/// taken as bought.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "outcome", rename_all = "kebab-case")]
pub enum Purchase {
    Bought {
        transaction: Signed,
    },
    /// Ask to Buy, or a payment that needs the customer's action first. The
    /// transaction comes later through `Transaction.updates`.
    Pending,
    Cancelled,
    Failed {
        #[serde(default)]
        message: Option<String>,
    },
    Unverified,
}

#[derive(Deserialize)]
struct Products {
    products: Vec<Listed>,
}

#[derive(Deserialize)]
struct Entitled {
    transactions: Vec<Signed>,
}

#[derive(Deserialize)]
struct Restored {
    cancelled: bool,
    transactions: Vec<Signed>,
}

#[derive(Deserialize)]
struct Finished {
    finished: bool,
}

/// One command to the Swift plugin, answered as JSON: the plugin's handle
/// (`run_mobile_plugin_async`) on iOS, a stand-in StoreKit in the tests. An
/// error is StoreKit's own sentence.
pub trait Call: Send + Sync + 'static {
    fn call(
        &self,
        command: &'static str,
        payload: Value,
    ) -> impl Future<Output = Result<Value, String>> + Send;
}

/// StoreKit, as RATA asks it.
struct Kit<C> {
    call: C,
}

fn read<T: serde::de::DeserializeOwned>(answer: Value) -> Result<T, String> {
    serde_json::from_value(answer).map_err(|_| "The App Store's answer could not be read.".into())
}

impl<C: Call> Kit<C> {
    async fn products(&self, ids: &[&str]) -> Result<Vec<Listed>, String> {
        let answer = self.call.call("products", json!({ "ids": ids })).await?;
        read::<Products>(answer).map(|p| p.products)
    }

    async fn purchase(&self, id: &str) -> Result<Purchase, String> {
        read(self.call.call("purchase", json!({ "id": id })).await?)
    }

    async fn entitlements(&self) -> Result<Vec<Signed>, String> {
        let answer = self.call.call("entitlements", json!({})).await?;
        read::<Entitled>(answer).map(|e| e.transactions)
    }

    async fn restore(&self) -> Result<Restored, String> {
        read(self.call.call("restore", json!({})).await?)
    }

    async fn manage(&self) -> Result<(), String> {
        self.call.call("manage", json!({})).await.map(|_| ())
    }

    async fn finish(&self, id: &str) -> Result<bool, String> {
        let answer = self.call.call("finish", json!({ "id": id })).await?;
        read::<Finished>(answer).map(|f| f.finished)
    }

    async fn listen(&self, handler: Value) -> Result<(), String> {
        self.call
            .call("listen", json!({ "handler": handler }))
            .await
            .map(|_| ())
    }
}

/// A transaction the listener sent over its channel, if that is what the
/// message is.
pub fn update_in(body: &tauri::ipc::InvokeResponseBody) -> Option<Signed> {
    match body {
        tauri::ipc::InvokeResponseBody::Json(json) => serde_json::from_str(json).ok(),
        tauri::ipc::InvokeResponseBody::Raw(_) => None,
    }
}

// ----------------------------------------------------------- mailrata.org

/// What `/api/licence/apple` answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answered {
    /// A licence, and the server's sentence about it.
    Licence {
        token: String,
        message: Option<String>,
    },
    /// No licence, with the server's reason and its sentence: nothing live,
    /// a purchase it could not verify, App Store purchases not switched on
    /// yet. The same shape `/api/licence/renew` refuses with.
    Refused { reason: String, message: String },
    /// Not reached, or an answer with nothing meant in it.
    Unreachable,
}

/// mailrata.org's `/api/licence/apple`.
pub struct Mailrata {
    url: String,
    http: reqwest::Client,
    wait: Duration,
}

impl Mailrata {
    /// The real one, through any proxy the system has, as Sign in with
    /// Microsoft reaches Microsoft.
    pub fn production() -> Result<Self, String> {
        Mailrata::at(LICENCE_URL, true)
    }

    /// The same at `url`: a stand-in, in the tests.
    pub fn at(url: &str, use_proxy: bool) -> Result<Self, String> {
        Ok(Mailrata {
            url: url.into(),
            http: crate::oauth::client_for(use_proxy, "mailrata.org")?,
            wait: WAIT,
        })
    }

    /// The transactions, as Apple signed them, for a licence.
    pub async fn licence_for(&self, transactions: &[String]) -> Answered {
        let body = json!({ "transactions": transactions }).to_string();
        let sent = self
            .http
            .post(&self.url)
            .timeout(self.wait)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .body(body)
            .send()
            .await;
        let Ok(mut answer) = sent else {
            return Answered::Unreachable;
        };
        let ok = answer.status().is_success();
        let mut got = Vec::new();
        loop {
            match answer.chunk().await {
                Ok(Some(chunk)) => {
                    got.extend_from_slice(&chunk);
                    if got.len() > ANSWER_MOST {
                        return Answered::Unreachable;
                    }
                }
                Ok(None) => break,
                Err(_) => return Answered::Unreachable,
            }
        }
        answered(ok, &got)
    }
}

/// What an answer from `/api/licence/apple` says, read as bridge.js reads
/// `/api/licence/renew`'s: a reason with a sentence is a real answer,
/// whatever the status, and is read first, so nothing else in it is used; a
/// licence counts only in a successful answer that says it is licensed;
/// anything else meant nothing.
fn answered(ok: bool, body: &[u8]) -> Answered {
    let Ok(said) = serde_json::from_slice::<Value>(body) else {
        return Answered::Unreachable;
    };
    let text = |key: &str| {
        said.get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
    };
    let licensed = said.get("licensed") == Some(&Value::Bool(true));
    if !licensed {
        return match (text("reason"), text("message").or_else(|| text("error"))) {
            (Some(reason), Some(message)) => Answered::Refused {
                reason: reason_named(reason),
                message: sentence(message),
            },
            _ => Answered::Unreachable,
        };
    }
    match (ok, text("licence")) {
        (true, Some(token)) => Answered::Licence {
            token: token.into(),
            message: text("message").map(sentence),
        },
        _ => Answered::Unreachable,
    }
}

/// A reason as the page may compare it: lowercase letters and hyphens.
fn reason_named(reason: &str) -> String {
    if (1..=40).contains(&reason.len())
        && reason.bytes().all(|b| b.is_ascii_lowercase() || b == b'-')
    {
        reason.into()
    } else {
        "refused".into()
    }
}

/// A sentence for the customer: one line, no control or bidi-control
/// characters, at most 400 characters.
fn sentence(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .filter(|c| !c.is_control() && !matches!(c, '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'))
        .take(400)
        .collect()
}

// --------------------------------------------------------------- the page

/// How a `store_*` command ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Outcome {
    /// Bought, and its licence kept.
    Bought,
    /// Restored, and the licence kept.
    Restored,
    /// A licence kept from what StoreKit had: a renewal, or a transaction
    /// the listener heard.
    Renewed,
    /// Waiting for approval (Ask to Buy) or for the customer's bank.
    Pending,
    Cancelled,
    /// StoreKit's own sentence is the message.
    Failed,
    /// StoreKit could not verify the App Store's answer, so nothing is used.
    Unverified,
    /// StoreKit lists nothing RATA sells.
    Nothing,
    /// mailrata.org gave no licence, or RATA did not keep it; any licence
    /// RATA had is still the one in use.
    Refused,
    /// mailrata.org could not be reached.
    Unreachable,
    /// Apple's own subscriptions sheet was shown.
    Shown,
}

/// A `store_*` command's answer. `available` is false on a computer and on
/// Android, and nothing else is said there.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Answer {
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<Outcome>,
    /// `store_products`: Pro, then Base, as StoreKit offers them here.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub products: Option<Vec<Offer>>,
    /// The standing afterwards: the new licence's, or the one kept.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub standing: Option<Standing>,
    /// mailrata.org's reason for a refusal, or `not-kept` when RATA did not
    /// keep what it gave.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl Answer {
    /// What a computer and Android answer.
    pub fn unavailable() -> Self {
        Answer::default()
    }

    fn of(outcome: Outcome) -> Self {
        Answer {
            available: true,
            outcome: Some(outcome),
            ..Answer::default()
        }
    }

    fn said(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }

    fn standing(mut self, standing: Standing) -> Self {
        self.standing = Some(standing);
        self
    }

    fn because(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }

    /// Whether a licence was kept, which the page hears as `rata-standing`.
    pub fn kept(&self) -> bool {
        matches!(
            self.outcome,
            Some(Outcome::Bought | Outcome::Restored | Outcome::Renewed)
        )
    }
}

/// The page's `rata-standing` event, as bridge.js fires it after a renewal,
/// carrying the standing. The standing goes in as a JSON literal, so nothing
/// in it can become code.
pub fn standing_script(standing: &Standing) -> String {
    let detail = serde_json::to_string(standing).unwrap_or_else(|_| "null".into());
    format!("window.dispatchEvent(new CustomEvent('rata-standing',{{detail:{detail}}}))")
}

/// Whether the licence is due to be renewed by itself: bridge.js's rule for
/// its own renewal. There must be a licence to renew, so after Delete account,
/// or on a phone never licensed, nothing is asked until the customer buys or
/// restores, or StoreKit hands over a transaction.
pub fn due(standing: &Standing) -> bool {
    standing.token.is_some() && (!standing.licensed || standing.renew_soon)
}

/// The transactions worth sending, in the order given, each once, at most
/// [`SENT_MOST`].
fn worth_sending(list: Vec<Signed>) -> Vec<Signed> {
    let mut out: Vec<Signed> = Vec::new();
    for s in list {
        if out.len() == SENT_MOST {
            break;
        }
        if s.worth_sending() && !out.iter().any(|o| o.id == s.id) {
            out.push(s);
        }
    }
    out
}

fn reason_of(reason: Option<crate::licence::Reason>) -> String {
    reason
        .and_then(|r| serde_json::to_value(r).ok())
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_else(|| "refused".into())
}

/// Buying and restoring RATA, and keeping its licence, through StoreKit.
pub struct AppStore<C> {
    kit: Kit<C>,
    server: Mailrata,
    /// One licence at a time: a purchase, a restore, the renewal and the
    /// listener each ask mailrata.org, keep and finish under this, so two
    /// never race to keep a licence or to finish the same transaction.
    one: tokio::sync::Mutex<()>,
}

impl<C: Call> AppStore<C> {
    pub fn new(call: C, server: Mailrata) -> Self {
        AppStore {
            kit: Kit { call },
            server,
            one: tokio::sync::Mutex::new(()),
        }
    }

    /// What RATA sells, as StoreKit offers it here: Pro, then Base, with
    /// their prices. None, with StoreKit's sentence, when it could not say.
    pub async fn products(&self) -> Answer {
        let ids: Vec<&str> = PRODUCTS.iter().map(|(id, _)| *id).collect();
        match self.kit.products(&ids).await {
            Ok(listed) => {
                let offers = PRODUCTS
                    .iter()
                    .filter_map(|(id, plan)| {
                        listed.iter().find(|p| p.id == *id).map(|p| Offer {
                            plan,
                            product: p.clone(),
                        })
                    })
                    .collect();
                Answer {
                    available: true,
                    products: Some(offers),
                    ..Answer::default()
                }
            }
            Err(e) => Answer {
                products: Some(Vec::new()),
                ..Answer::of(Outcome::Failed).said(e)
            },
        }
    }

    /// Buy `plan`: StoreKit's purchase sheet, then, once bought, its
    /// licence (`licence`).
    pub async fn buy(&self, rata: &Rata, plan: &str) -> Answer {
        let Some(id) = product_for(plan) else {
            return Answer::of(Outcome::Failed).said(NO_SUCH_PLAN);
        };
        let began = rata.epoch();
        match self.kit.purchase(id).await {
            Err(e) => Answer::of(Outcome::Failed).said(e),
            Ok(Purchase::Pending) => Answer::of(Outcome::Pending),
            Ok(Purchase::Cancelled) => Answer::of(Outcome::Cancelled),
            Ok(Purchase::Unverified) => Answer::of(Outcome::Unverified).said(UNVERIFIED),
            Ok(Purchase::Failed { message }) => Answer::of(Outcome::Failed).said(
                message
                    .filter(|m| !m.trim().is_empty())
                    .unwrap_or_else(|| NOT_BOUGHT.into()),
            ),
            Ok(Purchase::Bought { transaction }) => {
                let list = self.with_entitlements(transaction).await;
                self.licence(rata, began, list, Outcome::Bought).await
            }
        }
    }

    /// Restore purchases: the App Store's own sync, then the licence for
    /// what the customer is entitled to.
    pub async fn restore(&self, rata: &Rata) -> Answer {
        let began = rata.epoch();
        match self.kit.restore().await {
            Err(e) => Answer::of(Outcome::Failed).said(e),
            Ok(r) if r.cancelled => Answer::of(Outcome::Cancelled),
            Ok(r) => {
                self.licence(rata, began, r.transactions, Outcome::Restored)
                    .await
            }
        }
    }

    /// Apple's own sheet for managing or cancelling the subscription.
    pub async fn manage(&self) -> Answer {
        match self.kit.manage().await {
            Ok(()) => Answer::of(Outcome::Shown),
            Err(e) => Answer::of(Outcome::Failed).said(e),
        }
    }

    /// A licence from what the customer is entitled to now.
    pub async fn renew(&self, rata: &Rata) -> Answer {
        let began = rata.epoch();
        match self.kit.entitlements().await {
            Err(e) => Answer::of(Outcome::Failed).said(e),
            Ok(list) => self.licence(rata, began, list, Outcome::Renewed).await,
        }
    }

    /// A transaction StoreKit handed over by itself, with what the customer
    /// is entitled to now: a downgrade's renewal comes this way while the
    /// higher plan still runs, and mailrata.org gives the higher plan.
    pub async fn heard(&self, rata: &Rata, update: Signed) -> Answer {
        let began = rata.epoch();
        let list = self.with_entitlements(update).await;
        self.licence(rata, began, list, Outcome::Renewed).await
    }

    /// Where `Transaction.updates` should go: a channel of RATA's
    /// (`tauri::ipc::Channel`), as JSON.
    pub async fn listen(&self, handler: Value) -> Result<(), String> {
        self.kit.listen(handler).await
    }

    /// `first`, then what the customer is entitled to now. Should StoreKit
    /// not say, `first` still goes.
    async fn with_entitlements(&self, first: Signed) -> Vec<Signed> {
        let mut list = vec![first];
        if let Ok(more) = self.kit.entitlements().await {
            list.extend(more);
        }
        list
    }

    /// Steps 1 to 3 of the module's: the transactions to mailrata.org, the
    /// licence it gives kept as the licence box keeps one, and only then
    /// each transaction finished. `began` is the epoch read when this began
    /// (SEC-7): after a Delete account in between, nothing is kept and
    /// nothing finished. `done` is the outcome when a licence is kept.
    async fn licence(&self, rata: &Rata, began: u64, list: Vec<Signed>, done: Outcome) -> Answer {
        let _one = self.one.lock().await;
        let sent = worth_sending(list);
        if sent.is_empty() {
            return Answer::of(Outcome::Nothing)
                .said(NOTHING)
                .standing(rata.standing());
        }
        let records: Vec<String> = sent.iter().map(|s| s.jws.clone()).collect();
        let (token, message) = match self.server.licence_for(&records).await {
            Answered::Unreachable => {
                return Answer::of(Outcome::Unreachable)
                    .said(UNREACHABLE)
                    .standing(rata.standing());
            }
            Answered::Refused { reason, message } => {
                return Answer::of(Outcome::Refused)
                    .because(reason)
                    .said(message)
                    .standing(rata.standing());
            }
            Answered::Licence { token, message } => (token, message),
        };
        let standing = match rata.set_licence(Some(token), Some(began)) {
            Ok(s) => s,
            Err(e) => {
                return Answer::of(Outcome::Refused)
                    .because("not-kept")
                    .said(e)
                    .standing(rata.standing());
            }
        };
        if let Some(refused) = standing.refused.clone() {
            return Answer::of(Outcome::Refused)
                .because(reason_of(Some(refused.reason)))
                .said(refused.message)
                .standing(standing);
        }
        if !standing.licensed {
            return Answer::of(Outcome::Refused)
                .because(reason_of(standing.reason))
                .said(standing.message.clone())
                .standing(standing);
        }
        // Kept. A transaction StoreKit could not finish now comes back at the
        // next launch, and its licence is asked for again.
        for s in &sent {
            let _ = self.kit.finish(&s.id).await;
        }
        let message = message.unwrap_or_else(|| standing.message.clone());
        Answer::of(done).said(message).standing(standing)
    }
}

// ----------------------------------------------------------- the commands

/// What RATA sells in the App Store, with StoreKit's prices.
#[tauri::command]
pub async fn store_products(app: tauri::AppHandle) -> Answer {
    #[cfg(target_os = "ios")]
    {
        if let Some((shop, _)) = ios::shop(&app) {
            return shop.products().await;
        }
    }
    let _ = app;
    Answer::unavailable()
}

/// Buy `plan` (`pro` or `base`) through the App Store.
#[tauri::command]
pub async fn store_buy(app: tauri::AppHandle, plan: String) -> Answer {
    #[cfg(target_os = "ios")]
    {
        if let Some((shop, rata)) = ios::shop(&app) {
            let answer = shop.buy(&rata, &plan).await;
            ios::tell(&app, &answer);
            return answer;
        }
    }
    let _ = (app, plan);
    Answer::unavailable()
}

/// Restore purchases.
#[tauri::command]
pub async fn store_restore(app: tauri::AppHandle) -> Answer {
    #[cfg(target_os = "ios")]
    {
        if let Some((shop, rata)) = ios::shop(&app) {
            let answer = shop.restore(&rata).await;
            ios::tell(&app, &answer);
            return answer;
        }
    }
    let _ = app;
    Answer::unavailable()
}

/// Apple's sheet for managing or cancelling the subscription.
#[tauri::command]
pub async fn store_manage(app: tauri::AppHandle) -> Answer {
    #[cfg(target_os = "ios")]
    {
        if let Some((shop, _)) = ios::shop(&app) {
            return shop.manage().await;
        }
    }
    let _ = app;
    Answer::unavailable()
}

/// A licence from StoreKit's entitlements now, for the page's own renewal
/// (bridge.js's `settleLicence`) to try before `/api/licence/renew`, so that
/// the phone has one way of renewing at a time: `nothing` when StoreKit has
/// no subscription, and then the page renews as it does on a computer.
#[tauri::command]
pub async fn store_renew(app: tauri::AppHandle) -> Answer {
    #[cfg(target_os = "ios")]
    {
        if let Some((shop, rata)) = ios::shop(&app) {
            let answer = shop.renew(&rata).await;
            ios::tell(&app, &answer);
            return answer;
        }
    }
    let _ = app;
    Answer::unavailable()
}

/// The smoke build's check (`smoke`): the StoreKit plugin is loaded and
/// answers RATA's question for the products. A Simulator started by simctl
/// has no StoreKit configuration and no App Store account, so none may come
/// back; that it answers at all is what is checked.
#[cfg(feature = "smoke")]
pub async fn smoke_check(app: &tauri::AppHandle) -> crate::smoke::Check {
    #[cfg(target_os = "ios")]
    let answer = match ios::shop(app) {
        Some((shop, _)) => tokio::time::timeout(Duration::from_secs(30), shop.products())
            .await
            .ok(),
        None => None,
    };
    #[cfg(not(target_os = "ios"))]
    let answer = {
        let _ = app;
        Some(Answer::unavailable())
    };
    smoke_judged(answer.as_ref(), cfg!(target_os = "ios"))
}

#[cfg(feature = "smoke")]
fn smoke_judged(answer: Option<&Answer>, ios: bool) -> crate::smoke::Check {
    let detail = match answer {
        None => {
            json!({ "error": "the StoreKit plugin did not answer within 30 seconds, or is not loaded" })
        }
        Some(a) => json!({
            "available": a.available,
            "products": a.products.as_ref().map(|p| p.iter().map(|o| o.product.id.clone()).collect::<Vec<_>>()),
            "said": a.message,
        }),
    };
    crate::smoke::Check {
        name: "app store",
        ok: answer.is_some_and(|a| a.available == ios),
        detail,
    }
}

// ------------------------------------------------------------------- iOS

#[cfg(target_os = "ios")]
mod ios {
    use std::sync::Arc;

    use serde_json::Value;
    use tauri::plugin::PluginHandle;
    use tauri::{AppHandle, Manager, Wry};

    use super::{Answer, AppStore, Call, Mailrata, RENEW_EVERY, Signed, due, standing_script};
    use crate::core::Rata;

    pub type Shop = AppStore<PluginHandle<Wry>>;

    impl Call for PluginHandle<Wry> {
        async fn call(&self, command: &'static str, payload: Value) -> Result<Value, String> {
            self.run_mobile_plugin_async::<Value>(command, payload)
                .await
                .map_err(|e| e.to_string())
        }
    }

    pub fn shop(app: &AppHandle) -> Option<(Arc<Shop>, Arc<Rata>)> {
        let shop = app.try_state::<Arc<Shop>>()?.inner().clone();
        let rata = app.try_state::<Arc<Rata>>()?.inner().clone();
        Some((shop, rata))
    }

    /// The page hears of a licence kept, as it hears of a renewal.
    pub fn tell(app: &AppHandle, answer: &Answer) {
        if answer.kept()
            && let Some(standing) = &answer.standing
            && let Some(window) = app.get_webview_window("main")
        {
            let _ = window.eval(standing_script(standing));
        }
    }

    /// As RATA starts, once its window is up: the shop the commands use,
    /// the listener's channel, and the renewal at launch and every six
    /// hours while the licence is due.
    pub fn start(app: &AppHandle, rata: Arc<Rata>) {
        let Some(kit) = app.try_state::<tauri_plugin_storekit::StoreKit<Wry>>() else {
            return;
        };
        let Ok(server) = Mailrata::production() else {
            return;
        };
        let shop = Arc::new(AppStore::new(kit.0.clone(), server));
        app.manage(shop.clone());
        let app = app.clone();
        tauri::async_runtime::spawn(run(app, shop, rata));
    }

    async fn run(app: AppHandle, shop: Arc<Shop>, rata: Arc<Rata>) {
        // Each transaction from the listener, in turn. A queue of 32 is far
        // more than StoreKit hands over at once.
        let (heard, mut updates) = tokio::sync::mpsc::channel::<Signed>(32);
        let channel = tauri::ipc::Channel::<Value>::new(move |body| {
            if let Some(update) = super::update_in(&body) {
                let _ = heard.try_send(update);
            }
            Ok(())
        });
        let listening = match serde_json::to_value(&channel) {
            Ok(handler) => shop.listen(handler).await.is_ok(),
            Err(_) => false,
        };
        if listening {
            let (shop, rata, app) = (shop.clone(), rata.clone(), app.clone());
            tauri::async_runtime::spawn(async move {
                while let Some(update) = updates.recv().await {
                    let answer = shop.heard(&rata, update).await;
                    tell(&app, &answer);
                }
            });
        }
        // At launch, then every six hours.
        let mut every = tokio::time::interval(RENEW_EVERY);
        loop {
            every.tick().await;
            if due(&rata.standing()) {
                let answer = shop.renew(&rata).await;
                tell(&app, &answer);
            }
        }
    }
}

#[cfg(target_os = "ios")]
pub use ios::start;

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, VecDeque};
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::sync::oneshot;

    use super::*;
    use crate::store::Store;
    use crate::vault::Memory;

    /// The Swift plugin's commands, each of which RATA's Rust sends.
    const COMMANDS: [&str; 7] = [
        "products",
        "purchase",
        "entitlements",
        "restore",
        "manage",
        "finish",
        "listen",
    ];

    /// The key pair and the licences core.rs's tests use, made by
    /// rata-next/lib/licence.js, good until 2108.
    const KEY: &str = "-----BEGIN PUBLIC KEY-----\nMCowBQYDK2VwAyEA+pogY6bod0k5ez7c/lE4N1/X2/5sbonmcLhIb7Oqrzs=\n-----END PUBLIC KEY-----";
    const BASE: &str = "v1.eyJ2IjoxLCJzdWIiOiJidXllckBleGFtcGxlLmNvbSIsInBsYW4iOiJiYXNlIiwiaWF0IjoxNzg5NTczMjM1LCJleHAiOjQzODE1NzMyMzV9.tcfOa11iI2hOD5wozS1wS4if109Eg0lW5SuHi6XB0ZH8s0Yo4nBi9-g-av9MKjT4-xomtg3aa3xu1B9ngjXODg";
    const PRO: &str = "v1.eyJ2IjoxLCJzdWIiOiJidXllckBleGFtcGxlLmNvbSIsInBsYW4iOiJwcm8iLCJpYXQiOjE3ODk1NzMyMzUsImV4cCI6NDM4MTU3MzIzNX0.cSIyj1Xy5bhvD5sph49VZPSnPZkzvRd5zEERGN5b76g-pTJ4IobTVQBfSDDXMSAqHlNx3G14NYoT5OdK6hvUDw";
    /// Expired in 2020, too long ago to renew.
    const LAPSED: &str = "v1.eyJ2IjoxLCJzdWIiOiJidXllckBleGFtcGxlLmNvbSIsInBsYW4iOiJwcm8iLCJpYXQiOjE1Nzc4MzY4MDAsImV4cCI6MTU4MDQyODgwMH0.BlRlIy2nCmo9WsLkPb_8pCzY9dZFyjGyb_I085xtyQp2UmHu-3QJHUMPs9-VPSEglg4Qn235BJzLt8uQE4TlAA";
    /// A genuine licence signed with another key (licence.rs's tests'):
    /// what a mailrata.org signing with the wrong key would hand over.
    const OTHER_KEYS: &str = "v1.eyJ2IjoxLCJzdWIiOiJidXllckBleGFtcGxlLmNvbSIsInBsYW4iOiJwcm8iLCJpYXQiOjE3ODk1MTY4MDAsImV4cCI6MTc5MjEwODgwMH0.d8B8bwGISFtqh3wODGdMd6CCNqoa99GnHDzmY1tvNqR2O8vhuQSoe9BezQrry4bMIQXuFwfmfeEj4GALgwV5CA";

    const PRO_ID: &str = PRODUCTS[0].0;
    const BASE_ID: &str = PRODUCTS[1].0;

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    fn rata(name: &str) -> (Rata, PathBuf) {
        let dir = std::env::temp_dir().join(format!("rata-appstore-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("mailboxes.json");
        let app = Rata::new(
            Store::open(&file),
            Box::new(Memory::default()),
            rata_mail::Resolver::system().expect("resolver"),
            Some(KEY),
        );
        (app, file)
    }

    fn licensed(name: &str, token: &str) -> (Rata, PathBuf) {
        let (app, file) = rata(name);
        app.set_licence(Some(token.into()), None).unwrap();
        (app, file)
    }

    /// Apple's signed record of a transaction, in a JWS's shape, naming it.
    fn jws(id: &str) -> String {
        format!("eyJhbGciOiJFUzI1NiJ9.tx{id}.c2lnbmVk")
    }

    /// A transaction as the Swift side hands it over.
    fn tx(id: &str, product: &str) -> Value {
        json!({
            "id": id,
            "originalId": id,
            "product": product,
            "jws": jws(id),
            "revoked": false,
        })
    }

    /// A stand-in StoreKit: answers each command as the Swift plugin
    /// would, from a script, and notes every command it is asked and what
    /// the list on disk held at each `finish`.
    #[derive(Default)]
    struct StandIn {
        scripted: Mutex<HashMap<&'static str, VecDeque<Result<Value, String>>>>,
        asked: Mutex<Vec<(&'static str, Value)>>,
        list: Mutex<Option<PathBuf>>,
        at_finish: Mutex<Vec<String>>,
    }

    impl StandIn {
        fn new() -> Arc<Self> {
            Arc::new(StandIn::default())
        }

        fn then(
            self: &Arc<Self>,
            command: &'static str,
            answer: Result<Value, String>,
        ) -> Arc<Self> {
            self.scripted
                .lock()
                .unwrap()
                .entry(command)
                .or_default()
                .push_back(answer);
            self.clone()
        }

        fn watching(self: &Arc<Self>, list: &Path) -> Arc<Self> {
            *self.list.lock().unwrap() = Some(list.to_path_buf());
            self.clone()
        }

        fn asked(&self) -> Vec<&'static str> {
            self.asked.lock().unwrap().iter().map(|(c, _)| *c).collect()
        }

        fn finished(&self) -> Vec<String> {
            self.asked
                .lock()
                .unwrap()
                .iter()
                .filter(|(c, _)| *c == "finish")
                .map(|(_, p)| p["id"].as_str().unwrap().to_string())
                .collect()
        }
    }

    impl Call for Arc<StandIn> {
        async fn call(&self, command: &'static str, payload: Value) -> Result<Value, String> {
            self.asked.lock().unwrap().push((command, payload));
            if command == "finish" {
                let list = self.list.lock().unwrap().clone();
                if let Some(list) = list {
                    let held = std::fs::read_to_string(list).unwrap_or_default();
                    self.at_finish.lock().unwrap().push(held);
                }
            }
            let next = self
                .scripted
                .lock()
                .unwrap()
                .get_mut(command)
                .and_then(VecDeque::pop_front);
            match next {
                Some(answer) => answer,
                None => match command {
                    "entitlements" => Ok(json!({ "transactions": [] })),
                    "finish" => Ok(json!({ "finished": true })),
                    _ => Err(format!("{command} was not scripted")),
                },
            }
        }
    }

    /// One answer of the stand-in mailrata.org.
    struct Reply {
        status: &'static str,
        headers: String,
        body: String,
    }

    fn reply(status: &'static str, body: impl Into<String>) -> Reply {
        Reply {
            status,
            headers: String::new(),
            body: body.into(),
        }
    }

    fn licence_reply(token: &str) -> Reply {
        reply(
            "200 OK",
            json!({
                "licensed": true,
                "licence": token,
                "plan": "pro",
                "message": "Licensed for RATA Pro through the App Store. RATA renews the licence by itself while the subscription lasts.",
            })
            .to_string(),
        )
    }

    /// A stand-in mailrata.org: each request, as it came, is kept; the
    /// replies go out in order, then a bare 500 to anything more.
    struct Server {
        url: String,
        heard: Arc<Mutex<Vec<String>>>,
    }

    impl Server {
        fn heard(&self) -> Vec<String> {
            self.heard.lock().unwrap().clone()
        }
    }

    async fn read_request(s: &mut tokio::net::TcpStream) -> String {
        let mut got = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            let n = s.read(&mut buf).await.unwrap_or(0);
            got.extend_from_slice(&buf[..n]);
            let text = String::from_utf8_lossy(&got).to_string();
            if let Some(end) = text.find("\r\n\r\n") {
                let len: usize = text[..end]
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|v| v.trim().parse().unwrap())
                    })
                    .unwrap_or(0);
                if got.len() >= end + 4 + len {
                    return text;
                }
            }
            if n == 0 {
                return text;
            }
        }
    }

    /// `gate`: told when the first request is in, and holding its answer
    /// until released, so a test can do something in between.
    async fn serve(
        replies: Vec<Reply>,
        gate: Option<(oneshot::Sender<()>, oneshot::Receiver<()>)>,
    ) -> Server {
        let l = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let url = format!(
            "http://127.0.0.1:{}/api/licence/apple",
            l.local_addr().unwrap().port()
        );
        let heard = Arc::new(Mutex::new(Vec::new()));
        let kept = heard.clone();
        tokio::spawn(async move {
            let mut replies = replies.into_iter();
            let mut gate = gate;
            loop {
                let Ok((mut s, _)) = l.accept().await else {
                    return;
                };
                let request = read_request(&mut s).await;
                kept.lock().unwrap().push(request);
                if let Some((asked, release)) = gate.take() {
                    let _ = asked.send(());
                    let _ = release.await;
                }
                let r = replies
                    .next()
                    .unwrap_or_else(|| reply("500 Internal Server Error", ""));
                let answer = format!(
                    "HTTP/1.1 {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{}\r\n{}",
                    r.status,
                    r.body.len(),
                    r.headers,
                    r.body
                );
                let _ = s.write_all(answer.as_bytes()).await;
                let _ = s.shutdown().await;
            }
        });
        Server { url, heard }
    }

    fn shop(kit: &Arc<StandIn>, server: &str) -> AppStore<Arc<StandIn>> {
        AppStore::new(kit.clone(), Mailrata::at(server, false).unwrap())
    }

    /// The transactions a request to mailrata.org carried.
    fn sent_in(request: &str) -> Vec<String> {
        let body = &request[request.find("\r\n\r\n").unwrap() + 4..];
        let v: Value = serde_json::from_str(body).unwrap();
        v["transactions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t.as_str().unwrap().to_string())
            .collect()
    }

    fn bought(id: &str, product: &str) -> Result<Value, String> {
        Ok(json!({ "outcome": "bought", "transaction": tx(id, product) }))
    }

    // ------------------------------------------------------ the products

    /// The product ids live in two places, kept equal: here and
    /// `APPLE_PRODUCTS` in rata-next/lib/plan.js, which the website's
    /// licence follows. Until plan.js has it (claude/iap-server), only this
    /// side's shape is checked; once it does, the pairs must be the same.
    #[test]
    fn the_products_are_the_websites_and_pro_comes_first() {
        assert_eq!(PRODUCTS.map(|(_, plan)| plan), ["pro", "base"]);
        for (id, plan) in PRODUCTS {
            assert_eq!(id, format!("org.mailrata.{plan}.monthly"));
            assert_eq!(plan_of(id), Some(plan));
            assert_eq!(product_for(plan), Some(id));
            assert_eq!(
                crate::licence::plan_def(plan).key,
                plan,
                "a plan the licence knows"
            );
        }
        assert_eq!(product_for("enterprise"), None);
        assert_eq!(plan_of("com.example.other"), None);

        let plan_js = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../rata-next/lib/plan.js");
        let source = std::fs::read_to_string(&plan_js).expect("rata-next/lib/plan.js");
        match apple_products_in(&source) {
            Some(pairs) => assert_eq!(
                pairs,
                PRODUCTS
                    .iter()
                    .map(|(id, plan)| (id.to_string(), plan.to_string()))
                    .collect::<Vec<_>>(),
                "APPLE_PRODUCTS in rata-next/lib/plan.js"
            ),
            None => println!(
                "rata-next/lib/plan.js has no APPLE_PRODUCTS on this tree yet (claude/iap-server): only this side was checked"
            ),
        }
    }

    /// `APPLE_PRODUCTS = Object.freeze({ 'id': 'plan', ... })`, in its order.
    fn apple_products_in(source: &str) -> Option<Vec<(String, String)>> {
        let start = source.find("APPLE_PRODUCTS =")?;
        let rest = &source[start..];
        let body = &rest[rest.find('{')? + 1..rest.find('}')?];
        let quoted: Vec<&str> = body.split(['\'', '"']).skip(1).step_by(2).collect();
        Some(
            quoted
                .chunks(2)
                .map(|p| (p[0].to_string(), p.get(1).unwrap_or(&"").to_string()))
                .collect(),
        )
    }

    #[test]
    fn the_products_in_plan_js_are_read_as_written() {
        let js = "export const APPLE_PRODUCTS = Object.freeze({\n  'pro.id': 'pro',\n  \"base.id\": 'base',\n});\n";
        assert_eq!(
            apple_products_in(js),
            Some(vec![
                ("pro.id".into(), "pro".into()),
                ("base.id".into(), "base".into())
            ])
        );
        assert_eq!(apple_products_in("export const PLANS = {};"), None);
    }

    /// ios/RATA.storekit, for testing in Xcode, sells exactly these: one
    /// group, "RATA", monthly, Pro at level 1 and Base at level 2, with
    /// prices that say they are placeholders.
    #[test]
    fn the_storekit_configuration_sells_the_same_products() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../ios/RATA.storekit");
        let config: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(config["products"], json!([]));
        assert_eq!(config["nonRenewingSubscriptions"], json!([]));
        let [group] = config["subscriptionGroups"].as_array().unwrap().as_slice() else {
            panic!("one subscription group");
        };
        assert_eq!(group["name"], "RATA");
        let subs = group["subscriptions"].as_array().unwrap();
        assert_eq!(subs.len(), PRODUCTS.len());
        for (level, (id, _)) in PRODUCTS.iter().enumerate() {
            let s = subs.iter().find(|s| s["productID"] == *id).expect(id);
            assert_eq!(s["groupNumber"], level + 1, "{id}");
            assert_eq!(s["recurringSubscriptionPeriod"], "P1M", "{id}");
            assert_eq!(s["type"], "RecurringSubscription", "{id}");
            assert_eq!(s["subscriptionGroupID"], group["id"], "{id}");
            let name = s["referenceName"].as_str().unwrap();
            assert!(name.contains("placeholder price"), "{id}: {name}");
            let said = s["localizations"][0]["description"].as_str().unwrap();
            assert!(said.contains("Placeholder price"), "{id}: {said}");
        }
    }

    /// The page reaches none of the plugin: it names no command a
    /// permission could allow, no capability of RATA's names it, and only
    /// RATA for iOS is built with it. And each command RATA's Rust sends is
    /// one its Swift class answers.
    #[test]
    fn only_rata_reaches_the_plugin_and_only_on_ios() {
        let here = Path::new(env!("CARGO_MANIFEST_DIR"));
        let build = std::fs::read_to_string(here.join("../plugins/storekit/build.rs")).unwrap();
        assert!(build.contains("const COMMANDS: &[&str] = &[];"), "{build}");
        let capabilities = here.join("capabilities");
        if capabilities.exists() {
            for entry in std::fs::read_dir(&capabilities).unwrap() {
                let text = std::fs::read_to_string(entry.unwrap().path()).unwrap_or_default();
                assert!(
                    !text.contains("storekit"),
                    "a capability names the StoreKit plugin"
                );
            }
        }
        let manifest = std::fs::read_to_string(here.join("Cargo.toml")).unwrap();
        let named: Vec<&str> = manifest
            .split("\n[")
            .filter(|table| table.contains("tauri-plugin-storekit"))
            .map(|table| table.lines().next().unwrap_or(""))
            .collect();
        assert_eq!(named, ["target.'cfg(target_os = \"ios\")'.dependencies]"]);

        let swift = std::fs::read_to_string(
            here.join("../plugins/storekit/ios/Sources/StoreKitPlugin.swift"),
        )
        .unwrap();
        for command in COMMANDS {
            assert!(
                swift.contains(&format!("@objc public func {command}(_ invoke: Invoke)")),
                "the Swift plugin has no {command}"
            );
        }
    }

    #[test]
    fn products_come_pro_then_base_with_the_app_stores_prices() {
        rt().block_on(async {
            let listed = |id: &str, price: &str| {
                json!({
                    "id": id,
                    "displayName": "RATA",
                    "description": "Your mail",
                    "displayPrice": price,
                    "period": { "unit": "month", "value": 1 },
                })
            };
            let kit = StandIn::new().then(
                "products",
                Ok(json!({ "products": [
                    listed(BASE_ID, "£4.99"),
                    listed("com.example.other", "£1.00"),
                    listed(PRO_ID, "£9.99"),
                ] })),
            );
            let answer = shop(&kit, "http://127.0.0.1:9/").products().await;
            assert_eq!(
                kit.asked.lock().unwrap()[0].1,
                json!({ "ids": [PRO_ID, BASE_ID] })
            );
            let said = serde_json::to_value(&answer).unwrap();
            assert_eq!(said["available"], true);
            let products = said["products"].as_array().unwrap();
            assert_eq!(products.len(), 2, "{said}");
            assert_eq!(products[0]["plan"], "pro");
            assert_eq!(products[0]["id"], PRO_ID);
            assert_eq!(products[0]["displayPrice"], "£9.99");
            assert_eq!(
                products[0]["period"],
                json!({ "unit": "month", "value": 1 })
            );
            assert_eq!(products[1]["plan"], "base");
            assert_eq!(products[1]["displayPrice"], "£4.99");

            // StoreKit could not say: no products, and its sentence.
            let kit = StandIn::new().then(
                "products",
                Err("The Internet connection appears to be offline.".into()),
            );
            let answer = shop(&kit, "http://127.0.0.1:9/").products().await;
            assert!(answer.available);
            assert_eq!(answer.products, Some(vec![]));
            assert_eq!(
                answer.message.as_deref(),
                Some("The Internet connection appears to be offline.")
            );
        });
    }

    // ---------------------------------------------------------- buying

    /// Bought: what StoreKit verified, and what the customer is entitled to
    /// now, go to mailrata.org; its licence is kept; and only then is each
    /// transaction finished.
    #[test]
    fn bought_keeps_the_licence_and_only_then_finishes() {
        rt().block_on(async {
            let (app, list) = rata("bought");
            let server = serve(vec![licence_reply(PRO)], None).await;
            let kit = StandIn::new()
                .then("purchase", bought("1001", PRO_ID))
                .then(
                    "entitlements",
                    Ok(json!({ "transactions": [tx("1001", PRO_ID), tx("1000", BASE_ID)] })),
                )
                .watching(&list);
            let answer = shop(&kit, &server.url).buy(&app, "pro").await;

            assert_eq!(answer.outcome, Some(Outcome::Bought), "{answer:?}");
            assert!(answer.kept());
            let standing = answer.standing.as_ref().unwrap();
            assert!(standing.licensed);
            assert_eq!(standing.plan.map(|p| p.key), Some("pro"));
            assert!(
                answer
                    .message
                    .as_deref()
                    .unwrap()
                    .contains("through the App Store")
            );
            assert_eq!(app.standing().token.as_deref(), Some(PRO));

            assert_eq!(kit.asked.lock().unwrap()[0].1, json!({ "id": PRO_ID }));
            assert_eq!(
                kit.asked(),
                ["purchase", "entitlements", "finish", "finish"]
            );
            assert_eq!(kit.finished(), ["1001", "1000"]);
            // At each finish, the licence was already on disk.
            let at = kit.at_finish.lock().unwrap().clone();
            assert_eq!(at.len(), 2);
            assert!(at.iter().all(|held| held.contains(PRO)), "{at:?}");

            // One request: to /api/licence/apple, JSON, each transaction once.
            let heard = server.heard();
            assert_eq!(heard.len(), 1);
            assert!(
                heard[0].starts_with("POST /api/licence/apple HTTP/1.1\r\n"),
                "{}",
                heard[0]
            );
            let lower = heard[0].to_ascii_lowercase();
            assert!(lower.contains("content-type: application/json"));
            assert!(!lower.contains("cookie"));
            assert_eq!(sent_in(&heard[0]), [jws("1001"), jws("1000")]);
        });
    }

    #[test]
    fn pending_cancelled_failed_and_unverified_ask_mailrata_nothing_and_finish_nothing() {
        rt().block_on(async {
            let (app, _) = licensed("not-bought", BASE);
            let server = serve(vec![], None).await;
            let cases = [
                (json!({ "outcome": "pending" }), Outcome::Pending, None),
                (json!({ "outcome": "cancelled" }), Outcome::Cancelled, None),
                (
                    json!({ "outcome": "failed", "message": "You are not allowed to make purchases." }),
                    Outcome::Failed,
                    Some("You are not allowed to make purchases."),
                ),
                (json!({ "outcome": "failed" }), Outcome::Failed, Some(NOT_BOUGHT)),
                (json!({ "outcome": "unverified" }), Outcome::Unverified, Some(UNVERIFIED)),
                // An unverified answer is never bought, whatever it carries.
                (
                    json!({ "outcome": "unverified", "transaction": tx("1", PRO_ID) }),
                    Outcome::Unverified,
                    Some(UNVERIFIED),
                ),
            ];
            for (said, outcome, message) in cases {
                let kit = StandIn::new().then("purchase", Ok(said.clone()));
                let answer = shop(&kit, &server.url).buy(&app, "pro").await;
                assert_eq!(answer.outcome, Some(outcome), "{said}");
                assert_eq!(answer.message.as_deref(), message, "{said}");
                assert!(!answer.kept());
                assert_eq!(kit.asked(), ["purchase"], "{said}");
            }
            // StoreKit's own error, as it threw it.
            let kit = StandIn::new().then("purchase", Err("The App Store is not available.".into()));
            let answer = shop(&kit, &server.url).buy(&app, "base").await;
            assert_eq!(answer.outcome, Some(Outcome::Failed));
            assert_eq!(answer.message.as_deref(), Some("The App Store is not available."));
            // A plan the App Store does not sell asks StoreKit nothing.
            let kit = StandIn::new();
            let answer = shop(&kit, &server.url).buy(&app, "enterprise").await;
            assert_eq!(answer.message.as_deref(), Some(NO_SUCH_PLAN));
            assert!(kit.asked().is_empty());

            assert!(server.heard().is_empty(), "mailrata.org was asked");
            assert_eq!(app.standing().token.as_deref(), Some(BASE));
        });
    }

    /// mailrata.org refused (nothing live, not verified, not switched on):
    /// the licence RATA has stays, the server's sentence is said, and
    /// nothing is finished, so StoreKit hands the purchase over again.
    #[test]
    fn a_refusal_keeps_the_licence_says_why_and_finishes_nothing() {
        rt().block_on(async {
            let (app, _) = licensed("refused", BASE);
            let unverified = "mailrata.org could not verify this App Store purchase. Try Restore purchases.";
            let off = "App Store purchases are not switched on for RATA yet.";
            let unreadable = "That was not a list of App Store purchases mailrata.org can read.";
            let refusals = [
                (
                    "200 OK",
                    json!({ "licensed": false, "reason": "no-subscription", "message": NOTHING, "error": NOTHING }),
                    "no-subscription",
                    NOTHING,
                ),
                (
                    "200 OK",
                    json!({ "licensed": false, "reason": "unverified", "message": unverified }),
                    "unverified",
                    unverified,
                ),
                (
                    "503 Service Unavailable",
                    json!({ "licensed": false, "reason": "not-configured", "message": off }),
                    "not-configured",
                    off,
                ),
                // The design's name for the sentence, alone.
                (
                    "400 Bad Request",
                    json!({ "reason": "malformed", "error": unreadable }),
                    "malformed",
                    unreadable,
                ),
            ];
            for (status, body, reason, message) in refusals {
                let server = serve(vec![reply(status, body.to_string())], None).await;
                let kit = StandIn::new().then("purchase", bought("7", PRO_ID));
                let answer = shop(&kit, &server.url).buy(&app, "pro").await;
                assert_eq!(answer.outcome, Some(Outcome::Refused), "{body}");
                assert_eq!(answer.reason.as_deref(), Some(reason));
                assert_eq!(answer.message.as_deref(), Some(message));
                assert!(kit.finished().is_empty(), "{body}");
                let standing = answer.standing.unwrap();
                assert!(standing.licensed);
                assert_eq!(standing.token.as_deref(), Some(BASE));
            }
        });
    }

    /// mailrata.org not reached, or answering with nothing meant in it:
    /// the licence stays, and nothing is finished.
    #[test]
    fn mailrata_unreachable_keeps_the_licence_and_finishes_nothing() {
        rt().block_on(async {
            let (app, _) = licensed("unreachable", BASE);
            let closed = {
                let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
                format!(
                    "http://127.0.0.1:{}/api/licence/apple",
                    l.local_addr().unwrap().port()
                )
            };
            // A redirect is not followed, even to a licence.
            let elsewhere = serve(vec![licence_reply(PRO)], None).await;
            let moved = Reply {
                status: "302 Found",
                headers: format!("Location: {}\r\n", elsewhere.url),
                body: String::new(),
            };
            let failing_licence = json!({ "licensed": true, "licence": PRO }).to_string();
            let cases: Vec<(String, &str)> = vec![
                (closed, "closed"),
                (serve(vec![moved], None).await.url, "a redirect"),
                (
                    serve(
                        vec![reply("500 Internal Server Error", "{\"error\":\"boom\"}")],
                        None,
                    )
                    .await
                    .url,
                    "an error with no reason",
                ),
                (
                    serve(vec![reply("200 OK", "<html>Hostinger</html>")], None)
                        .await
                        .url,
                    "a page",
                ),
                (
                    serve(vec![reply("502 Bad Gateway", failing_licence)], None)
                        .await
                        .url,
                    "a licence in a failure",
                ),
                (
                    serve(vec![reply("200 OK", "x".repeat(ANSWER_MOST + 1))], None)
                        .await
                        .url,
                    "far too much",
                ),
            ];
            for (url, what) in cases {
                let kit = StandIn::new().then("purchase", bought("8", BASE_ID));
                let answer = shop(&kit, &url).buy(&app, "base").await;
                assert_eq!(answer.outcome, Some(Outcome::Unreachable), "{what}");
                assert_eq!(answer.message.as_deref(), Some(UNREACHABLE), "{what}");
                assert!(kit.finished().is_empty(), "{what}");
                assert_eq!(app.standing().token.as_deref(), Some(BASE), "{what}");
            }
            assert!(elsewhere.heard().is_empty(), "the redirect was followed");
        });
    }

    /// A server that never answers counts as unreachable once the wait is
    /// up: twenty seconds, as bridge.js gives a renewal (shortened here).
    #[test]
    fn a_server_that_never_answers_is_given_twenty_seconds() {
        assert_eq!(WAIT, Duration::from_secs(20));
        assert_eq!(Mailrata::production().unwrap().wait, WAIT);
        rt().block_on(async {
            let l = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
            let url = format!(
                "http://127.0.0.1:{}/api/licence/apple",
                l.local_addr().unwrap().port()
            );
            tokio::spawn(async move {
                let (_s, _) = l.accept().await.unwrap();
                tokio::time::sleep(Duration::from_secs(60)).await;
            });
            let mut server = Mailrata::at(&url, false).unwrap();
            server.wait = Duration::from_millis(300);
            let began = std::time::Instant::now();
            assert_eq!(server.licence_for(&[jws("1")]).await, Answered::Unreachable);
            assert!(began.elapsed() < Duration::from_secs(5));
        });
    }

    /// A licence RATA would not keep (signed with another key, as from a
    /// mailrata.org set up wrongly) leaves the working one in use, says so,
    /// and finishes nothing.
    #[test]
    fn a_licence_that_is_not_kept_finishes_nothing() {
        rt().block_on(async {
            let (app, _) = licensed("not-kept", BASE);
            let server = serve(vec![licence_reply(OTHER_KEYS)], None).await;
            let kit = StandIn::new().then("purchase", bought("9", PRO_ID));
            let answer = shop(&kit, &server.url).buy(&app, "pro").await;
            assert_eq!(answer.outcome, Some(Outcome::Refused));
            assert_eq!(answer.reason.as_deref(), Some("bad-signature"));
            assert!(
                answer
                    .message
                    .as_deref()
                    .unwrap()
                    .contains("RATA kept the licence it already has"),
                "{answer:?}"
            );
            assert!(kit.finished().is_empty());
            assert_eq!(app.standing().token.as_deref(), Some(BASE));
        });
    }

    /// Delete account while mailrata.org is answering (SEC-7): the licence
    /// it gives is not kept, and nothing is finished.
    #[test]
    fn delete_account_in_between_keeps_nothing_and_finishes_nothing() {
        rt().block_on(async {
            let (app, list) = licensed("deleted", BASE);
            let (asked_tx, asked) = oneshot::channel();
            let (release, released) = oneshot::channel();
            let server = serve(vec![licence_reply(PRO)], Some((asked_tx, released))).await;
            let kit = StandIn::new().then("purchase", bought("10", PRO_ID));
            let shop = shop(&kit, &server.url);
            let buying = shop.buy(&app, "pro");
            let deleting = async {
                asked.await.unwrap();
                app.forget_everything().unwrap();
                release.send(()).unwrap();
            };
            let (answer, ()) = tokio::join!(buying, deleting);
            assert_eq!(answer.outcome, Some(Outcome::Refused), "{answer:?}");
            assert_eq!(answer.reason.as_deref(), Some("not-kept"));
            assert!(
                answer
                    .message
                    .as_deref()
                    .unwrap()
                    .contains("removed from this computer")
            );
            assert!(kit.finished().is_empty());
            assert!(!app.standing().licensed);
            assert_eq!(app.standing().token, None);
            assert!(!std::fs::read_to_string(&list).unwrap().contains(PRO));
        });
    }

    // --------------------------------------------- restoring and renewing

    #[test]
    fn restore_licenses_what_the_app_store_restored() {
        rt().block_on(async {
            let (app, _) = rata("restore");
            let server = serve(vec![licence_reply(BASE)], None).await;
            let kit = StandIn::new().then(
                "restore",
                Ok(json!({ "cancelled": false, "transactions": [tx("20", BASE_ID)] })),
            );
            let answer = shop(&kit, &server.url).restore(&app).await;
            assert_eq!(answer.outcome, Some(Outcome::Restored), "{answer:?}");
            assert!(answer.kept());
            assert_eq!(app.standing().plan.map(|p| p.key), Some("base"));
            assert_eq!(kit.asked(), ["restore", "finish"]);
            assert_eq!(sent_in(&server.heard()[0]), [jws("20")]);

            // Cancelled at the App Store's sign-in, or nothing to restore.
            let kit = StandIn::new().then(
                "restore",
                Ok(json!({ "cancelled": true, "transactions": [] })),
            );
            let answer = shop(&kit, &server.url).restore(&app).await;
            assert_eq!(answer.outcome, Some(Outcome::Cancelled));
            let kit = StandIn::new().then(
                "restore",
                Ok(json!({ "cancelled": false, "transactions": [] })),
            );
            let answer = shop(&kit, &server.url).restore(&app).await;
            assert_eq!(answer.outcome, Some(Outcome::Nothing));
            assert_eq!(answer.message.as_deref(), Some(NOTHING));
            let kit = StandIn::new().then("restore", Err("Cannot connect to iTunes Store".into()));
            let answer = shop(&kit, &server.url).restore(&app).await;
            assert_eq!(answer.outcome, Some(Outcome::Failed));
            assert_eq!(
                answer.message.as_deref(),
                Some("Cannot connect to iTunes Store")
            );
            assert_eq!(server.heard().len(), 1);
        });
    }

    /// Renewal by itself happens only while there is a licence that is due
    /// (bridge.js's rule): none after Delete account or before a first
    /// purchase, none for a licence with weeks left.
    #[test]
    fn renewal_is_due_only_for_a_licence_that_needs_it() {
        let (app, _) = rata("due");
        assert!(!due(&app.standing()), "no licence");
        app.set_licence(Some(PRO.into()), None).unwrap();
        assert!(!due(&app.standing()), "good until 2108");
        let mut soon = app.standing();
        soon.renew_soon = true;
        assert!(due(&soon), "in its last week");
        let (lapsed, _) = licensed("due-lapsed", LAPSED);
        assert!(due(&lapsed.standing()), "expired");
    }

    /// When StoreKit has no subscription, a renewal changes nothing and
    /// asks mailrata.org nothing: bridge.js's own renewal stands.
    #[test]
    fn a_renewal_with_nothing_from_storekit_changes_nothing() {
        rt().block_on(async {
            let (app, _) = licensed("renew-nothing", LAPSED);
            let server = serve(vec![], None).await;
            let kit = StandIn::new();
            let answer = shop(&kit, &server.url).renew(&app).await;
            assert_eq!(answer.outcome, Some(Outcome::Nothing));
            assert!(!answer.kept());
            assert_eq!(kit.asked(), ["entitlements"]);
            assert!(server.heard().is_empty());
            assert_eq!(app.standing().token.as_deref(), Some(LAPSED));

            // With one, an expired licence is renewed through it.
            let server = serve(vec![licence_reply(PRO)], None).await;
            let kit = StandIn::new().then(
                "entitlements",
                Ok(json!({ "transactions": [tx("30", PRO_ID)] })),
            );
            let answer = shop(&kit, &server.url).renew(&app).await;
            assert_eq!(answer.outcome, Some(Outcome::Renewed), "{answer:?}");
            assert!(app.standing().licensed);
            assert_eq!(kit.finished(), ["30"]);
        });
    }

    /// What the listener hears (a renewal, an Ask to Buy approved, a
    /// purchase on another device) is licensed as a purchase is, with what
    /// the customer is entitled to now, and finished once kept.
    #[test]
    fn the_listener_path_licenses_and_finishes_what_storekit_handed_over() {
        rt().block_on(async {
            let (app, list) = rata("heard");
            let server = serve(vec![licence_reply(BASE)], None).await;
            let kit = StandIn::new()
                .then(
                    "entitlements",
                    Ok(json!({ "transactions": [tx("40", BASE_ID)] })),
                )
                .watching(&list);
            let update: Signed = serde_json::from_value(tx("40", BASE_ID)).unwrap();
            let answer = shop(&kit, &server.url).heard(&app, update).await;
            assert_eq!(answer.outcome, Some(Outcome::Renewed), "{answer:?}");
            assert!(answer.kept());
            assert_eq!(app.standing().plan.map(|p| p.key), Some("base"));
            assert_eq!(sent_in(&server.heard()[0]), [jws("40")], "each once");
            assert_eq!(kit.finished(), ["40"]);
            assert!(kit.at_finish.lock().unwrap()[0].contains(BASE));

            // Something RATA does not sell is not sent at all.
            let kit = StandIn::new();
            let stranger: Signed = serde_json::from_value(tx("41", "com.example.other")).unwrap();
            let answer = shop(&kit, &server.url).heard(&app, stranger).await;
            assert_eq!(answer.outcome, Some(Outcome::Nothing));
            assert_eq!(server.heard().len(), 1);
        });
    }

    /// Base and Pro both live (a downgrade waiting for the period's end):
    /// both go, and the plan kept is the one mailrata.org gives, the higher.
    #[test]
    fn with_base_and_pro_both_live_both_go_and_the_higher_plan_is_kept() {
        rt().block_on(async {
            let (app, _) = licensed("highest", BASE);
            let server = serve(vec![licence_reply(PRO)], None).await;
            let kit = StandIn::new().then(
                "entitlements",
                Ok(json!({ "transactions": [tx("50", BASE_ID), tx("51", PRO_ID)] })),
            );
            let update: Signed = serde_json::from_value(tx("50", BASE_ID)).unwrap();
            let answer = shop(&kit, &server.url).heard(&app, update).await;
            assert_eq!(sent_in(&server.heard()[0]), [jws("50"), jws("51")]);
            assert_eq!(answer.standing.unwrap().plan.map(|p| p.key), Some("pro"));
            assert_eq!(app.standing().plan.map(|p| p.key), Some("pro"));
            assert_eq!(kit.finished(), ["50", "51"]);
        });
    }

    #[test]
    fn manage_shows_apples_sheet_or_says_why_not() {
        rt().block_on(async {
            let kit = StandIn::new().then("manage", Ok(Value::Null));
            let answer = shop(&kit, "http://127.0.0.1:9/").manage().await;
            assert_eq!(answer.outcome, Some(Outcome::Shown));
            let no_window = "RATA has no window to show your subscriptions in.";
            let kit = StandIn::new().then("manage", Err(no_window.into()));
            let answer = shop(&kit, "http://127.0.0.1:9/").manage().await;
            assert_eq!(answer.outcome, Some(Outcome::Failed));
            assert_eq!(answer.message.as_deref(), Some(no_window));
        });
    }

    // ------------------------------------------------------- the details

    /// Only transactions of Apple's shape, for products RATA sells, each
    /// once, at most ten: what mailrata.org would refuse is not sent.
    #[test]
    fn only_apples_kind_of_transaction_is_sent() {
        let signed = |id: &str, product: &str, jws: &str| Signed {
            id: id.into(),
            product: product.into(),
            jws: jws.into(),
        };
        let good = |id: &str| signed(id, PRO_ID, &jws(id));
        let list = vec![
            good("1"),
            good("1"),
            signed("2", "com.example.other", &jws("2")),
            signed("x3", PRO_ID, &jws("3")),
            signed("4", PRO_ID, "not a jws"),
            signed("5", PRO_ID, "a.b.c.d"),
            signed("6", PRO_ID, &format!("a.{}.c", "b".repeat(JWS_MOST))),
            signed("123456789012345678901", PRO_ID, &jws("7")),
            good("8"),
        ];
        let sent: Vec<String> = worth_sending(list).into_iter().map(|s| s.id).collect();
        assert_eq!(sent, ["1", "8"]);
        let many: Vec<Signed> = (0..15).map(|n| good(&n.to_string())).collect();
        assert_eq!(worth_sending(many).len(), SENT_MOST);
    }

    #[test]
    fn mailratas_answer_is_read_as_bridge_reads_a_renewal() {
        let read = |ok: bool, v: Value| answered(ok, v.to_string().as_bytes());
        assert_eq!(
            read(
                true,
                json!({ "licensed": true, "licence": PRO, "message": "Licensed." })
            ),
            Answered::Licence {
                token: PRO.into(),
                message: Some("Licensed.".into())
            }
        );
        // A reason is read first, and a licence beside it is not used.
        assert_eq!(
            read(
                true,
                json!({ "licensed": false, "licence": PRO, "reason": "no-subscription", "message": NOTHING })
            ),
            Answered::Refused {
                reason: "no-subscription".into(),
                message: NOTHING.into()
            }
        );
        // A reason that is not a reason's shape, and a sentence with
        // control and bidi-control characters in it.
        assert_eq!(
            read(
                true,
                json!({ "reason": "<b>Nope</b>", "message": "Line one\nline\u{202e} two\u{7}" })
            ),
            Answered::Refused {
                reason: "refused".into(),
                message: "Line one line two".into()
            }
        );
        assert_eq!(
            read(true, json!({ "licensed": false })),
            Answered::Unreachable
        );
        assert_eq!(
            read(true, json!({ "licensed": true })),
            Answered::Unreachable
        );
        assert_eq!(
            read(true, json!({ "licensed": "true", "licence": PRO })),
            Answered::Unreachable
        );
        assert_eq!(
            read(false, json!({ "licensed": true, "licence": PRO })),
            Answered::Unreachable
        );
        assert_eq!(answered(true, b"not json"), Answered::Unreachable);
    }

    /// Off iOS, and on Android, every `store_*` command says the App Store
    /// is not available, and nothing else.
    #[test]
    fn off_ios_the_app_store_is_not_available() {
        assert_eq!(
            serde_json::to_value(Answer::unavailable()).unwrap(),
            json!({ "available": false })
        );
        assert!(!Answer::unavailable().kept());
    }

    /// The page's `rata-standing`, with the standing as a JSON literal.
    #[test]
    fn the_page_hears_rata_standing_with_the_standing() {
        let (app, _) = licensed("event", PRO);
        let script = standing_script(&app.standing());
        let prefix = "window.dispatchEvent(new CustomEvent('rata-standing',{detail:";
        assert!(script.starts_with(prefix), "{script}");
        assert!(script.ends_with("}}))"), "{script}");
        let detail: Value = serde_json::from_str(&script[prefix.len()..script.len() - 3]).unwrap();
        assert_eq!(detail["licensed"], true);
        assert_eq!(detail["plan"]["key"], "pro");
    }

    /// The listener's channel: a channel of RATA's own, handed to the Swift
    /// side as Tauri names one, whose messages are transactions.
    #[test]
    fn the_listener_channel_carries_transactions() {
        rt().block_on(async {
            let heard = Arc::new(Mutex::new(Vec::new()));
            let into = heard.clone();
            let channel = tauri::ipc::Channel::<Value>::new(move |body| {
                if let Some(update) = update_in(&body) {
                    into.lock().unwrap().push(update);
                }
                Ok(())
            });
            let handler = serde_json::to_value(&channel).unwrap();
            assert!(
                handler.as_str().unwrap().starts_with("__CHANNEL__:"),
                "{handler}"
            );

            let kit = StandIn::new().then("listen", Ok(Value::Null));
            shop(&kit, "http://127.0.0.1:9/")
                .listen(handler.clone())
                .await
                .unwrap();
            assert_eq!(
                kit.asked.lock().unwrap()[0],
                ("listen", json!({ "handler": handler }))
            );

            channel.send(tx("60", PRO_ID)).unwrap();
            channel.send(json!({ "nonsense": true })).unwrap();
            let heard = heard.lock().unwrap().clone();
            assert_eq!(heard.len(), 1);
            assert_eq!(heard[0].id, "60");
            assert_eq!(heard[0].jws, jws("60"));
            assert!(update_in(&tauri::ipc::InvokeResponseBody::Raw(vec![1, 2])).is_none());
        });
    }

    #[cfg(feature = "smoke")]
    #[test]
    fn the_smoke_check_passes_when_the_plugin_answers_on_ios() {
        let listed = Answer {
            available: true,
            products: Some(vec![]),
            message: Some("no products".into()),
            ..Answer::default()
        };
        assert!(smoke_judged(Some(&listed), true).ok);
        assert!(!smoke_judged(None, true).ok);
        assert!(!smoke_judged(Some(&Answer::unavailable()), true).ok);
        assert!(smoke_judged(Some(&Answer::unavailable()), false).ok);
    }
}
