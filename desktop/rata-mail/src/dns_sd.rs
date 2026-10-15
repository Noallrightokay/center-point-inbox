//! Asking the system's own resolver for DNS records, on iOS.
//!
//! An iOS app cannot read the network's DNS settings: its sandbox has no
//! `/etc/resolv.conf`, and the System Configuration store hickory reads on a
//! Mac is not offered there. Only the system's resolver (mDNSResponder) knows
//! the servers, and on an IPv6-only network (DNS64 and NAT64, which is how
//! Apple reviews apps) only it answers through the network's own. So on iOS
//! each SRV, MX, TXT and CNAME question goes to it through dns_sd's
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
//! Built for iOS, and on a Mac with the `system-dns` feature for the tests
//! that prove it (build.rs, `.github/workflows/mail-apple.yml`); everywhere
//! else hickory answers, as before.

use std::cell::Cell;
use std::ffi::{CString, c_char, c_int, c_void};
use std::io::{Error, ErrorKind};
use std::ptr;
use std::time::{Duration, Instant};

use crate::records::question;
use crate::resolve::SYSTEM_WAIT;

/// The record types RATA asks for (RFC 1035, RFC 2782).
pub(crate) const CNAME: u16 = 5;
pub(crate) const MX: u16 = 15;
pub(crate) const TXT: u16 = 16;
pub(crate) const SRV: u16 = 33;

// From dns_sd.h.
const FLAGS_MORE_COMING: u32 = 0x1;
const FLAGS_ADD: u32 = 0x2;
/// Without it a name that does not exist is never answered at all, so every
/// such question would wait out the whole of `SYSTEM_WAIT`; with it the
/// resolver says `NO_SUCH_RECORD`. It also hands back each CNAME it follows
/// on the way to the record asked for, which `reply` passes over.
const FLAGS_RETURN_INTERMEDIATES: u32 = 0x1000;
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
/// is how hickory's failed lookups are treated on the desktop.
pub(crate) async fn ask(name: &str, rrtype: u16) -> Vec<Vec<u8>> {
    asked(name, rrtype).await.0
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
    /// The name cannot be asked at all.
    NotAsked,
}

/// [`ask`], and how the question ended.
pub(crate) async fn asked(name: &str, rrtype: u16) -> (Vec<Vec<u8>>, Ended) {
    let Some(name) = question(name).and_then(|n| CString::new(n).ok()) else {
        return (vec![], Ended::NotAsked);
    };
    let by = Instant::now() + SYSTEM_WAIT;
    let asking = tokio::task::spawn_blocking(move || ask_now(&name, rrtype, by));
    // `ask_now` stops at `by` by itself. This also covers a resolver that
    // stalls inside `DNSServiceProcessResult`: the caller goes on, and the
    // blocking task frees its question whenever that returns.
    let grace = tokio::time::Instant::from_std(by) + Duration::from_millis(250);
    match tokio::time::timeout_at(grace, asking).await {
        Ok(Ok(found)) => found,
        _ => (vec![], Ended::OutOfTime),
    }
}

/// What the replies to one question have said so far. `reply` sees it
/// through the context pointer while the question is being answered, so
/// everything it changes is a `Cell`.
struct Heard {
    want: u16,
    records: Cell<Vec<Vec<u8>>>,
    /// The last reply said more were queued right behind it.
    more: Cell<bool>,
    /// The type asked for has been answered, or said not to exist.
    answered: Cell<bool>,
    /// The error the resolver reported, other than "no such record".
    failed: Cell<Option<i32>>,
}

/// Called by `DNSServiceProcessResult`, once for each reply. It cannot
/// panic, which across this boundary would end the app.
extern "C" fn reply(
    _sd: ServiceRef,
    flags: u32,
    _interface: u32,
    error: i32,
    _fullname: *const c_char,
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
    match error {
        NO_ERROR => {}
        // The name, or this record at it, does not exist: that is an answer,
        // and an empty one.
        NO_SUCH_RECORD | NO_SUCH_NAME => {
            heard.answered.set(true);
            return;
        }
        _ => {
            heard.failed.set(Some(error));
            return;
        }
    }
    // A CNAME followed on the way, or a record taken back: not an answer.
    if rrtype != heard.want || rrclass != CLASS_IN || flags & FLAGS_ADD == 0 {
        return;
    }
    heard.answered.set(true);
    let bytes: &[u8] = if rdlen == 0 {
        &[]
    } else if rdata.is_null() {
        return;
    } else {
        // SAFETY: dns_sd hands `rdlen` bytes at `rdata`, readable for the
        // length of this call; they are copied before it returns.
        unsafe { std::slice::from_raw_parts(rdata.cast::<u8>(), usize::from(rdlen)) }
    };
    let mut records = heard.records.take();
    records.push(bytes.to_vec());
    heard.records.set(records);
}

/// Ask, and gather the answers until there are no more or `by` comes. This
/// blocks, so it runs on tokio's blocking threads.
fn ask_now(name: &CString, rrtype: u16, by: Instant) -> (Vec<Vec<u8>>, Ended) {
    let heard = Heard {
        want: rrtype,
        records: Cell::new(vec![]),
        more: Cell::new(false),
        answered: Cell::new(false),
        failed: Cell::new(None),
    };
    let context = ptr::from_ref(&heard).cast_mut().cast::<c_void>();
    let mut sd: ServiceRef = ptr::null_mut();
    // SAFETY: every pointer is valid for the call; `heard` outlives the
    // question, which `Question` frees before `heard` goes.
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
        return (vec![], Ended::Failed(started));
    }
    if sd.is_null() {
        return (vec![], Ended::Closed);
    }
    let asking = Question(sd);
    let ended = asking.wait(&heard, by);
    drop(asking);
    (heard.records.take(), ended)
}

/// One question in flight. Dropping it frees it, which also closes its
/// connection to the system's resolver, on every way out of `ask_now`.
struct Question(ServiceRef);

impl Drop for Question {
    fn drop(&mut self) {
        // SAFETY: a reference `DNSServiceQueryRecord` gave, freed once, here.
        unsafe { DNSServiceRefDeallocate(self.0) }
    }
}

impl Question {
    /// Read replies until the type asked for has been answered with nothing
    /// queued behind, the resolver fails, or `by` comes.
    fn wait(&self, heard: &Heard, by: Instant) -> Ended {
        // SAFETY: the reference is live until `self` is dropped.
        let fd = unsafe { DNSServiceRefSockFD(self.0) };
        if fd < 0 {
            return Ended::Closed;
        }
        loop {
            let left = by.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ended::OutOfTime;
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
            if n == 0 {
                return Ended::OutOfTime;
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
        }
    }
}
