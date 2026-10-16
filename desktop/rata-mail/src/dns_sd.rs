//! Asking the system's own resolver for DNS records, on iOS.
//!
//! An iOS app cannot read the network's DNS settings: its sandbox has no
//! `/etc/resolv.conf`, and the System Configuration store hickory reads on a
//! Mac is not offered there. Only the system's resolver (mDNSResponder) knows
//! the servers, and on an IPv6-only network (DNS64 and NAT64, which is how
//! Apple reviews apps) only it answers through the network's own. So on iOS
//! each SRV, MX and TXT question goes to it through dns_sd's
//! `DNSServiceQueryRecord`, which every Apple system has in libSystem; the few
//! functions and constants needed are declared here rather than taken from a
//! crate. Host addresses come from getaddrinfo instead
//! (`resolve::system_addresses`).
//!
//! Each question has its own connection to the system's resolver and is
//! waited for in a blocking task, so one that is slow or fails never holds up
//! another, nor the async runtime. The answers are each record's rdata, read
//! by `crate::records`.
//!
//! No CNAME question is ever asked. One about a name that has no CNAME
//! record holds up later questions that reach that name. Seen on macOS 15:
//! after one about a name that does not exist, getaddrinfo for that same
//! name waits 10 s and more instead of answering at once (as it does after
//! an MX or TXT question about it, in about 25 ms). Seen on macOS 26 and in
//! its iOS 26.5 Simulator: after one about the name at the end of an alias
//! chain (a name that exists, with no CNAME), a TXT question about the
//! chain's first alias was waited out (`SYSTEM_WAIT`). And what the system
//! caches it keeps for every app, so Outlook or Mail asking about the same
//! name later could meet it too. So where discovery wants to know whether a
//! name is an alias (`autodiscover.<domain>`, L1), it asks for the name's
//! IPv4 addresses instead, and takes the alias the system hands back on the
//! way to them ([`aliased`]): the system follows an alias whatever is asked,
//! and what that leaves in its cache is what any app's address question
//! about the name leaves there.
//!
//! Built for iOS, and on a Mac with the `system-dns` feature for the tests
//! that prove it (build.rs, `.github/workflows/mail-apple.yml`); everywhere
//! else hickory answers, as before.

use std::cell::Cell;
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::io::{Error, ErrorKind};
use std::ptr;
use std::time::{Duration, Instant};

use crate::records::question;
use crate::resolve::SYSTEM_WAIT;

/// The record types RATA asks for (RFC 1035, RFC 2782), and the alias
/// ([`CNAME`]) a question can be handed on the way to one of them.
pub(crate) const A: u16 = 1;
pub(crate) const CNAME: u16 = 5;
pub(crate) const MX: u16 = 15;
pub(crate) const TXT: u16 = 16;
pub(crate) const SRV: u16 = 33;

// From dns_sd.h.
const FLAGS_MORE_COMING: u32 = 0x1;
/// A record, or a "no such record", that is being added. Without it a reply
/// takes back something said before (a record that has gone, or a cached
/// "no such record" a fresher answer overturned), which is never an answer.
const FLAGS_ADD: u32 = 0x2;
/// Without it a name that does not exist is never answered at all, so every
/// such question would wait out the whole of `SYSTEM_WAIT`; with it the
/// resolver says `NO_SUCH_RECORD`. It also hands back each CNAME it follows
/// on the way to the record asked for: `reply` keeps only the name's own,
/// for [`aliased`], and never takes one for the answer.
const FLAGS_RETURN_INTERMEDIATES: u32 = 0x1000;
/// The answer came from the system's cache. A cached "no such record" is
/// not the last word: the resolver can ask the network anyway, and a real
/// answer then follows (Apple's own `dns-sd -q` on macOS showed the record
/// 116 ms after a cached "No Such Record"). So it is believed only once
/// `SETTLE` has passed with nothing fresher.
const FLAGS_ANSWERED_FROM_CACHE: u32 = 0x4000_0000;
const ANY_INTERFACE: u32 = 0;
const CLASS_IN: u16 = 1;
const NO_ERROR: i32 = 0;
const NO_SUCH_NAME: i32 = -65538;
const NO_SUCH_RECORD: i32 = -65554;

