//! A self-test that a `smoke` build runs as it starts, for a device nobody
//! can watch: the iOS Simulator on GitHub's Macs (.github/workflows/ios.yml).
//!
//! It checks, on the device itself, each thing RATA leans on there and that
//! no test on a computer can stand in for: the keychain, and that its
//! entries stay on this phone; the app's folder, made private by the list
//! on a first start, and the list's lock; names looked up through the
//! system's resolver (the records discovery and L1 read), discovery of a
//! real address, a TLS connection to a real mail server through the guard;
//! what a phone answers where a computer has more (connected services,
//! Create file, updates, Microsoft sign-in, the system's opener); the
//! folder saves go to; the page itself; once the page has opened its mail
//! store, that the list's folder and the mail are kept out of the phone's
//! backups; and, when asked at launch, that a web address handed to the
//! system the way RATA hands one on a phone (`links::open_web`) is taken.
//! Each answer goes into `smoke.json` in the app's cache
//! folder (never its data folder, which the list must be the first to
//! make), written once and whole when the last check is done, for the job
//! to read.
//!
//! Never on by default, never built by release.yml, and
//! harness/verify-release.sh fails any published binary that holds the
//! words only this module puts in one. Nothing here signs in anywhere, sends
//! mail or keeps anything: the keychain entry is its own service's and is
//! deleted again, and the one address it discovers is never dialled.
//!
//! Three things are taken at launch, for the job's runs: a licence in
//! `RATA_SMOKE_LICENCE`, stored through the licence box's own path
//! (`Rata::set_licence`); a view in `RATA_SMOKE_VIEW`, which the page
//! script opens as a person would: it fills the real sign-up form for an
//! RFC 2606 address on the first screen, closes the welcome, and presses the
//! view's own button; and `RATA_SMOKE_OPEN=1`, which has the self-test open
//! [`OPEN_URL`] last of all, so the job can see the system's browser come
//! up (ios/run-smoke.sh).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tauri::State;
use tokio::sync::Notify;

use crate::core::Rata;
use crate::store::{self, Hold};
use crate::vault::{Held, Keychain, SMOKE_SERVICE, Unreadable};

/// In every `smoke.json`, and so in every smoke build: one of the words
/// harness/verify-release.sh looks for in what is published.
pub const MARK: &str = "rata-smoke-self-test";
/// The file the job reads, in the app's cache folder.
pub const FILE: &str = "smoke.json";
/// A licence to store at start, as the licence box would.
pub const LICENCE_VAR: &str = "RATA_SMOKE_LICENCE";
/// A view for the page script to open (`set` is Settings).
pub const VIEW_VAR: &str = "RATA_SMOKE_VIEW";
/// `1` has the self-test open [`OPEN_URL`] once every other check is done.
pub const OPEN_VAR: &str = "RATA_SMOKE_OPEN";
/// The address it opens: IANA's reserved example (RFC 2606), which no mail
/// names and which answers. Fixed here: the launch only says whether.
pub const OPEN_URL: &str = "https://example.com/";

/// The variables a computer's build gives the services that sign in through
/// a listener on the loopback (Microsoft for a mailbox and OneDrive, Slack,
/// Google Drive), each with whether this build was given it: a phone must
/// answer as a build without them (`desktop_only`, lib.rs), and that shows
/// only where they were given, which the job holds every run to. The names
/// alone: never a value.
const SIGN_IN_GIVEN: [(&str, bool); 4] = [
    (
        "RATA_MS_CLIENT_ID",
        option_env!("RATA_MS_CLIENT_ID").is_some(),
    ),
    (
        "RATA_SLACK_CLIENT_ID",
        option_env!("RATA_SLACK_CLIENT_ID").is_some(),
    ),
    (
        "RATA_GOOGLE_CLIENT_ID",
        option_env!("RATA_GOOGLE_CLIENT_ID").is_some(),
    ),
    (
        "RATA_GOOGLE_CLIENT_SECRET",
        option_env!("RATA_GOOGLE_CLIENT_SECRET").is_some(),
    ),
];

/// How long the page is given to load, sign up and report. Well inside the
/// job's three minutes, and far longer than a page that works needs.
const PAGE_WAIT: Duration = Duration::from_secs(120);
/// The keychain entry's name, under [`SMOKE_SERVICE`].
const ENTRY: &str = "self-test";
/// Discovered with no password: a domain the table does not know, whose
/// MX names Google, so the answer comes from DNS. Never dialled.
const DISCOVER: &str = "someone@anthropic.com";
/// A real IMAP server, for the greeting.
const IMAP_HOST: &str = "imap.gmail.com";
/// Where L1's records are read: Microsoft's minecraft.net, which publishes
/// both, an autodiscover alias and an SPF record naming Microsoft 365, as
/// the engine's own test of them on Apple's systems reads them. Its TXT
/// answer fits one UDP reply: microsoft.com's does not, and the system
/// then asks again over TCP, which the DNS GitHub's Macs use never answers
/// (rata-mail, resolve.rs).
const L1_DOMAIN: &str = "minecraft.net";

/// Script errors the page is known to have that are not RATA's, each with
/// why, matched by how they begin. They are still reported, as `expected`.
///
/// tauri-plugin-notification adds a start-up script to every page
/// (`init-iife.js`) which, on every system but Windows, asks the plugin
/// whether notifications are allowed. RATA lets its page call no plugin
/// command at all (it has no capabilities: new-mail notifications are sent
/// from Rust, `notify.rs`), so the question is refused and its promise
/// rejected with nobody to catch it. macOS and Linux have had the same
/// rejection all along.
const EXPECTED_ERRORS: [(&str, &str); 1] = [(
    "unhandled rejection: notification.is_permission_granted not allowed.",
    "the notification plugin's own start-up script asks a command RATA's page may not use",
)];

/// One check and what it found.
#[derive(Debug, Clone, Serialize)]
pub struct Check {
    pub name: &'static str,
    pub ok: bool,
    pub detail: Value,
}

impl Check {
    fn new(name: &'static str, ok: bool, detail: Value) -> Self {
        Check { name, ok, detail }
    }
}

/// What the page script reports once the page has settled.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Page {
    /// The page's file name: `auth.html`, `app.html`.
    pub path: String,
    pub title: String,
    /// The bridge's hook (`window.__RATA_NATIVE__`), which only app.html
    /// carries.
    pub bridge: bool,
    /// Tauri's own global, which bridge.js calls through.
    pub tauri: bool,
    pub width: f64,
    pub height: f64,
    pub dpr: f64,
    /// `env(safe-area-inset-*)`, in CSS pixels.
    pub safe: Insets,
    /// Script errors and failed loads since the page began, at most 20.
    pub errors: Vec<String>,
    /// The view asked for, and whether it is the one showing.
    pub view: Option<String>,
    pub shown: Option<bool>,
    /// Whether the page is shown in dark appearance
    /// (`prefers-color-scheme: dark`), which ios/run-smoke.sh holds to
    /// what the Simulator was set to.
    pub dark: bool,
    pub agent: String,
    pub after_ms: f64,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Insets {
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    pub left: f64,
}

/// What a run has found so far, and where it goes.
pub struct Smoke {
    dir: PathBuf,
    started: u64,
    view: Option<String>,
    checks: Mutex<Vec<Check>>,
    page: Mutex<Option<Page>>,
    heard: Notify,
}

