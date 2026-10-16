//! A self-test that a `smoke` build runs as it starts, for a device nobody
//! can watch: the iOS Simulator on GitHub's Macs (.github/workflows/ios.yml).
//!
//! It checks, on the device itself, each thing RATA leans on there and that
//! no test on a computer can stand in for: the keychain, the app's folder
//! and the list's lock, names looked up through the system's resolver (the
//! records discovery and L1 read), discovery of a real address, a TLS
//! connection to a real mail server through the guard, and the page itself.
//! Each answer goes into `smoke.json` in the app's data folder, written once
//! and whole when the last check is done, for the job to read.
//!
//! Never on by default, never built by release.yml, and
//! harness/verify-release.sh fails any published binary that holds the
//! words only this module puts in one. Nothing here signs in anywhere, sends
//! mail or keeps anything: the keychain entry is its own service's and is
//! deleted again, and the one address it discovers is never dialled.
//!
//! Two things are taken at launch, for the job's screenshots: a licence in
//! `RATA_SMOKE_LICENCE`, stored through the licence box's own path
//! (`Rata::set_licence`), and a view in `RATA_SMOKE_VIEW`, which the page
//! script opens as a person would: it fills the real sign-up form for an
//! RFC 2606 address on the first screen, closes the welcome, and presses the
//! view's own button.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tauri::State;
use tokio::sync::Notify;

use crate::core::Rata;
use crate::store::{self, Hold};
use crate::vault::{Keychain, SMOKE_SERVICE, Unreadable};

/// In every `smoke.json`, and so in every smoke build: one of the words
/// harness/verify-release.sh looks for in what is published.
pub const MARK: &str = "rata-smoke-self-test";
/// The file the job reads, in the app's data folder.
pub const FILE: &str = "smoke.json";
/// A licence to store at start, as the licence box would.
pub const LICENCE_VAR: &str = "RATA_SMOKE_LICENCE";
/// A view for the page script to open (`set` is Settings).
pub const VIEW_VAR: &str = "RATA_SMOKE_VIEW";

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

/// A view's name as the page names it (`inbox`, `set`): lowercase letters
/// only, since it goes into the page script and a selector.
fn view_named(raw: Option<String>) -> Option<String> {
    raw.filter(|v| (1..=16).contains(&v.len()) && v.bytes().all(|b| b.is_ascii_lowercase()))
}

// ------------------------------------------------------------- at start