// From poll.h.
const POLLIN: i16 = 0x1;

#[repr(C)]
struct PollFd {
    fd: c_int,
    events: i16,
    revents: i16,
}

/// A `DNSServiceRef`: a pointer to a structure only the library knows.
type ServiceRef = *mut c_void;

/// `DNSServiceQueryRecordReply`.
type QueryReply = extern "C" fn(
    sd: ServiceRef,
    flags: u32,
    interface: u32,
    error: i32,
    fullname: *const c_char,
    rrtype: u16,
    rrclass: u16,
    rdlen: u16,
    rdata: *const c_void,
    ttl: u32,
    context: *mut c_void,
);

unsafe extern "C" {
    fn DNSServiceQueryRecord(
        sd: *mut ServiceRef,
        flags: u32,
        interface: u32,
        fullname: *const c_char,
        rrtype: u16,
        rrclass: u16,
        reply: QueryReply,
        context: *mut c_void,
    ) -> i32;
    fn DNSServiceRefSockFD(sd: ServiceRef) -> c_int;
    fn DNSServiceProcessResult(sd: ServiceRef) -> i32;
    fn DNSServiceRefDeallocate(sd: ServiceRef);
    fn poll(fds: *mut PollFd, nfds: u32, timeout: c_int) -> c_int;
}

/// The rdata of every `rrtype` record at `name`, as the system's resolver
/// hands it. Nothing when the name or the record does not exist, when the
/// resolver fails, and when no answer has come within `SYSTEM_WAIT`, which
/// is how hickory's failed lookups are treated on the desktop. Nothing, and
/// at once, for a name that is not to be asked at all
/// (`records::question`).
pub(crate) async fn ask(name: &str, rrtype: u16) -> Vec<Vec<u8>> {
    asked(name, rrtype).await.0
}

/// The rdata of the CNAME record at `name`, when it is an alias, as the
/// system hands it back on the way to the name's IPv4 addresses. Never a
/// CNAME question (see the module's own words). Nothing when it is no
/// alias, and in every case [`ask`] answers nothing.
pub(crate) async fn alias(name: &str) -> Option<Vec<u8>> {
    aliased(name).await.0
}

/// How a question ended. Only the tests read it, to tell an answer from a
/// question that was waited out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) enum Ended {
    /// The type asked for was answered, or said not to exist, with nothing
    /// more queued behind.
    Answered,
    /// The system's resolver failed, with dns_sd's error code.
    Failed(i32),
    /// The connection to the system's resolver closed or failed.
    Closed,
    /// No answer by the deadline.
    OutOfTime,
    /// The name cannot be asked at all, or is not to be (`.local` above
    /// all, which would go out by multicast DNS on the local network;
    /// `records::question`), so it was not.
    NotAsked,
}

/// [`ask`], and how the question ended.
pub(crate) async fn asked(name: &str, rrtype: u16) -> (Vec<Vec<u8>>, Ended) {
    let Some(name) = fit(name) else {
        return (vec![], Ended::NotAsked);
    };
    let found = asking(name, rrtype, SYSTEM_WAIT).await;
    (found.records, found.ended)
}

/// [`alias`], and how the question ended.
pub(crate) async fn aliased(name: &str) -> (Option<Vec<u8>>, Ended) {
    let Some(name) = fit(name) else {
        return (None, Ended::NotAsked);
    };
    let found = asking(name, A, SYSTEM_WAIT).await;
    (found.alias, found.ended)
}

/// `name` as the system's resolver is asked it, if it is to be asked.
fn fit(name: &str) -> Option<CString> {
    question(name).and_then(|n| CString::new(n).ok())
}

/// What one question found.
struct Found {
    /// The rdata of every record of the type asked for.
    records: Vec<Vec<u8>>,
    /// The rdata of the name's own CNAME record, when the system followed
    /// one on the way.
    alias: Option<Vec<u8>>,
    ended: Ended,
}