impl Smoke {
    /// Begun as RATA starts, before anything is checked: the folder made,
    /// and the last run's `smoke.json` gone, so that the job never reads an
    /// old one as this run's.
    pub fn begin(dir: PathBuf, view: Option<String>) -> Arc<Self> {
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::remove_file(dir.join(FILE));
        Arc::new(Smoke {
            dir,
            started: unix_now(),
            view,
            checks: Mutex::new(Vec::new()),
            page: Mutex::new(None),
            heard: Notify::new(),
        })
    }

    pub fn record(&self, check: Check) {
        if let Ok(mut checks) = self.checks.lock() {
            checks.push(check);
        }
    }

    /// The page's report. The first is kept; a page reports once.
    fn heard(&self, page: Page) {
        if let Ok(mut held) = self.page.lock()
            && held.is_none()
        {
            *held = Some(clipped(page));
        }
        self.heard.notify_one();
    }

    async fn wait_for_page(&self, wait: Duration) -> Option<Page> {
        let held = || self.page.lock().ok().and_then(|p| p.clone());
        if let Some(page) = held() {
            return Some(page);
        }
        let _ = tokio::time::timeout(wait, self.heard.notified()).await;
        held()
    }

    /// Writes `smoke.json` whole: to a file beside it, then renamed over
    /// it, so it is never read half written.
    fn finish(&self, page: Option<Page>) -> std::io::Result<PathBuf> {
        let checks = self.checks.lock().map(|c| c.clone()).unwrap_or_default();
        let ok = !checks.is_empty() && checks.iter().all(|c| c.ok);
        let doc = json!({
            "mark": MARK,
            "version": env!("CARGO_PKG_VERSION"),
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "started": self.started,
            "finished": unix_now(),
            "ok": ok,
            "checks": checks,
            "page": page,
        });
        let path = self.dir.join(FILE);
        let part = self.dir.join(format!("{FILE}.part"));
        std::fs::write(&part, serde_json::to_vec_pretty(&doc).unwrap_or_default())?;
        std::fs::rename(&part, &path)?;
        Ok(path)
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// What the page says, kept short: the job prints it.
fn clipped(mut page: Page) -> Page {
    let cut = |s: &mut String, most: usize| {
        if s.chars().count() > most {
            *s = s.chars().take(most).collect();
        }
    };
    cut(&mut page.path, 100);
    cut(&mut page.title, 200);
    cut(&mut page.agent, 300);
    page.errors.truncate(20);
    for e in &mut page.errors {
        cut(e, 300);
    }
    page.view = page.view.and_then(|v| view_named(Some(v)));
    page
}

/// The licence given at launch, if any.
pub fn given_licence() -> Option<String> {
    std::env::var(LICENCE_VAR)
        .ok()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

/// The view asked for at launch, if it is a plain name.
pub fn asked_view() -> Option<String> {
    view_named(std::env::var(VIEW_VAR).ok())
}

/// Whether the launch asked for [`OPEN_URL`] to be opened.
fn asked_open() -> bool {
    open_asked(std::env::var(OPEN_VAR).ok())
}

fn open_asked(raw: Option<String>) -> bool {
    raw.is_some_and(|v| v.trim() == "1")
}

/// A view's name as the page names it (`inbox`, `set`): lowercase letters
/// only, since it goes into the page script and a selector.
fn view_named(raw: Option<String>) -> Option<String> {
    raw.filter(|v| (1..=16).contains(&v.len()) && v.bytes().all(|b| b.is_ascii_lowercase()))
}

// ------------------------------------------------------------- at start

/// The app's folder and the list's lock. `held` is whether this RATA got
/// the lock at start (`store::hold`); a second try must then find it taken,
/// which is what makes it a lock and not a file. `made_now` is whether the
/// folder was not there before (a first start), so that the list made it.
/// On Unix it must be this account's alone (0700), on a first start and on
/// every one after: nothing else may make it first.
pub fn store(list: &Path, held: bool, made_now: bool) -> Check {
    let Some(folder) = list.parent() else {
        return Check::new("store", false, json!({ "error": "the list has no folder" }));
    };
    let is_folder = folder.is_dir();
    let probe = folder.join(".smoke-probe");
    let writable = std::fs::write(&probe, b"").is_ok() && std::fs::remove_file(&probe).is_ok();
    let again = match store::hold(list, Duration::ZERO) {
        Hold::Mine(_) => "free",
        Hold::Taken => "taken",
        Hold::Unguarded => "unguarded",
    };
    #[cfg(unix)]
    let (mode, private) = {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(folder)
            .map(|m| m.permissions().mode() & 0o777)
            .ok();
        (
            mode.map(|m| format!("{m:o}")).unwrap_or_default(),
            mode == Some(0o700),
        )
    };
    #[cfg(not(unix))]
    let (mode, private) = (String::new(), true);
    Check::new(
        "store",
        held && is_folder && writable && again == "taken" && private,
        json!({
            "folder": folder.display().to_string(),
            "made by this start": made_now,
            "mode": mode,
            "writable": writable,
            "lock at start": if held { "held" } else { "not held" },
            "lock tried again": again,
        }),
    )
}

/// A licence given at launch, stored as the licence box stores one: through
/// `set_licence`, which checks it against the key built in. Never says the
/// token.
pub fn licence(rata: &Rata, token: &str) -> Check {
    match rata.set_licence(Some(token.to_string()), None) {
        Ok(s) => Check::new(
            "licence",
            s.licensed && s.refused.is_none(),
            json!({
                "plan": s.plan.map(|p| p.key),
                "message": s.message,
                "refused": s.refused.map(|r| r.message),
            }),
        ),
        Err(e) => Check::new("licence", false, json!({ "error": e })),
    }
}

/// The page script, run at the start of every page the window loads (never
/// in a message's frame): it gathers script errors from the first moment,
/// and once the page has settled reports what [`Page`] holds through
/// `smoke_page`. With a view asked for, it gets there first.
pub fn script(view: Option<&str>) -> String {
    let view = serde_json::to_string(&view).unwrap_or_else(|_| "null".into());
    PAGE_SCRIPT.replace("__RATA_SMOKE_VIEW__", &view)
}

const PAGE_SCRIPT: &str = r#"(() => {
  if (window.top !== window || window.__rataSmoke) return;
  window.__rataSmoke = true;
  const VIEW = __RATA_SMOKE_VIEW__;
  const started = Date.now();
  const errors = [];
  const note = (s) => { if (errors.length < 20) errors.push(String(s).slice(0, 300)); };
  addEventListener('error', (e) => {
    const t = e.target;
    if (t && t !== window && (t.src || t.href)) note('could not load ' + (t.src || t.href));
    else note((e.message || 'error') + (e.filename ? ' (' + String(e.filename).split('/').pop() + ':' + e.lineno + ')' : ''));
  }, true);
  addEventListener('unhandledrejection', (e) => {
    const r = e.reason;
    note('unhandled rejection: ' + (r && r.message ? r.message : r));
  });
  const here = () => location.pathname.split('/').pop() || 'index.html';
  const inset = (side) => {
    const d = document.createElement('div');
    d.style.position = 'fixed';
    d.style.visibility = 'hidden';
    d.style.paddingTop = 'env(safe-area-inset-' + side + ')';
    document.documentElement.appendChild(d);
    const v = parseFloat(getComputedStyle(d).paddingTop) || 0;
    d.remove();
    return v;
  };
  const report = (shown) => {
    const ipc = window.__TAURI_INTERNALS__;
    if (!ipc || typeof ipc.invoke !== 'function') return;
    ipc.invoke('smoke_page', { page: {
      path: here(),
      title: document.title,
      bridge: typeof window.__RATA_NATIVE__ === 'function',
      tauri: !!(window.__TAURI__ && window.__TAURI__.core),
      width: innerWidth,
      height: innerHeight,
      dpr: devicePixelRatio,
      safe: { top: inset('top'), right: inset('right'), bottom: inset('bottom'), left: inset('left') },
      errors,
      view: VIEW,
      shown,
      dark: matchMedia('(prefers-color-scheme: dark)').matches,
      agent: navigator.userAgent,
      afterMs: Date.now() - started,
    } }).catch(() => {});
  };
  // The first screen's own sign-up form, filled and sent as a person would:
  // a device account for an RFC 2606 address, with a password nobody keeps.
  const signUp = () => {
    const field = (id, value) => {
      const el = document.getElementById(id);
      if (!el) return;
      el.value = value;
      el.dispatchEvent(new Event('input', { bubbles: true }));
    };
    const tab = document.getElementById('tab-signup');
    if (tab) tab.click();
    const pw = Array.from(crypto.getRandomValues(new Uint8Array(12)), (b) => b.toString(16).padStart(2, '0')).join('');
    field('s-email', 'smoke@example.com');
    field('s-name', 'Smoke Test');
    field('s-pw', pw);
    const go = document.getElementById('s-next');
    if (go) go.click();
    // Still here a while later: the sign-up did not go through.
    setTimeout(() => {
      const why = document.getElementById('s-err');
      note('the sign-up did not leave ' + here() + (why && why.textContent ? ': ' + why.textContent : ''));
      report(false);
    }, 20000);
  };
  // The view's own button, after the welcome a new account is shown.
  const open = () => {
    const welcome = document.getElementById('welcome-ov');
    const enter = document.getElementById('w-enter');
    if (welcome && welcome.classList.contains('open') && enter) enter.click();
    const button = document.querySelector('#mnav button[data-view="' + VIEW + '"]') || document.querySelector('[data-view="' + VIEW + '"]');
    if (button) button.click();
    setTimeout(() => {
      const view = document.getElementById('view-' + VIEW);
      report(!!(view && view.classList.contains('active')));
    }, 1500);
  };
  const settle = () => {
    const page = here();
    if (page === 'index.html') return;
    if (VIEW && page === 'auth.html') return signUp();
    if (VIEW && page === 'app.html') return open();
    report(null);
  };
  const later = () => setTimeout(settle, 2500);
  if (document.readyState === 'complete') later();
  else addEventListener('load', later);
})();"#;

/// The page's report.
#[tauri::command]
pub fn smoke_page(smoke: State<'_, Arc<Smoke>>, page: Page) {
    smoke.heard(page);
}

/// The rest, once the window is up: the keychain, the network, what a
/// phone answers and where it saves, then the page's report (waited for),
/// the backups once the page has opened its mail store, a web address
/// opened when the launch asked (last, since the browser then comes in
/// front of RATA and its page), and `smoke.json`.
pub fn start(smoke: Arc<Smoke>, handle: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let secret = fresh_secret();
        let kept = tauri::async_runtime::spawn_blocking(move || keychain(&InKeychain, &secret))
            .await
            .unwrap_or_else(|e| Check::new("keychain", false, json!({ "error": e.to_string() })));
        smoke.record(kept);
        network(&smoke).await;
        smoke.record(judge_phone(&phone_answers(&handle).await));
        smoke.record(saves(&handle));
        let page = smoke.wait_for_page(PAGE_WAIT).await;
        smoke.record(judge_page(page.as_ref(), smoke.view.as_deref()));
        smoke.record(backups(&handle, page.as_ref()));
        if asked_open() {
            let opener = handle.clone();
            let opened = tauri::async_runtime::spawn_blocking(move || opens_a_link(&opener))
                .await
                .unwrap_or_else(|e| judge_open(OPEN_URL, Err(e.to_string())));
            smoke.record(opened);
        }
        if let Err(e) = smoke.finish(page) {
            eprintln!("{MARK}: {FILE} could not be written: {e}");
        }
    });
}