/// The app's folder and the list's lock. `held` is whether this RATA got
/// the lock at start (`store::hold`); a second try must then find it taken,
/// which is what makes it a lock and not a file.
pub fn store(list: &Path, held: bool) -> Check {
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
    let mode = {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(folder)
            .map(|m| format!("{:o}", m.permissions().mode() & 0o777))
            .unwrap_or_default()
    };
    #[cfg(not(unix))]
    let mode = String::new();
    Check::new(
        "store",
        held && is_folder && writable && again == "taken",
        json!({
            "folder": folder.display().to_string(),
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

/// The rest, once the window is up: the keychain, the network, then the
/// page's report (waited for), and `smoke.json`.
pub fn start(smoke: Arc<Smoke>) {
    tauri::async_runtime::spawn(async move {
        let secret = fresh_secret();
        let kept = tauri::async_runtime::spawn_blocking(move || keychain(&InKeychain, &secret))
            .await
            .unwrap_or_else(|e| Check::new("keychain", false, json!({ "error": e.to_string() })));
        smoke.record(kept);
        network(&smoke).await;
        let page = smoke.wait_for_page(PAGE_WAIT).await;
        smoke.record(judge_page(page.as_ref(), smoke.view.as_deref()));
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
    fn put(&self, secret: &str) -> Result<(), String>;
    fn get(&self) -> Result<String, Unreadable>;
    fn forget(&self) -> Result<(), String>;
}

struct InKeychain;

impl Slot for InKeychain {
    fn put(&self, secret: &str) -> Result<(), String> {
        Keychain::put_smoke(ENTRY, secret)
    }
    fn get(&self) -> Result<String, Unreadable> {
        Keychain::get_smoke(ENTRY)
    }
    fn forget(&self) -> Result<(), String> {
        Keychain::forget_smoke(ENTRY)
    }
}

/// Written, read back, deleted, and then really gone. An entry an earlier
/// run left (one stopped halfway) is cleared first. The secret is never in
/// what is reported.
fn keychain(slot: &dyn Slot, secret: &str) -> Check {
    let run = || -> Result<(), String> {
        slot.forget()
            .map_err(|e| format!("an earlier run's entry could not be cleared: {e}"))?;
        slot.put(secret)
            .map_err(|e| format!("it could not be written: {e}"))?;
        match slot.get() {
            Ok(s) if s == secret => {}
            Ok(_) => return Err("it read back as something else".into()),
            Err(e) => return Err(format!("it could not be read back: {e}")),
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
    match run() {
        Ok(()) => Check::new(
            "keychain",
            true,
            json!({ "service": SMOKE_SERVICE, "written, read back and deleted": true }),
        ),
        Err(e) => Check::new(
            "keychain",
            false,
            json!({ "service": SMOKE_SERVICE, "error": e }),
        ),
    }
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
    let txt = r.txt("microsoft.com").await;
    let alias = r.cname("autodiscover.microsoft.com").await;
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

/// What L1 reads of a domain behind a mail filter, asked of microsoft.com:
/// its SPF record (a TXT record, read strictly) and its autodiscover alias,
/// which names Microsoft 365.
fn judge_l1(txt: &[String], alias: Option<String>) -> Check {
    use rata_mail::discover::{autodiscover_provider, parse_spf, spf_record};
    let spf = spf_record(txt).and_then(parse_spf);
    let microsoft = alias.as_deref().and_then(autodiscover_provider).is_some();
    Check::new(
        "l1 records",
        spf.as_ref().is_some_and(|s| !s.includes.is_empty()) && microsoft,
        json!({
            "microsoft.com txt records": txt.len(),
            "spf includes": spf.map(|s| s.includes),
            "autodiscover.microsoft.com": alias,
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

/// The page loaded, is RATA's, has no script errors, has the bridge where
/// it should (app.html), and showed the view asked for.
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
    if !p.errors.is_empty() {
        wrong.push(format!("{} script error(s)", p.errors.len()));
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
        json!({ "path": p.path, "problems": wrong }),
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
    }

    /// An entry in a map, which can be made to misbehave.
    #[derive(Default)]
    struct Fake {
        held: Mutex<Option<String>>,
        reads_wrong: bool,
        keeps_on_forget: bool,
    }

    impl Slot for Fake {
        fn put(&self, secret: &str) -> Result<(), String> {
            *self.held.lock().unwrap() = Some(secret.into());
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
    }

    #[test]
    fn a_keychain_entry_is_written_read_back_and_deleted() {
        let secret = fresh_secret();
        assert_eq!(secret.len(), 32);
        let good = Fake::default();
        // Left by an earlier run that stopped halfway: cleared first.
        good.put("left behind").unwrap();
        let c = keychain(&good, &secret);
        assert!(c.ok, "{c:?}");
        assert!(good.held.lock().unwrap().is_none());

        let wrong = Fake {
            reads_wrong: true,
            ..Fake::default()
        };
        let c = keychain(&wrong, &secret);
        assert!(
            !c.ok && c.detail.to_string().contains("something else"),
            "{c:?}"
        );

        let sticky = Fake {
            keeps_on_forget: true,
            ..Fake::default()
        };
        let c = keychain(&sticky, &secret);
        assert!(
            !c.ok && c.detail.to_string().contains("still there"),
            "{c:?}"
        );
        // The secret is never what is reported.
        for c in [keychain(&wrong, &secret), keychain(&sticky, &secret)] {
            assert!(!c.detail.to_string().contains(&secret), "{c:?}");
        }
    }

    #[test]
    fn the_store_check_needs_this_rata_to_hold_the_lists_lock() {
        let dir = tmpdir("store");
        let list = dir.join("org.mailrata.desktop/mailboxes.json");
        let Hold::Mine(lock) = store::hold(&list, Duration::ZERO) else {
            panic!("the lock should be free");
        };
        let c = store(&list, true);
        assert!(c.ok, "{c:?}");
        assert_eq!(c.detail["lock tried again"], "taken");
        assert_eq!(c.detail["writable"], true);
        assert!(!list.parent().unwrap().join(".smoke-probe").exists());
        drop(lock);
        // Let go of: a second try gets it, which is no lock at all.
        let c = store(&list, true);
        assert!(!c.ok && c.detail["lock tried again"] == "free", "{c:?}");
        // Not held at start (another copy had it, or locks are refused).
        assert!(!store(&list, false).ok);
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
        assert!(!judge_page(Some(&broken), None).ok);

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

        let spf = vec![
            "google-site-verification=abc".to_string(),
            "v=spf1 include:_spf-a.microsoft.com include:spf-a.hotmail.com -all".to_string(),
        ];
        let alias = Some("autodiscover.outlook.com".to_string());
        assert!(judge_l1(&spf, alias.clone()).ok);
        assert!(!judge_l1(&spf, None).ok);
        assert!(!judge_l1(&[], alias).ok);

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