/// Ask `name`, already judged fit to ask, and wait at most `wait`.
async fn asking(name: CString, rrtype: u16, wait: Duration) -> Found {
    let by = Instant::now() + wait;
    let asking = tokio::task::spawn_blocking(move || ask_now(&name, rrtype, by));
    // `ask_now` stops at `by` by itself. This also covers a resolver that
    // stalls inside `DNSServiceProcessResult`: the caller goes on, and the
    // blocking task frees its question whenever that returns.
    let grace = tokio::time::Instant::from_std(by) + Duration::from_millis(250);
    match tokio::time::timeout_at(grace, asking).await {
        Ok(Ok(found)) => found,
        _ => Found {
            records: vec![],
            alias: None,
            ended: Ended::OutOfTime,
        },
    }
}

/// What the replies to one question have said so far. `reply` sees it
/// through the context pointer while the question is being answered, so
/// everything it changes is a `Cell`.
struct Heard {
    want: u16,
    /// The name asked, without its final dot, to know its own alias by.
    name: Vec<u8>,
    records: Cell<Vec<Vec<u8>>>,
    /// The name's own CNAME record's rdata, the first one handed back.
    alias: Cell<Option<Vec<u8>>>,
    /// The last reply said more were queued right behind it.
    more: Cell<bool>,
    /// The type asked for has been answered, or said not to exist.
    answered: Cell<bool>,
    /// The cache said there is no such record, which a fresh answer may
    /// still overturn (`FLAGS_ANSWERED_FROM_CACHE`).
    cached_none: Cell<bool>,
    /// The error the resolver reported, other than "no such record".
    failed: Cell<Option<i32>>,
}

/// How long a cached "no such record" is given to be overturned by the
/// network before it is believed.
const SETTLE: Duration = Duration::from_secs(1);

/// Called by `DNSServiceProcessResult`, once for each reply. It cannot
/// panic, which across this boundary would end the app.
extern "C" fn reply(
    _sd: ServiceRef,
    flags: u32,
    _interface: u32,
    error: i32,
    fullname: *const c_char,
    rrtype: u16,
    rrclass: u16,
    rdlen: u16,
    rdata: *const c_void,
    _ttl: u32,
    context: *mut c_void,
) {
    if context.is_null() {
        return;
    }
    // SAFETY: `context` is the `Heard` that `ask_now` started the question
    // with. It outlives the question, and replies come only from
    // `DNSServiceProcessResult`, on the thread that owns it.
    let heard = unsafe { &*context.cast_const().cast::<Heard>() };
    heard.more.set(flags & FLAGS_MORE_COMING != 0);
    // Only what is being added says anything: a reply without the flag
    // takes back something said before.
    let added = flags & FLAGS_ADD != 0;
    match error {
        NO_ERROR => {}
        // The name, or this record at it, does not exist: that is an answer,
        // and an empty one, when it is about the type asked for.
        NO_SUCH_RECORD | NO_SUCH_NAME => {
            if added && rrtype == heard.want {
                if flags & FLAGS_ANSWERED_FROM_CACHE == 0 {
                    heard.answered.set(true);
                } else {
                    heard.cached_none.set(true);
                }
            }
            return;
        }
        _ => {
            heard.failed.set(Some(error));
            return;
        }
    }
    if !added || rrclass != CLASS_IN {
        return;
    }
    let bytes: &[u8] = if rdlen == 0 {
        &[]
    } else if rdata.is_null() {
        return;
    } else {
        // SAFETY: dns_sd hands `rdlen` bytes at `rdata`, readable for the
        // length of this call; they are copied before it returns.
        unsafe { std::slice::from_raw_parts(rdata.cast::<u8>(), usize::from(rdlen)) }
    };
    if rrtype == heard.want {
        heard.answered.set(true);
        let mut records = heard.records.take();
        records.push(bytes.to_vec());
        heard.records.set(records);
    } else if rrtype == CNAME {
        // An alias followed on the way, never the answer. Only the name's
        // own is kept: every later one in the chain is another name's.
        // SAFETY: `fullname` is null or the reply's name, a C string
        // readable for the length of this call.
        let own = !fullname.is_null()
            && without_dot(unsafe { CStr::from_ptr(fullname) }.to_bytes())
                .eq_ignore_ascii_case(&heard.name);
        let first = heard.alias.take();
        heard
            .alias
            .set(first.or_else(|| own.then(|| bytes.to_vec())));
    }
}