fn fresh_secret() -> String {
    let mut b = [0u8; 16];
    let _ = getrandom::fill(&mut b);
    b.iter().map(|x| format!("{x:02x}")).collect()
}

// ------------------------------------------------------------- keychain

/// One keychain entry, as the check sees it: [`SMOKE_SERVICE`]'s in a
/// smoke build, a map in the tests.
trait Slot {
    /// Written as RATA writes one (`vault`).
    fn put(&self, secret: &str) -> Result<(), String>;
    /// Written as `keyring` writes one, which on iOS is to travel: what
    /// RATA's own writer must write over.
    fn put_as_keyring(&self, secret: &str) -> Result<(), String>;
    fn get(&self) -> Result<String, Unreadable>;
    fn forget(&self) -> Result<(), String>;
    /// How the keychain holds it, which only iOS says.
    fn held(&self) -> Result<Held, String>;
}

struct InKeychain;

impl Slot for InKeychain {
    fn put(&self, secret: &str) -> Result<(), String> {
        Keychain::put_smoke(ENTRY, secret)
    }
    fn put_as_keyring(&self, secret: &str) -> Result<(), String> {
        Keychain::put_smoke_as_keyring(ENTRY, secret)
    }
    fn get(&self) -> Result<String, Unreadable> {
        Keychain::get_smoke(ENTRY)
    }
    fn forget(&self) -> Result<(), String> {
        Keychain::forget_smoke(ENTRY)
    }
    fn held(&self) -> Result<Held, String> {
        Keychain::held_smoke(ENTRY)
    }
}

/// On iOS, where RATA writes its own entries to stay on the phone (vault.rs).
fn keychain(slot: &dyn Slot, secret: &str) -> Check {
    keychain_where(slot, secret, cfg!(target_os = "ios"))
}

/// Written over an entry `keyring` wrote, read back, deleted, and then
/// really gone; where `stays_here`, also held as one entry that stays on
/// this phone (`kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly`) and is
/// never synchronized. An entry an earlier run left (one stopped halfway)
/// is cleared first. The secret is never in what is reported.
fn keychain_where(slot: &dyn Slot, secret: &str, stays_here: bool) -> Check {
    let mut held: Option<Held> = None;
    let mut run = || -> Result<(), String> {
        slot.forget()
            .map_err(|e| format!("an earlier run's entry could not be cleared: {e}"))?;
        slot.put_as_keyring("written to travel")
            .map_err(|e| format!("an entry could not be written as keyring writes one: {e}"))?;
        slot.put(secret)
            .map_err(|e| format!("it could not be written: {e}"))?;
        match slot.get() {
            Ok(s) if s == secret => {}
            Ok(_) => return Err("it read back as something else".into()),
            Err(e) => return Err(format!("it could not be read back: {e}")),
        }
        if stays_here {
            let h = slot
                .held()
                .map_err(|e| format!("how it is kept could not be read: {e}"))?;
            let fine = h.here && h.entries == 1 && !h.synchronized;
            held = Some(h);
            if !fine {
                return Err("it is not kept as one entry on this phone alone".into());
            }
        }
        slot.forget()
            .map_err(|e| format!("it could not be deleted: {e}"))?;
        match slot.get() {
            Err(Unreadable::Missing(_)) => Ok(()),
            Ok(_) => Err("it was still there once deleted".into()),
            Err(Unreadable::Locked(e)) => {
                Err(format!("whether it was deleted could not be read: {e}"))
            }
        }
    };
    let ran = run();
    let mut detail = json!({
        "service": SMOKE_SERVICE,
        "written over keyring's, read back and deleted": ran.is_ok(),
    });
    if let Some(h) = held {
        detail["entries"] = json!(h.entries);
        detail["accessible"] = json!(h.classes);
        detail["this device only"] = json!(h.here);
        detail["synchronized"] = json!(h.synchronized);
    }
    if let Err(e) = &ran {
        detail["error"] = json!(e);
    }
    Check::new("keychain", ran.is_ok(), detail)
}

// ------------------------------------------------------- what a phone has

/// What a phone answers where a computer has more, each found the way the
/// page asks for it.
#[derive(Debug, Clone)]
struct PhoneAnswers {
    /// Each connected service and whether it is offered (STAGE 3 items 7
    /// and 9: none is, on a phone).
    connections: Vec<(&'static str, bool)>,
    /// Create file's list: refused, with its kind (item 7).
    create_file: Result<usize, &'static str>,
    /// Microsoft mailbox sign-in (item 9).
    microsoft: bool,
    /// An update key, and whether a check would ask anything (the store
    /// updates a phone's RATA).
    update_key: bool,
    update_check: bool,
    /// The opener plugin's mobile half, which opens a web address for RATA
    /// (`links::open_web`), is set up.
    opener: bool,
    /// The sign-in variables this build was given, by name
    /// ([`SIGN_IN_GIVEN`]).
    given: Vec<&'static str>,
}

async fn phone_answers(handle: &tauri::AppHandle) -> PhoneAnswers {
    use tauri::Manager;
    let rata = handle.state::<Arc<Rata>>();
    PhoneAnswers {
        connections: rata
            .connections_status()
            .into_iter()
            .map(|s| (s.service, s.available))
            .collect(),
        create_file: rata.created_listing().map(|l| l.len()).map_err(|r| r.kind),
        microsoft: rata.microsoft_ready(),
        update_key: crate::update::has_key(handle.config()),
        update_check: crate::update::check(handle).await.enabled,
        opener: handle
            .try_state::<tauri_plugin_opener::Opener<tauri::Wry>>()
            .is_some(),
        given: SIGN_IN_GIVEN
            .iter()
            .filter(|(_, given)| *given)
            .map(|(name, _)| *name)
            .collect(),
    }
}

/// Nothing a phone cannot do is offered, and the opener is there. Which
/// sign-in variables the build was given is said, not judged: a build made
/// by hand may have none, and the job holds its own builds to all four.
fn judge_phone(a: &PhoneAnswers) -> Check {
    let mut wrong: Vec<String> = Vec::new();
    if a.connections.is_empty() {
        wrong.push("no connected services were listed".into());
    }
    for (service, available) in &a.connections {
        if *available {
            wrong.push(format!("{service} is offered"));
        }
    }
    if a.create_file != Err("unavailable") {
        wrong.push(format!("Create file answered {:?}", a.create_file));
    }
    if a.microsoft {
        wrong.push("Microsoft sign-in is offered".into());
    }
    if a.update_key || a.update_check {
        wrong.push("this copy would check for updates of its own".into());
    }
    if !a.opener {
        wrong.push("the opener is not set up".into());
    }
    Check::new(
        "phone answers",
        wrong.is_empty(),
        json!({
            "connections": a.connections
                .iter()
                .map(|(s, on)| format!("{s}: {}", if *on { "offered" } else { "not offered" }))
                .collect::<Vec<_>>(),
            "create file": match a.create_file {
                Ok(n) => format!("lists {n}"),
                Err(kind) => format!("refused: {kind}"),
            },
            "microsoft sign-in": a.microsoft,
            "update key": a.update_key,
            "update check": a.update_check,
            "opener": a.opener,
            "given to the build": a.given,
            "problems": wrong,
        }),
    )
}

/// [`OPEN_URL`] handed to the system exactly as RATA hands a link on a
/// phone: through `links::classify`, then `links::open_web`, which there is
/// the opener plugin's mobile half. Whether the system's browser then came
/// up is for the job to see (ios/run-smoke.sh).
fn opens_a_link(handle: &tauri::AppHandle) -> Check {
    let opened = match crate::links::classify(OPEN_URL) {
        Some(crate::links::Link::Web(url)) => crate::links::open_web(handle, &url),
        _ => Err("links::classify did not take it for a web address".into()),
    };
    judge_open(OPEN_URL, opened)
}

fn judge_open(url: &str, opened: Result<(), String>) -> Check {
    Check::new(
        "opens a link",
        opened.is_ok(),
        json!({
            "address": url,
            "handed to the system": opened.is_ok(),
            "error": opened.err(),
        }),
    )
}

/// Where saves go (`commands::downloads`), and that a file can be written
/// there and removed again.
fn saves(handle: &tauri::AppHandle) -> Check {
    use tauri::Manager;
    let folder = crate::commands::downloads(handle);
    let home = handle.path().home_dir().ok();
    let wrote = match &folder {
        Some(f) => {
            let probe = f.join(".rata-smoke-probe");
            std::fs::write(&probe, b"RATA")
                .and_then(|()| std::fs::remove_file(&probe))
                .map_err(|e| e.to_string())
        }
        None => Err("there is no folder".into()),
    };
    judge_saves(folder.as_deref(), home.as_deref(), wrote)
}

/// On iOS a save goes to the app's own Documents folder, inside its
/// container, which the Files app shows (prepare.sh's Info.plist keys), and
/// takes a file.
fn judge_saves(folder: Option<&Path>, home: Option<&Path>, wrote: Result<(), String>) -> Check {
    let inside = match (folder, home) {
        (Some(f), Some(h)) => f.strip_prefix(h).ok().map(|r| r.display().to_string()),
        _ => None,
    };
    let ok = inside.as_deref() == Some("Documents") && wrote.is_ok();
    Check::new(
        "saves",
        ok,
        json!({
            "folder in the container": inside,
            "folder": folder.map(|f| f.display().to_string()),
            "written and removed": wrote.is_ok(),
            "error": wrote.err(),
        }),
    )
}

// ------------------------------------------------------------- backups

/// Once the page has opened its mail store: RATA's folder and each folder
/// WebKit keeps IndexedDB in, as `backup` finds them, each read back from
/// the system as kept out of backups. Read only: what this checks is what
/// RATA's start and the page's licence question already did.
fn backups(handle: &tauri::AppHandle, page: Option<&Page>) -> Check {
    use tauri::Manager;
    let paths = handle.path();
    let (Ok(app), Ok(home)) = (paths.app_local_data_dir(), paths.home_dir()) else {
        return Check::new("backups", false, json!({ "error": "no folders" }));
    };
    let webkit = crate::backup::webkit_in(&home);
    let found: Vec<(String, Result<bool, String>)> = crate::backup::folders(&app, &webkit)
        .iter()
        .map(|f| {
            let shown = f.strip_prefix(&home).unwrap_or(f).display().to_string();
            (shown, crate::backup::excluded(f))
        })
        .collect();
    let mail_open = page.is_some_and(|p| p.path == "app.html");
    let mut check = judge_backups(&found, mail_open);
    if found.len() < 2 {
        check.detail["webkit holds"] = json!(listing(&webkit, &home, 5));
    }
    check
}

/// RATA's folder (first) is kept out, and so is each IndexedDB folder;
/// where the page has opened its mail store, there is one at least.
fn judge_backups(found: &[(String, Result<bool, String>)], mail_open: bool) -> Check {
    let mut wrong: Vec<String> = Vec::new();
    if found.is_empty() {
        wrong.push("RATA's folder was not found".into());
    }
    if mail_open && found.len() < 2 {
        wrong.push("no folder of WebKit's holds the mail".into());
    }
    for (folder, excluded) in found {
        match excluded {
            Ok(true) => {}
            Ok(false) => wrong.push(format!("{folder} is backed up")),
            Err(e) => wrong.push(format!("{folder}: {e}")),
        }
    }
    Check::new(
        "backups",
        wrong.is_empty(),
        json!({
            "kept out": found
                .iter()
                .map(|(f, e)| format!("{f}: {}", match e {
                    Ok(true) => "kept out".to_string(),
                    Ok(false) => "BACKED UP".to_string(),
                    Err(why) => why.clone(),
                }))
                .collect::<Vec<_>>(),
            "mail store open": mail_open,
            "problems": wrong,
        }),
    )
}

/// The folders under `dir`, no deeper than `depth`, relative to `home`, at
/// most 80: what WebKit made, when no mail store was found in it.
fn listing(dir: &Path, home: &Path, depth: usize) -> Vec<String> {
    fn walk(dir: &Path, home: &Path, depth: usize, out: &mut Vec<String>) {
        if depth == 0 || out.len() >= 80 {
            return;
        }
        let Ok(read) = std::fs::read_dir(dir) else {
            return;
        };
        let mut entries: Vec<_> = read.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            if out.len() >= 80 {
                return;
            }
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                let p = e.path();
                out.push(p.strip_prefix(home).unwrap_or(&p).display().to_string());
                walk(&p, home, depth - 1, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, home, depth, &mut out);
    out
}

// ------------------------------------------------------------- network

/// Names looked up through the engine's resolver (the system's own on
/// iOS), discovery, and a greeting through the guard.
async fn network(smoke: &Smoke) {
    let r = match rata_mail::Resolver::system() {
        Ok(r) => r,
        Err(e) => {
            smoke.record(Check::new("lookups", false, json!({ "error": e })));
            return;
        }
    };
    smoke.record(judge_mx(&r.mx("gmail.com").await));
    smoke.record(judge_srv(r.srv_imaps("gmail.com").await));
    // The SPF record, then the autodiscover alias. On iOS the alias is
    // never a CNAME question, which held up later questions reaching the
    // name: the system follows the alias on its way to the name's addresses
    // and hands it back (Resolver::cname, rata-mail's dns_sd), and this
    // proves that path on the device.
    let txt = r.txt(L1_DOMAIN).await;
    let alias = r.cname(&format!("autodiscover.{L1_DOMAIN}")).await;
    smoke.record(judge_l1(&txt, alias));
    smoke.record(judge_discovery(
        rata_mail::resolve::discover(&r, DISCOVER, None).await,
    ));
    smoke.record(judge_greeting(
        rata_mail::imap::greeting(&r, IMAP_HOST, rata_mail::discover::IMAP_PORT).await,
    ));
}

/// gmail.com's MX records, all Google's.
fn judge_mx(found: &[String]) -> Check {
    let ok = !found.is_empty()
        && found
            .iter()
            .all(|e| e.trim_end_matches('.').ends_with(".google.com"));
    Check::new("mx", ok, json!({ "gmail.com": found }))
}

/// gmail.com's RFC 6186 record, which names its IMAP server.
fn judge_srv(found: Option<(String, u16)>) -> Check {
    let ok = found
        .as_ref()
        .is_some_and(|(h, p)| h == IMAP_HOST && *p == rata_mail::discover::IMAP_PORT);
    Check::new(
        "srv",
        ok,
        json!({ "_imaps._tcp.gmail.com": found.map(|(h, p)| format!("{h}:{p}")) }),
    )
}

/// What L1 reads of a domain behind a mail filter, asked of [`L1_DOMAIN`]:
/// its SPF record (a TXT record, read strictly), whose mailbox providers
/// must be Microsoft 365 alone, and its autodiscover alias, which must name
/// Microsoft 365 too.
fn judge_l1(txt: &[String], alias: Option<String>) -> Check {
    use rata_mail::discover::{
        MS_IMAP, autodiscover_provider, parse_spf, spf_provider, spf_record,
    };
    let spf = spf_record(txt).and_then(parse_spf);
    let named: Vec<&str> = spf
        .iter()
        .flat_map(|s| s.includes.iter())
        .filter_map(|i| spf_provider(i))
        .map(|h| h.host)
        .collect();
    let by_spf = !named.is_empty() && named.iter().all(|h| *h == MS_IMAP);
    let by_alias = alias
        .as_deref()
        .and_then(autodiscover_provider)
        .is_some_and(|h| h.host == MS_IMAP);
    Check::new(
        "l1 records",
        by_spf && by_alias,
        json!({
            format!("{L1_DOMAIN} txt records"): txt.len(),
            "spf includes": spf.map(|s| s.includes),
            "spf names": named,
            format!("autodiscover.{L1_DOMAIN}"): alias,
        }),
    )
}

/// Where an address's mail is, with no password: the MX names Google.
fn judge_discovery(found: rata_mail::Discovery) -> Check {
    match found {
        rata_mail::Discovery::Candidates { hosts, .. } => {
            let first = hosts.first();
            Check::new(
                "discovery",
                first.is_some_and(|c| c.host == IMAP_HOST),
                json!({
                    "address": DISCOVER,
                    "candidates": hosts
                        .iter()
                        .map(|c| format!("{}:{} ({:?})", c.host, c.port, c.source))
                        .collect::<Vec<_>>(),
                }),
            )
        }
        rata_mail::Discovery::Refuse(why) => Check::new(
            "discovery",
            false,
            json!({ "address": DISCOVER, "refused": why }),
        ),
    }
}

/// A real IMAP server's first line, through the guard and TLS.
fn judge_greeting(said: Result<String, String>) -> Check {
    match said {
        Ok(line) => Check::new(
            "tls",
            line.starts_with("* OK"),
            json!({ "server": format!("{IMAP_HOST}:993"), "greeting": line }),
        ),
        Err(e) => Check::new(
            "tls",
            false,
            json!({ "server": format!("{IMAP_HOST}:993"), "error": e }),
        ),
    }
}

// ------------------------------------------------------------- the page

/// The page loaded, is RATA's, has no script errors but the expected ones
/// ([`EXPECTED_ERRORS`]), has the bridge where it should (app.html), and
/// showed the view asked for.
fn judge_page(page: Option<&Page>, view: Option<&str>) -> Check {
    let Some(p) = page else {
        return Check::new("page", false, json!({ "error": "the page never reported" }));
    };
    let mut wrong: Vec<String> = Vec::new();
    if !p.title.contains("RATA") {
        wrong.push(format!("its title is {:?}", p.title));
    }
    if p.width <= 0.0 || p.height <= 0.0 {
        wrong.push("it has no size".into());
    }
    if p.path == "app.html" && !(p.bridge && p.tauri) {
        wrong.push("the bridge is not there".into());
    }
    let mut expected: Vec<String> = Vec::new();
    let mut errors: Vec<&str> = Vec::new();
    for e in &p.errors {
        match EXPECTED_ERRORS
            .iter()
            .find(|(start, _)| e.starts_with(start))
        {
            Some((_, why)) => expected.push(format!("{e} ({why})")),
            None => errors.push(e),
        }
    }
    if !errors.is_empty() {
        wrong.push(format!(
            "{} script error(s): {}",
            errors.len(),
            errors.join(" | ")
        ));
    }
    if let Some(v) = view {
        if p.path != "app.html" {
            wrong.push(format!("it stopped at {} before the {v} view", p.path));
        } else if p.shown != Some(true) {
            wrong.push(format!("the {v} view did not open"));
        }
    }
    Check::new(
        "page",
        wrong.is_empty(),
        json!({ "path": p.path, "problems": wrong, "expected": expected }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;
    use crate::vault::Memory;

    fn tmpdir(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("rata-smoke-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn the_page_script_takes_only_a_plain_view_name() {
        assert_eq!(view_named(Some("set".into())).as_deref(), Some("set"));
        for refused in [
            "",
            "Set",
            "set\"",
            "a b",
            "inbox');alert(1)//",
            "abcdefghijklmnopq",
        ] {
            assert_eq!(view_named(Some(refused.into())), None, "{refused:?}");
        }
        let plain = script(None);
        assert!(plain.contains("const VIEW = null;"), "{plain}");
        let set = script(Some("set"));
        assert!(set.contains("const VIEW = \"set\";"));
        assert!(!set.contains("__RATA_SMOKE_VIEW__"));
        assert!(set.contains("'smoke_page'"));
        // The appearance the page is shown in, which run-smoke.sh holds to
        // the Simulator's; a report from before the field reads as light.
        assert!(set.contains("dark: matchMedia('(prefers-color-scheme: dark)').matches"));
        let page: Page =
            serde_json::from_value(json!({ "path": "app.html", "dark": true })).unwrap();
        assert!(page.dark);
        let page: Page = serde_json::from_value(json!({ "path": "app.html" })).unwrap();
        assert!(!page.dark);
    }

    /// An entry in a map, which can be made to misbehave. Its class is what
    /// the iOS keychain would say: `ak` (when unlocked, and into backups) as
    /// `keyring` writes one, `cku` (after first unlock, this device only) as
    /// RATA's own writer does, unless the fake writes as `keyring` does.
    /// With `synchronized_twin`, iCloud Keychain also holds one of that
    /// service and account, which a read-back asking for synchronized
    /// items too (`vault::ios_every`) finds.
    #[derive(Default)]
    struct Fake {
        held: Mutex<Option<String>>,
        class: Mutex<&'static str>,
        reads_wrong: bool,
        keeps_on_forget: bool,
        writes_as_keyring: bool,
        synchronized_twin: bool,
    }

    impl Slot for Fake {
        fn put(&self, secret: &str) -> Result<(), String> {
            *self.held.lock().unwrap() = Some(secret.into());
            if !self.writes_as_keyring {
                *self.class.lock().unwrap() = "cku";
            }
            Ok(())
        }
        fn put_as_keyring(&self, secret: &str) -> Result<(), String> {
            *self.held.lock().unwrap() = Some(secret.into());
            *self.class.lock().unwrap() = "ak";
            Ok(())
        }
        fn get(&self) -> Result<String, Unreadable> {
            match self.held.lock().unwrap().clone() {
                Some(_) if self.reads_wrong => Ok("something else".into()),
                Some(s) => Ok(s),
                None => Err(Unreadable::Missing("gone".into())),
            }
        }
        fn forget(&self) -> Result<(), String> {
            if !self.keeps_on_forget {
                *self.held.lock().unwrap() = None;
            }
            Ok(())
        }
        fn held(&self) -> Result<Held, String> {
            let class = *self.class.lock().unwrap();
            let there = self.held.lock().unwrap().is_some();
            let mut classes = vec![class.to_string()];
            if self.synchronized_twin {
                classes.push("ak".into());
            }
            Ok(Held {
                entries: usize::from(there) + usize::from(self.synchronized_twin),
                classes,
                here: there && class == "cku" && !self.synchronized_twin,
                synchronized: self.synchronized_twin,
            })
        }
    }

    #[test]
    fn a_keychain_entry_is_written_read_back_and_deleted() {
        let secret = fresh_secret();
        assert_eq!(secret.len(), 32);
        let good = Fake::default();
        // Left by an earlier run that stopped halfway: cleared first.
        good.put("left behind").unwrap();
        let c = keychain_where(&good, &secret, false);
        assert!(c.ok, "{c:?}");
        assert!(good.held.lock().unwrap().is_none());

        let wrong = Fake {
            reads_wrong: true,
            ..Fake::default()
        };
        let c = keychain_where(&wrong, &secret, false);
        assert!(
            !c.ok && c.detail.to_string().contains("something else"),
            "{c:?}"
        );

        let sticky = Fake {
            keeps_on_forget: true,
            ..Fake::default()
        };
        let c = keychain_where(&sticky, &secret, false);
        assert!(
            !c.ok && c.detail.to_string().contains("still there"),
            "{c:?}"
        );
        // The secret is never what is reported.
        for c in [
            keychain_where(&wrong, &secret, true),
            keychain_where(&sticky, &secret, true),
        ] {
            assert!(!c.detail.to_string().contains(&secret), "{c:?}");
        }
    }

    /// On iOS the entry RATA writes over one `keyring` wrote must then be
    /// one entry, kept on this phone alone and never synchronized; a writer
    /// that leaves `keyring`'s class (into backups) fails the check there,
    /// and only there.
    #[test]
    fn on_ios_an_entry_must_be_kept_on_this_phone_alone() {
        let secret = fresh_secret();
        let good = Fake::default();
        let c = keychain_where(&good, &secret, true);
        assert!(c.ok, "{c:?}");
        assert_eq!(c.detail["accessible"][0], "cku");
        assert_eq!(c.detail["entries"], 1);
        assert_eq!(c.detail["this device only"], true);

        let travels = Fake {
            writes_as_keyring: true,
            ..Fake::default()
        };
        let c = keychain_where(&travels, &secret, true);
        assert!(!c.ok, "{c:?}");
        assert_eq!(c.detail["accessible"][0], "ak");
        assert!(
            c.detail["error"]
                .as_str()
                .unwrap()
                .contains("on this phone alone")
        );
        assert!(keychain_where(&travels, &secret, false).ok);

        // A twin in iCloud Keychain, of the same service and account, is
        // counted and fails the check, however RATA's own entry is kept.
        let twin = Fake {
            synchronized_twin: true,
            ..Fake::default()
        };
        let c = keychain_where(&twin, &secret, true);
        assert!(!c.ok, "{c:?}");
        assert_eq!(c.detail["entries"], 2);
        assert_eq!(c.detail["synchronized"], true);
        assert!(
            c.detail["error"]
                .as_str()
                .unwrap()
                .contains("on this phone alone")
        );
    }

    /// What a phone answers where a computer has more: every service not
    /// offered, Create file refused as unavailable, no Microsoft sign-in, no
    /// update of its own, and the opener set up. Any one otherwise fails.
    #[test]
    fn a_phone_offers_nothing_it_cannot_do() {
        let phone = PhoneAnswers {
            connections: vec![("apple", false), ("slack", false), ("google", false)],
            create_file: Err("unavailable"),
            microsoft: false,
            update_key: false,
            update_check: false,
            opener: true,
            given: vec!["RATA_MS_CLIENT_ID", "RATA_GOOGLE_CLIENT_SECRET"],
        };
        let c = judge_phone(&phone);
        assert!(c.ok, "{c:?}");
        assert_eq!(c.detail["create file"], "refused: unavailable");
        // Said by name, for the job; given or not, nothing fails on it.
        assert_eq!(
            c.detail["given to the build"],
            json!(["RATA_MS_CLIENT_ID", "RATA_GOOGLE_CLIENT_SECRET"])
        );
        let mut nothing_given = phone.clone();
        nothing_given.given.clear();
        assert!(judge_phone(&nothing_given).ok);

        let mut slack = phone.clone();
        slack.connections[1].1 = true;
        let mut create = phone.clone();
        create.create_file = Ok(0);
        let mut plan = phone.clone();
        plan.create_file = Err("plan");
        let mut microsoft = phone.clone();
        microsoft.microsoft = true;
        let mut updates = phone.clone();
        updates.update_check = true;
        let mut opener = phone.clone();
        opener.opener = false;
        let mut none = phone.clone();
        none.connections.clear();
        for (why, answers) in [
            ("slack is offered", slack),
            ("Create file answered Ok(0)", create),
            ("Create file answered Err(\"plan\")", plan),
            ("Microsoft sign-in is offered", microsoft),
            ("check for updates", updates),
            ("opener is not set up", opener),
            ("no connected services", none),
        ] {
            let c = judge_phone(&answers);
            assert!(!c.ok, "{why}: {c:?}");
            let said = c.detail["problems"].as_array().unwrap();
            assert!(
                said.iter().any(|p| p.as_str().unwrap().contains(why)),
                "{why}: {c:?}"
            );
        }
    }

    /// The variables named are the four the sign-in modules read, and the
    /// address opened is one `links::classify` takes for the web, as every
    /// link RATA opens must be.
    #[test]
    fn what_a_phone_is_given_and_opens_is_what_ratas_links_are() {
        let names: Vec<&str> = SIGN_IN_GIVEN.iter().map(|(n, _)| *n).collect();
        assert_eq!(
            names,
            [
                "RATA_MS_CLIENT_ID",
                "RATA_SLACK_CLIENT_ID",
                "RATA_GOOGLE_CLIENT_ID",
                "RATA_GOOGLE_CLIENT_SECRET"
            ]
        );
        assert!(matches!(
            crate::links::classify(OPEN_URL),
            Some(crate::links::Link::Web(_))
        ));
    }

    /// Opening is asked for by `1` alone, and judged on whether the system
    /// took the address.
    #[test]
    fn a_link_is_opened_only_when_asked_and_judged_on_the_answer() {
        assert!(open_asked(Some("1".into())));
        assert!(open_asked(Some(" 1\n".into())));
        for no in [
            None,
            Some(String::new()),
            Some("0".into()),
            Some("yes".into()),
        ] {
            assert!(!open_asked(no.clone()), "{no:?}");
        }
        let c = judge_open(OPEN_URL, Ok(()));
        assert!(c.ok, "{c:?}");
        assert_eq!(c.name, "opens a link");
        assert_eq!(c.detail["address"], OPEN_URL);
        let c = judge_open(OPEN_URL, Err("no browser".into()));
        assert!(!c.ok);
        assert_eq!(c.detail["error"], "no browser");
    }

    /// A save goes to the container's own Documents folder, which takes a
    /// file; anywhere else, or a folder refusing the file, fails.
    #[test]
    fn a_phone_saves_into_its_own_documents() {
        let home = Path::new("/var/mobile/Containers/Data/Application/A1");
        let docs = home.join("Documents");
        let c = judge_saves(Some(&docs), Some(home), Ok(()));
        assert!(c.ok, "{c:?}");
        assert_eq!(c.detail["folder in the container"], "Documents");
        assert!(!judge_saves(Some(&docs), Some(home), Err("read-only".into())).ok);
        let downloads = home.join("Downloads");
        assert!(!judge_saves(Some(&downloads), Some(home), Ok(())).ok);
        let outside = Path::new("/var/mobile/Documents");
        assert!(!judge_saves(Some(outside), Some(home), Ok(())).ok);
        assert!(!judge_saves(None, Some(home), Err("no folder".into())).ok);
    }

    /// RATA's folder must be kept out of backups, and so must every folder
    /// WebKit keeps IndexedDB in; once the page has opened its mail store,
    /// there must be one.
    #[test]
    fn the_list_and_the_mail_are_kept_out_of_backups() {
        let ours = (
            "Library/Application Support/org.mailrata.desktop".to_string(),
            Ok(true),
        );
        let mail = ("Library/WebKit/WebsiteData/IndexedDB".to_string(), Ok(true));
        assert!(judge_backups(&[ours.clone(), mail.clone()], true).ok);
        // The first screen opens no mail store.
        assert!(judge_backups(std::slice::from_ref(&ours), false).ok);
        let c = judge_backups(std::slice::from_ref(&ours), true);
        assert!(!c.ok && c.detail["problems"].to_string().contains("holds the mail"));
        let backed = (mail.0.clone(), Ok(false));
        let c = judge_backups(&[ours.clone(), backed], true);
        assert!(!c.ok && c.detail["problems"].to_string().contains("is backed up"));
        let unread = (ours.0.clone(), Err("the system would not say".into()));
        assert!(!judge_backups(&[unread, mail], true).ok);
        assert!(!judge_backups(&[], false).ok);
    }

    #[test]
    fn a_listing_of_webkit_is_short_and_relative() {
        let home = tmpdir("listing");
        let webkit = crate::backup::webkit_in(&home);
        std::fs::create_dir_all(webkit.join("WebsiteData/Default/a")).unwrap();
        std::fs::write(webkit.join("WebsiteData/file"), b"").unwrap();
        assert_eq!(
            listing(&webkit, &home, 5),
            vec![
                "Library/WebKit/WebsiteData",
                "Library/WebKit/WebsiteData/Default",
                "Library/WebKit/WebsiteData/Default/a",
            ]
        );
        for n in 0..100 {
            std::fs::create_dir_all(webkit.join(format!("many/{n:03}"))).unwrap();
        }
        assert_eq!(listing(&webkit, &home, 5).len(), 80);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn the_store_check_needs_this_rata_to_hold_the_lists_lock() {
        let dir = tmpdir("store");
        let list = dir.join("org.mailrata.desktop/mailboxes.json");
        let Hold::Mine(lock) = store::hold(&list, Duration::ZERO) else {
            panic!("the lock should be free");
        };
        let c = store(&list, true, true);
        assert!(c.ok, "{c:?}");
        assert_eq!(c.detail["lock tried again"], "taken");
        assert_eq!(c.detail["writable"], true);
        assert_eq!(c.detail["made by this start"], true);
        assert!(!list.parent().unwrap().join(".smoke-probe").exists());
        drop(lock);
        // Let go of: a second try gets it, which is no lock at all.
        let c = store(&list, true, false);
        assert!(!c.ok && c.detail["lock tried again"] == "free", "{c:?}");
        // Not held at start (another copy had it, or locks are refused).
        assert!(!store(&list, false, false).ok);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The list makes its folder this account's alone (0700). A folder
    /// something else made first, open to others, fails the check: it
    /// would describe what made it, not RATA.
    #[cfg(unix)]
    #[test]
    fn the_store_check_needs_the_folder_to_be_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tmpdir("store-mode");
        let list = dir.join("org.mailrata.desktop/mailboxes.json");
        let Hold::Mine(lock) = store::hold(&list, Duration::ZERO) else {
            panic!("the lock should be free");
        };
        let c = store(&list, true, true);
        assert!(c.ok && c.detail["mode"] == "700", "{c:?}");
        drop(lock);

        let other = dir.join("made-first/mailboxes.json");
        std::fs::create_dir_all(other.parent().unwrap()).unwrap();
        std::fs::set_permissions(
            other.parent().unwrap(),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        let Hold::Mine(_lock) = store::hold(&other, Duration::ZERO) else {
            panic!("the lock should be free");
        };
        let c = store(&other, true, false);
        assert!(!c.ok && c.detail["mode"] == "755", "{c:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The fixture of core.rs's tests: a key pair made by
    /// rata-next/lib/licence.js, and a Pro licence that lasts until 2108.
    const KEY: &str = "-----BEGIN PUBLIC KEY-----\nMCowBQYDK2VwAyEA+pogY6bod0k5ez7c/lE4N1/X2/5sbonmcLhIb7Oqrzs=\n-----END PUBLIC KEY-----";
    const PRO: &str = "v1.eyJ2IjoxLCJzdWIiOiJidXllckBleGFtcGxlLmNvbSIsInBsYW4iOiJwcm8iLCJpYXQiOjE3ODk1NzMyMzUsImV4cCI6NDM4MTU3MzIzNX0.cSIyj1Xy5bhvD5sph49VZPSnPZkzvRd5zEERGN5b76g-pTJ4IobTVQBfSDDXMSAqHlNx3G14NYoT5OdK6hvUDw";

    #[test]
    fn a_licence_given_at_launch_is_stored_as_the_licence_box_stores_one() {
        let dir = tmpdir("licence");
        let rata = Rata::new(
            Store::open(dir.join("mailboxes.json")),
            Box::new(Memory::default()),
            rata_mail::Resolver::system().expect("resolver"),
            Some(KEY),
        );
        let c = licence(&rata, "  not a licence  ");
        assert!(!c.ok, "{c:?}");
        assert!(!rata.standing().licensed);

        // Wrapped and padded as a mail client might leave it.
        let c = licence(&rata, &format!("\n{PRO}\n"));
        assert!(c.ok, "{c:?}");
        assert_eq!(c.detail["plan"], "pro");
        assert!(!c.detail.to_string().contains(PRO));
        let standing = rata.standing();
        assert!(standing.licensed);
        assert_eq!(standing.token.as_deref(), Some(PRO));
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn page(path: &str, title: &str) -> Page {
        Page {
            path: path.into(),
            title: title.into(),
            bridge: path == "app.html",
            tauri: true,
            width: 393.0,
            height: 852.0,
            dpr: 3.0,
            ..Page::default()
        }
    }

    #[test]
    fn the_page_is_judged_by_its_title_its_bridge_its_errors_and_its_view() {
        assert!(judge_page(Some(&page("auth.html", "Sign in to RATA")), None).ok);
        assert!(judge_page(Some(&page("app.html", "RATA — Your mail")), None).ok);
        assert!(!judge_page(None, None).ok);
        assert!(!judge_page(Some(&page("app.html", "Example Domain")), None).ok);

        let mut bare = page("app.html", "RATA");
        bare.bridge = false;
        assert!(!judge_page(Some(&bare), None).ok);

        let mut broken = page("app.html", "RATA");
        broken.errors = vec!["ReferenceError: Can't find variable: Notification".into()];
        let c = judge_page(Some(&broken), None);
        assert!(!c.ok);
        assert!(
            c.detail["problems"][0]
                .as_str()
                .unwrap()
                .contains("Can't find variable")
        );

        // The notification plugin's own question, refused as on a computer:
        // expected, and still reported. Anything beside it still fails, and
        // so does the same words not at the start.
        let refused = "unhandled rejection: notification.is_permission_granted not allowed. \
                       Permissions associated with this command: notification:default";
        let mut plugin = page("app.html", "RATA");
        plugin.errors = vec![refused.into()];
        let c = judge_page(Some(&plugin), None);
        assert!(c.ok, "{c:?}");
        assert!(
            c.detail["expected"][0]
                .as_str()
                .unwrap()
                .starts_with(refused)
        );
        plugin
            .errors
            .push("TypeError: x is undefined (app.html:12)".into());
        assert!(!judge_page(Some(&plugin), None).ok);
        plugin.errors = vec![format!("could not load {refused}")];
        assert!(!judge_page(Some(&plugin), None).ok);

        // Asked for Settings: it must be app.html, with Settings showing.
        assert!(!judge_page(Some(&page("auth.html", "Sign in to RATA")), Some("set")).ok);
        let mut set = page("app.html", "RATA");
        assert!(!judge_page(Some(&set), Some("set")).ok);
        set.shown = Some(true);
        assert!(judge_page(Some(&set), Some("set")).ok);
    }

    #[test]
    fn a_report_is_kept_short_and_only_the_first_is_kept() {
        let smoke = Smoke::begin(tmpdir("heard"), None);
        let mut long = page("app.html", &"RATA ".repeat(100));
        long.errors = vec!["e".repeat(1000); 50];
        long.view = Some("set\" onload".into());
        smoke.heard(long);
        smoke.heard(page("auth.html", "second"));
        let kept = smoke.page.lock().unwrap().clone().unwrap();
        assert_eq!(kept.path, "app.html");
        assert_eq!(kept.title.chars().count(), 200);
        assert_eq!(kept.errors.len(), 20);
        assert!(kept.errors.iter().all(|e| e.chars().count() == 300));
        assert_eq!(kept.view, None);
    }

    #[test]
    fn network_answers_are_judged_on_what_they_name() {
        let google = vec![
            "gmail-smtp-in.l.google.com.".to_string(),
            "alt1.gmail-smtp-in.l.google.com.".to_string(),
        ];
        assert!(judge_mx(&google).ok);
        assert!(!judge_mx(&[]).ok);
        assert!(!judge_mx(&["mx.example.com.".to_string()]).ok);

        assert!(judge_srv(Some((IMAP_HOST.into(), 993))).ok);
        assert!(!judge_srv(Some((IMAP_HOST.into(), 143))).ok);
        assert!(!judge_srv(None).ok);

        // An SPF record naming Microsoft 365 among senders that are not
        // mailbox providers, and the autodiscover alias: both say Microsoft.
        let spf = vec![
            "google-site-verification=abc".to_string(),
            "v=spf1 include:spf.protection.outlook.com include:_spf.salesforce.com -all"
                .to_string(),
        ];
        let alias = Some("autodiscover.outlook.com".to_string());
        let c = judge_l1(&spf, alias.clone());
        assert!(c.ok, "{c:?}");
        assert_eq!(c.detail["spf names"][0], rata_mail::discover::MS_IMAP);
        assert_eq!(c.detail["minecraft.net txt records"], 2);
        // Either half missing, an SPF record naming no provider or another
        // one too, or an alias somewhere else, is not what L1 needs.
        assert!(!judge_l1(&spf, None).ok);
        assert!(!judge_l1(&[], alias.clone()).ok);
        let unknown = vec!["v=spf1 include:_spf-a.microsoft.com -all".to_string()];
        assert!(!judge_l1(&unknown, alias.clone()).ok);
        let two = vec![
            "v=spf1 include:spf.protection.outlook.com include:_spf.google.com ~all".to_string(),
        ];
        assert!(!judge_l1(&two, alias).ok);
        assert!(!judge_l1(&spf, Some("autodiscover.example.com".into())).ok);

        assert!(judge_greeting(Ok("* OK Gimap ready".into())).ok);
        assert!(!judge_greeting(Ok("* BYE".into())).ok);
        assert!(!judge_greeting(Err("timed out".into())).ok);

        let refused = rata_mail::Discovery::Refuse("no".into());
        assert!(!judge_discovery(refused).ok);
    }

    #[test]
    fn smoke_json_replaces_the_last_runs_whole() {
        let dir = tmpdir("finish");
        std::fs::write(dir.join(FILE), "an earlier run's").unwrap();
        let smoke = Smoke::begin(dir.clone(), Some("set".into()));
        assert!(!dir.join(FILE).exists(), "the last run's goes at once");

        smoke.record(Check::new("keychain", true, json!({})));
        let path = smoke.finish(Some(page("app.html", "RATA"))).unwrap();
        let doc: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(doc["mark"], MARK);
        assert_eq!(doc["ok"], true);
        assert_eq!(doc["page"]["path"], "app.html");
        assert!(!dir.join(format!("{FILE}.part")).exists());

        smoke.record(Check::new("tls", false, json!({})));
        smoke.finish(None).unwrap();
        let doc: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(doc["ok"], false);
        assert_eq!(doc["checks"].as_array().unwrap().len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