/// A name without its final dot (or dots).
fn without_dot(mut name: &[u8]) -> &[u8] {
    while let Some(start) = name.strip_suffix(b".") {
        name = start;
    }
    name
}

/// Ask, and gather the answers until there are no more or `by` comes. This
/// blocks, so it runs on tokio's blocking threads.
fn ask_now(name: &CStr, rrtype: u16, by: Instant) -> Found {
    let heard = Heard {
        want: rrtype,
        name: without_dot(name.to_bytes()).to_vec(),
        records: Cell::new(vec![]),
        alias: Cell::new(None),
        more: Cell::new(false),
        answered: Cell::new(false),
        cached_none: Cell::new(false),
        failed: Cell::new(None),
    };
    let ended = ask_into(&heard, name, rrtype, by);
    Found {
        records: heard.records.take(),
        alias: heard.alias.take(),
        ended,
    }
}

/// Start the question with `heard` as its context, and wait for it.
fn ask_into(heard: &Heard, name: &CStr, rrtype: u16, by: Instant) -> Ended {
    let context = ptr::from_ref(heard).cast_mut().cast::<c_void>();
    let mut sd: ServiceRef = ptr::null_mut();
    // SAFETY: every pointer is valid for the call; `heard` outlives the
    // question, which `Question` frees before this returns.
    let started = unsafe {
        DNSServiceQueryRecord(
            &mut sd,
            FLAGS_RETURN_INTERMEDIATES,
            ANY_INTERFACE,
            name.as_ptr(),
            rrtype,
            CLASS_IN,
            reply,
            context,
        )
    };
    if started != NO_ERROR {
        return Ended::Failed(started);
    }
    if sd.is_null() {
        return Ended::Closed;
    }
    Question(sd).wait(heard, by)
}

/// One question in flight. Dropping it frees it, which also closes its
/// connection to the system's resolver, on every way out of `ask_into`.
struct Question(ServiceRef);

impl Drop for Question {
    fn drop(&mut self) {
        // SAFETY: a reference `DNSServiceQueryRecord` gave, freed once, here.
        unsafe { DNSServiceRefDeallocate(self.0) }
    }
}

impl Question {
    /// Read replies until the type asked for has been answered with nothing
    /// queued behind, the resolver fails, or `by` comes. A cached "no such
    /// record" ends it `SETTLE` later unless something fresher comes first.
    fn wait(&self, heard: &Heard, by: Instant) -> Ended {
        // SAFETY: the reference is live until `self` is dropped.
        let fd = unsafe { DNSServiceRefSockFD(self.0) };
        if fd < 0 {
            return Ended::Closed;
        }
        let mut settled: Option<Instant> = None;
        loop {
            let until = settled.map_or(by, |s| s.min(by));
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return match settled {
                    Some(_) => Ended::Answered,
                    None => Ended::OutOfTime,
                };
            }
            let ms = c_int::try_from(left.as_millis().max(1)).unwrap_or(c_int::MAX);
            let mut ready = PollFd {
                fd,
                events: POLLIN,
                revents: 0,
            };
            // SAFETY: one `pollfd`, valid for the call.
            let n = unsafe { poll(&mut ready, 1, ms) };
            if n < 0 && Error::last_os_error().kind() == ErrorKind::Interrupted {
                continue;
            }
            // Time is up: the next turn says how this ended.
            if n == 0 {
                continue;
            }
            // The connection failed, or closed with nothing left to read.
            if n < 0 || ready.revents & POLLIN == 0 {
                return Ended::Closed;
            }
            // SAFETY: the reference is live, and its socket has a reply.
            let processed = unsafe { DNSServiceProcessResult(self.0) };
            if processed != NO_ERROR {
                return Ended::Failed(processed);
            }
            if let Some(error) = heard.failed.get() {
                return Ended::Failed(error);
            }
            if heard.answered.get() && !heard.more.get() {
                return Ended::Answered;
            }
            if heard.cached_none.get() && settled.is_none() {
                settled = Some(Instant::now() + SETTLE);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;

    /// How much later than its deadline a question may end. Polling wakes a
    /// millisecond or so late; a busy runner can be slower still.
    const LATE: Duration = Duration::from_millis(500);

    /// A name no resolver will ever answer for: `.local` goes out by
    /// multicast DNS, which never says a name does not exist. Asked here
    /// past `records::question`, which keeps every real question away from
    /// it.
    fn unanswered() -> CString {
        CString::new(format!("rata-nobody-{}.local", std::process::id())).unwrap()
    }

    /// How many files this process has open, sockets among them.
    fn open_files() -> usize {
        std::fs::read_dir("/dev/fd").expect("/dev/fd").count()
    }

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    #[test]
    #[ignore = "the system's resolver: run on macOS and in an iOS Simulator by mail-apple.yml"]
    fn a_question_nobody_answers_ends_at_its_deadline() {
        // `ask_now` on its own thread, so that one that never stopped would
        // fail this test rather than hang it.
        for wait in [Duration::ZERO, Duration::from_millis(300)] {
            let (tx, rx) = mpsc::channel();
            let started = Instant::now();
            std::thread::spawn(move || {
                let _ = tx.send(ask_now(&unanswered(), TXT, started + wait));
            });
            let found = rx
                .recv_timeout(wait + LATE)
                .unwrap_or_else(|_| panic!("{wait:?}: still asking {LATE:?} after its deadline"));
            let took = started.elapsed();
            assert!(
                found.records.is_empty() && found.alias.is_none(),
                "{wait:?}: {} records",
                found.records.len()
            );
            assert_eq!(found.ended, Ended::OutOfTime, "{wait:?}, after {took:?}");
            assert!(took >= wait && took < wait + LATE, "{wait:?}: {took:?}");
        }
        // And as `asked` waits for it, on tokio's blocking threads.
        rt().block_on(async {
            let wait = Duration::from_millis(300);
            let started = Instant::now();
            let found = asking(unanswered(), MX, wait).await;
            let took = started.elapsed();
            assert_eq!(found.ended, Ended::OutOfTime, "after {took:?}");
            assert!(found.records.is_empty());
            assert!(took >= wait && took < wait + LATE, "{took:?}");
        });
    }

    #[test]
    #[ignore = "the system's resolver and real DNS: run on macOS and in an iOS Simulator by mail-apple.yml"]
    fn every_question_lets_go_of_its_connection() {
        // Each question has its own socket to the system's resolver, which
        // `Question`'s drop closes however the question ended: answered,
        // said not to exist, an alias, or waited out. Twenty of them leave
        // as many files open as there were before.
        rt().block_on(async {
            let pid = std::process::id();
            let before = open_files();
            for round in 0..4 {
                let (found, ended) = asked("gmail.com", MX).await;
                assert!(
                    !found.is_empty() && ended == Ended::Answered,
                    "{round}: MX {ended:?}"
                );
                let (found, ended) = asked(&format!("nx-{pid}-{round}.gmail.com"), TXT).await;
                assert!(
                    found.is_empty() && ended == Ended::Answered,
                    "{round}: nx {ended:?}"
                );
                let (alias, ended) = aliased("autodiscover.microsoft.com").await;
                assert!(
                    alias.is_some() && ended == Ended::Answered,
                    "{round}: alias {ended:?}"
                );
                let found = asking(unanswered(), TXT, Duration::from_millis(100)).await;
                assert_eq!(found.ended, Ended::OutOfTime, "{round}: unanswered");
                let found = asking(unanswered(), SRV, Duration::ZERO).await;
                assert_eq!(found.ended, Ended::OutOfTime, "{round}: no time at all");
            }
            assert_eq!(open_files(), before, "files open after twenty questions");
        });
    }
}
