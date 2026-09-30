//! The engine against real mail servers.
//!
//! Every other test in this crate talks to a scripted server: one that says
//! what the test's author expected a server to say. These talk to Dovecot (IMAP)
//! and GreenMail (SMTP) running on this machine, started by
//! `tests/loopback/servers.sh`, through the same public functions the app
//! calls — `verify`, `fetch_newest`, `fetch_older`, `act`, `send`, `watch` —
//! with TLS verified against a CA made for the run.
//!
//! Built only with `--features loopback-tests`, which lets the outbound guard
//! accept the literal `127.0.0.1` in a debug build and is a compile error in a
//! release one (see `guard.rs`). Run:
//!
//! ```text
//! sudo tests/loopback/servers.sh start /tmp/rata-lb
//! SSL_CERT_FILE=/tmp/rata-lb/ca.crt cargo test --features loopback-tests --test loopback
//! ```
//!
//! Each test signs in as an address of its own, made unique per run, so they
//! run in parallel and a second run against the same servers starts clean.
//! The tests' own seeding connection uses Dovecot's plain port 1143 and never
//! goes through the engine: it is the "other device" that files mail, changes
//! flags and rebuilds folders behind RATA's back.

use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_imap::types::Flag;
use futures::StreamExt;
use mail_parser::MimeHeaders;
use rata_mail::search_folder;
use rata_mail::{
    Account, Acted, Action, Address, Credential, DraftSaved, Fetched, File, Folder, HostVerdict,
    Known, Listed, Outgoing, Prior, Resolver, Sent, Source, Verify, Watched, act, check_literal,
    check_resolved, fetch_newest, fetch_older, fetch_uids, fetch_whole, imap::Whole, list_folders,
    save_draft, send, verify, watch,
};
use tokio::net::TcpStream;

/// Every test account's password. The servers accept any user name with it.
const PASS: &str = "rata-loopback";
const HOST: &str = "127.0.0.1";
/// Dovecot's plain port, for the tests' own connection only.
const SEED_PORT: u16 = 1143;
/// GreenMail's plain IMAP, to read back what the SMTP sink received.
const SINK_PORT: u16 = 3143;

type Seed = async_imap::Session<TcpStream>;

// ------------------------------------------------------------------ helpers

/// An address no other test, and no earlier run, has used.
fn user(tag: &str) -> String {
    static N: AtomicU32 = AtomicU32::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let n = N.fetch_add(1, Ordering::Relaxed);
    format!("{tag}-{nanos}-{n}@rata.test")
}

fn account(email: &str) -> Account {
    Account {
        email: email.into(),
        credential: Credential::Password(PASS.into()),
        host: HOST.into(),
        port: 993,
        label: "Loopback".into(),
    }
}

fn resolver() -> Resolver {
    // Without the test CA the engine would reject the servers' certificate
    // with a message that looks like a real bug. Say what is missing instead.
    assert!(
        std::env::var_os("SSL_CERT_FILE").is_some(),
        "SSL_CERT_FILE must point at the CA servers.sh wrote (<dir>/ca.crt)"
    );
    Resolver::system().expect("resolver")
}

/// A signed-in plain IMAP session to one of the local servers.
async fn session(port: u16, email: &str) -> Seed {
    let tcp = TcpStream::connect((HOST, port))
        .await
        .unwrap_or_else(|e| panic!("nothing on {HOST}:{port} ({e}) — is servers.sh running?"));
    let mut client = async_imap::Client::new(tcp);
    client.read_response().await.unwrap().expect("greeting");
    client
        .login(email, PASS)
        .await
        .map_err(|(e, _)| e)
        .expect("seed login")
}

async fn seed(email: &str) -> Seed {
    session(SEED_PORT, email).await
}

/// A plain message, with an INTERNALDATE `minute` minutes into 2026 so the
/// newest-first order is known.
fn message(from: &str, to: &str, subject: &str, extra: &str) -> String {
    format!(
        "From: Ann Example <{from}>\r\nTo: <{to}>\r\n{extra}Subject: {subject}\r\nMessage-ID: <{}@rata.test>\r\nDate: Thu, 1 Jan 2026 10:00:00 +0000\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nThe text of {subject}.\r\n",
        subject.replace(' ', "-")
    )
}

fn date(minute: u32) -> String {
    format!(
        "\"01-Jan-2026 {:02}:{:02}:00 +0000\"",
        10 + minute / 60,
        minute % 60
    )
}

/// Put `count` messages in `folder`, subjects "<folder> 0", "<folder> 1"…,
/// oldest first.
async fn put(s: &mut Seed, owner: &str, folder: &str, count: u32, flags: Option<&str>) {
    for i in 0..count {
        let raw = message("ann@rata.test", owner, &format!("{folder} {i}"), "");
        s.append(folder, flags, Some(&date(i)), raw).await.unwrap();
    }
}

/// UIDVALIDITY and every UID of a folder, as the server has them now.
async fn uids(s: &mut Seed, folder: &str) -> (u32, Vec<u32>) {
    let mb = s.select(folder).await.unwrap();
    let mut out = vec![];
    if mb.exists > 0 {
        let got: Vec<_> = s.fetch("1:*", "UID").await.unwrap().collect().await;
        out = got.into_iter().filter_map(|f| f.ok()?.uid).collect();
    }
    out.sort_unstable();
    (mb.uid_validity.unwrap(), out)
}

/// The flags of one message.
async fn flags(s: &mut Seed, folder: &str, uid: u32) -> Vec<String> {
    s.select(folder).await.unwrap();
    let got: Vec<_> = s
        .uid_fetch(uid.to_string(), "FLAGS")
        .await
        .unwrap()
        .collect()
        .await;
    let f = got.into_iter().next().expect("message").unwrap();
    f.flags()
        .map(|f| match f {
            Flag::Seen => "\\Seen".to_string(),
            Flag::Flagged => "\\Flagged".to_string(),
            other => format!("{other:?}"),
        })
        .collect()
}

fn in_folder(msgs: &[rata_mail::Message], folder: &Folder) -> Vec<String> {
    let mut s: Vec<String> = msgs
        .iter()
        .filter(|m| m.folder == *folder)
        .map(|m| m.subject.clone())
        .collect();
    s.sort();
    s
}

fn messages(f: Fetched) -> Vec<rata_mail::Message> {
    match f {
        Fetched::Messages(m) => m,
        other => panic!("expected messages, got {other:?}"),
    }
}

// -------------------------------------------------------------------- guard

/// The gate itself, in the build that has it open: exactly one address, typed
/// exactly one way, and never a name that resolves there.
#[test]
fn guard_admits_only_the_literal_loopback_address() {
    assert_eq!(check_literal("127.0.0.1"), Some(HostVerdict::Allowed));
    for still_refused in [
        "127.0.0.2",
        "127.1.2.3",
        "::1",
        "[::1]",
        "::ffff:127.0.0.1",
        "::ffff:7f00:1",
        "localhost",
        "a.localhost",
        "10.0.0.1",
        "192.168.1.1",
        "169.254.169.254",
        "0.0.0.0",
    ] {
        assert_eq!(
            check_literal(still_refused),
            Some(HostVerdict::NotPublic),
            "{still_refused} must stay refused with loopback-tests on"
        );
    }
    // A name answering with 127.0.0.1 is still a name pointing inside.
    assert_eq!(
        check_resolved(&["127.0.0.1".parse().unwrap()]),
        HostVerdict::NotPublic
    );
}

// --------------------------------------------------------------------- link

#[tokio::test]
async fn link_by_explicit_host() {
    let r = resolver();
    let me = user("link");
    match verify(&r, &me, PASS, Some(HOST)).await {
        Verify::Ok(v) => {
            assert_eq!(v.host, HOST);
            assert_eq!(v.port, 993);
            assert_eq!(v.source, Source::Override);
        }
        other => panic!("linking by server address failed: {other:?}"),
    }
    // A pasted "imaps://host:993/" is tidied to the same thing.
    assert!(matches!(
        verify(&r, &me, PASS, Some("imaps://127.0.0.1:993/")).await,
        Verify::Ok(_)
    ));
    // And a wrong password is a refusal, not "could not reach".
    match verify(&r, &me, "not-the-password", Some(HOST)).await {
        Verify::Refused(why) => assert!(why.contains("rejected the sign-in"), "{why}"),
        other => panic!("a wrong password was not read as one: {other:?}"),
    }
}

/// Set in the child process [`a_typed_server_address_keeps_the_reason_it_failed`]
/// starts.
const UNTRUSTING: &str = "RATA_LOOPBACK_UNTRUSTING";

/// Linking by a typed server address whose certificate this machine does not
/// trust says so, as a refresh does, rather than "Could not reach".
///
/// The engine reads the trust store once per process, and this one trusts the
/// test CA. So the test runs itself again in a process whose `SSL_CERT_FILE`
/// is the system's own bundle, which does not have it, and does the check
/// there.
#[test]
fn a_typed_server_address_keeps_the_reason_it_failed() {
    if std::env::var_os(UNTRUSTING).is_none() {
        let bundle = "/etc/ssl/certs/ca-certificates.crt";
        assert!(
            std::path::Path::new(bundle).exists(),
            "{bundle} is needed as a trust store without the test CA"
        );
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "a_typed_server_address_keeps_the_reason_it_failed",
                "--exact",
                "--nocapture",
            ])
            .env(UNTRUSTING, "1")
            .env("SSL_CERT_FILE", bundle)
            .output()
            .expect("the test binary runs");
        let said = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.status.success(), "{said}");
        assert!(said.contains("1 passed"), "the check did not run: {said}");
        return;
    }
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        match verify(&resolver(), &user("untrusted"), PASS, Some(HOST)).await {
            Verify::Failed(why) => {
                assert!(why.contains("could not be trusted"), "{why}");
                assert!(why.contains("UnknownIssuer"), "{why}");
                assert!(why.contains(HOST), "{why}");
            }
            other => panic!("expected a refusal that says why: {other:?}"),
        }
    });
}

// --------------------------------------------------------------- first sync

#[tokio::test]
async fn first_sync_reads_inbox_sent_drafts_archive_and_junk() {
    let r = resolver();
    let me = user("first");
    let mut s = seed(&me).await;
    put(&mut s, &me, "INBOX", 3, None).await;
    put(&mut s, &me, "Sent", 2, Some("(\\Seen)")).await;
    put(&mut s, &me, "Archive", 1, Some("(\\Seen)")).await;
    put(&mut s, &me, "Junk", 1, None).await;
    let draft = message(
        &me,
        "bob@rata.test",
        "Drafts 0",
        "Cc: <carol@rata.test>\r\nBcc: <dave@rata.test>\r\n",
    );
    s.append("Drafts", Some("(\\Seen \\Draft)"), Some(&date(0)), draft)
        .await
        .unwrap();
    let (_, draft_uids) = uids(&mut s, "Drafts").await;
    s.logout().await.unwrap();

    let got = fetch_newest(&r, &account(&me), 50, &[])
        .await
        .expect("first sync");
    let m = &got.messages;
    assert_eq!(
        in_folder(m, &Folder::Inbox),
        ["INBOX 0", "INBOX 1", "INBOX 2"]
    );
    assert_eq!(in_folder(m, &Folder::Sent), ["Sent 0", "Sent 1"]);
    assert_eq!(in_folder(m, &Folder::Archive), ["Archive 0"]);
    assert_eq!(in_folder(m, &Folder::Junk), ["Junk 0"]);
    assert_eq!(in_folder(m, &Folder::Drafts), ["Drafts 0"]);
    assert_eq!(m.len(), 8, "nothing from Trash or anywhere else: {m:#?}");

    // Newest first, by INTERNALDATE.
    let inbox: Vec<_> = m.iter().filter(|m| m.folder == Folder::Inbox).collect();
    assert_eq!(inbox[0].subject, "INBOX 2");
    assert!(
        inbox
            .iter()
            .all(|m| m.unread && m.uid > 0 && m.uidvalidity > 0)
    );
    assert_eq!(inbox[0].from_addr, "ann@rata.test");
    assert_eq!(inbox[0].body.trim(), "The text of INBOX 2.");

    // Ids carry the folder, so a Sent UID can never be the inbox's.
    let sent = m.iter().find(|m| m.folder == Folder::Sent).unwrap();
    assert!(sent.id.contains("_sent_"), "{}", sent.id);
    assert!(!sent.unread);

    // A draft keeps To, Cc and its Bcc, and every draft is listed by id.
    let d = m.iter().find(|m| m.folder == Folder::Drafts).unwrap();
    assert_eq!(d.to_all, ["bob@rata.test"]);
    assert_eq!(d.cc, ["carol@rata.test"]);
    assert_eq!(d.bcc, ["dave@rata.test"]);
    assert_eq!(got.drafts.as_deref(), Some(&[d.id.clone()][..]));
    assert_eq!(d.uid, draft_uids[0]);
    assert!(got.gaps.is_empty() && got.flags.is_empty());
}

// ------------------------------------------------------ incremental refresh

#[tokio::test]
async fn refresh_downloads_only_new_mail_and_brings_flag_changes() {
    let r = resolver();
    let me = user("refresh");
    let acct = account(&me);
    let mut s = seed(&me).await;
    put(&mut s, &me, "INBOX", 3, None).await;

    let first = fetch_newest(&r, &acct, 50, &[]).await.unwrap();
    let inbox: Vec<_> = first
        .messages
        .iter()
        .filter(|m| m.folder == Folder::Inbox)
        .collect();
    assert_eq!(inbox.len(), 3);
    let since = inbox.iter().map(|m| m.uid).max().unwrap();
    let known = [Known {
        folder: Folder::Inbox,
        uidvalidity: inbox[0].uidvalidity,
        since,
    }];

    // Nothing new: nothing downloaded, and the flags of what RATA holds.
    let quiet = fetch_newest(&r, &acct, 50, &known).await.unwrap();
    assert!(
        quiet.messages.iter().all(|m| m.folder != Folder::Inbox),
        "{:#?}",
        quiet.messages
    );
    assert_eq!(quiet.flags.len(), 3);
    assert!(quiet.flags.iter().all(|f| f.unread && !f.starred));

    // Another device reads one and stars another; two more arrive.
    let (_, held) = uids(&mut s, "INBOX").await;
    s.uid_store(held[0].to_string(), "+FLAGS (\\Seen)")
        .await
        .unwrap()
        .collect::<Vec<_>>()
        .await;
    s.uid_store(held[1].to_string(), "+FLAGS (\\Flagged)")
        .await
        .unwrap()
        .collect::<Vec<_>>()
        .await;
    for i in 3..5 {
        let raw = message("ann@rata.test", &me, &format!("INBOX {i}"), "");
        s.append("INBOX", None, Some(&date(i)), raw).await.unwrap();
    }
    s.logout().await.unwrap();

    let next = fetch_newest(&r, &acct, 50, &known).await.unwrap();
    let fresh = in_folder(&next.messages, &Folder::Inbox);
    assert_eq!(fresh, ["INBOX 3", "INBOX 4"], "only the new ones");
    assert!(next.messages.iter().all(|m| m.uid > since));
    let by_id = |uid: u32| {
        next.flags
            .iter()
            .find(|f| f.id.ends_with(&format!("_{uid}")))
            .unwrap_or_else(|| panic!("no flags for {uid}: {:?}", next.flags))
    };
    assert!(!by_id(held[0]).unread, "read elsewhere");
    assert!(by_id(held[1]).starred, "starred elsewhere");
    assert!(by_id(held[2]).unread && !by_id(held[2]).starred);
    assert!(next.gaps.is_empty());
}

/// Mail deleted or moved on another device leaves RATA (BUG-C): the next
/// refresh lists what each folder still holds, and a message expunged by a
/// second connection is not in it.
#[tokio::test]
async fn a_message_expunged_elsewhere_is_missing_from_the_next_listing() {
    let r = resolver();
    let me = user("present");
    let acct = account(&me);
    let mut s = seed(&me).await;
    put(&mut s, &me, "INBOX", 4, None).await;
    put(&mut s, &me, "Sent", 2, Some("(\\Seen)")).await;
    s.logout().await.unwrap();

    let first = fetch_newest(&r, &acct, 50, &[]).await.unwrap();
    let listed = |n: &rata_mail::Newest, f: &Folder| {
        n.present
            .iter()
            .find(|p| p.folder == *f)
            .cloned()
            .unwrap_or_else(|| panic!("{f:?} not listed: {:?}", n.present))
    };
    let (validity, held) = {
        let mut s = seed(&me).await;
        let got = uids(&mut s, "INBOX").await;
        s.logout().await.unwrap();
        got
    };
    let p = listed(&first, &Folder::Inbox);
    assert_eq!((p.uidvalidity, p.floor), (validity, 1));
    assert_eq!(p.uids, held);
    assert!(p.next > *held.last().unwrap());
    assert_eq!(listed(&first, &Folder::Sent).uids.len(), 2);
    let known = [Known {
        folder: Folder::Inbox,
        uidvalidity: validity,
        since: *held.last().unwrap(),
    }];

    // The phone deletes the second message for good, and a Sent one.
    let mut s = seed(&me).await;
    s.select("INBOX").await.unwrap();
    s.uid_store(held[1].to_string(), "+FLAGS.SILENT (\\Deleted)")
        .await
        .unwrap()
        .collect::<Vec<_>>()
        .await;
    s.expunge().await.unwrap().collect::<Vec<_>>().await;
    let (_, sent) = uids(&mut s, "Sent").await;
    s.uid_store(sent[0].to_string(), "+FLAGS.SILENT (\\Deleted)")
        .await
        .unwrap()
        .collect::<Vec<_>>()
        .await;
    s.expunge().await.unwrap().collect::<Vec<_>>().await;
    s.logout().await.unwrap();

    let next = fetch_newest(&r, &acct, 50, &known).await.unwrap();
    let p = listed(&next, &Folder::Inbox);
    assert_eq!(p.uidvalidity, validity);
    assert!(p.floor <= held[0]);
    assert!(!p.uids.contains(&held[1]), "{p:?}");
    assert_eq!(p.uids, vec![held[0], held[2], held[3]]);
    assert!(
        next.messages.iter().all(|m| m.folder != Folder::Inbox),
        "nothing new was downloaded"
    );
    assert_eq!(listed(&next, &Folder::Sent).uids, vec![sent[1]]);
}

// ---------------------------------------------------------------- older mail

#[tokio::test]
async fn older_mail_pages_back_by_position() {
    let r = resolver();
    let me = user("older");
    let acct = account(&me);
    let mut s = seed(&me).await;
    put(&mut s, &me, "INBOX", 7, None).await;
    s.logout().await.unwrap();

    let first = fetch_newest(&r, &acct, 3, &[]).await.unwrap();
    assert_eq!(
        in_folder(&first.messages, &Folder::Inbox),
        ["INBOX 4", "INBOX 5", "INBOX 6"]
    );
    let validity = first.messages[0].uidvalidity;
    let mut oldest = first.messages.iter().map(|m| m.uid).min().unwrap();

    let mut pages = vec![];
    loop {
        let page = messages(fetch_older(&r, &acct, Folder::Inbox, oldest, validity, 3).await);
        if page.is_empty() {
            break;
        }
        // Newest first within a page, and all below what was held.
        assert!(page.windows(2).all(|w| w[0].ts >= w[1].ts));
        assert!(page.iter().all(|m| m.uid < oldest));
        oldest = page.iter().map(|m| m.uid).min().unwrap();
        pages.push(in_folder(&page, &Folder::Inbox));
    }
    assert_eq!(
        pages,
        [vec!["INBOX 1", "INBOX 2", "INBOX 3"], vec!["INBOX 0"]]
    );
}

// ------------------------------------------------------------ server search

#[tokio::test]
async fn a_server_search_finds_a_message_by_a_word_in_its_body() {
    let r = resolver();
    let me = user("search");
    let acct = account(&me);
    let mut s = seed(&me).await;
    put(&mut s, &me, "INBOX", 4, None).await;
    // The words searched for are only in the text, which is quoted-printable,
    // as most mail programs send it.
    let raw = format!(
        "From: Bo Li <bo@rata.test>\r\nTo: <{me}>\r\nSubject: Lunch\r\nMessage-ID: <lunch@rata.test>\r\nDate: Thu, 1 Jan 2026 11:00:00 +0000\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\nThe zanzibar figures are in, and so is the caf=C3=A9 bill.\r\n"
    );
    s.append("INBOX", None, Some(&date(30)), raw).await.unwrap();
    s.logout().await.unwrap();

    let got = search_folder(&r, &acct, Folder::Inbox, "Zanzibar", 50)
        .await
        .unwrap();
    assert_eq!(got.matched, 1, "{got:?}");
    assert_eq!(in_folder(&got.messages, &Folder::Inbox), ["Lunch"]);
    assert!(got.messages[0].body.contains("zanzibar figures"));
    assert!(got.messages[0].uid > 0 && got.messages[0].uidvalidity > 0);

    // A letter outside ASCII goes with CHARSET UTF-8, and Dovecot finds it
    // in the decoded text.
    let got = search_folder(&r, &acct, Folder::Inbox, "café", 50)
        .await
        .unwrap();
    assert_eq!(in_folder(&got.messages, &Folder::Inbox), ["Lunch"]);

    // By sender, and every message by a word they all share.
    let got = search_folder(&r, &acct, Folder::Inbox, "bo@rata.test", 50)
        .await
        .unwrap();
    assert_eq!(in_folder(&got.messages, &Folder::Inbox), ["Lunch"]);
    let got = search_folder(&r, &acct, Folder::Inbox, "INBOX", 2)
        .await
        .unwrap();
    assert_eq!(got.matched, 4, "every seeded subject: {got:?}");
    assert_eq!(got.messages.len(), 2, "only the newest are fetched");

    // A query that would widen the search were it inside a quoted string is
    // looked for as it stands, and is in no message.
    let got = search_folder(&r, &acct, Folder::Inbox, "x\" OR ALL \"", 50)
        .await
        .unwrap();
    assert_eq!(got.matched, 0, "{got:?}");
}

// ---------------------------------------------------------------------- act

#[tokio::test]
async fn read_star_trash_archive_and_move_change_the_real_mailbox() {
    let r = resolver();
    let me = user("act");
    let acct = account(&me);
    let mut s = seed(&me).await;
    s.create("Projects").await.unwrap();
    put(&mut s, &me, "INBOX", 6, None).await;
    let (validity, u) = uids(&mut s, "INBOX").await;

    // Only the customer's own folder is offered for Move to.
    match list_folders(&r, &acct).await {
        Listed::Folders(f) => {
            let names: Vec<_> = f.iter().map(|f| f.name.as_str()).collect();
            assert_eq!(names, ["Projects"]);
        }
        other => panic!("{other:?}"),
    }

    let done = |a: Acted, uid: u32| match a {
        Acted::Done { done, gone } => {
            assert_eq!(done, [uid]);
            assert!(gone.is_empty());
        }
        other => panic!("{other:?}"),
    };
    done(
        act(&r, &acct, Folder::Inbox, &[u[0]], validity, Action::Read).await,
        u[0],
    );
    done(
        act(&r, &acct, Folder::Inbox, &[u[1]], validity, Action::Star).await,
        u[1],
    );
    assert!(
        flags(&mut s, "INBOX", u[0])
            .await
            .contains(&"\\Seen".into())
    );
    assert!(
        flags(&mut s, "INBOX", u[1])
            .await
            .contains(&"\\Flagged".into())
    );
    done(
        act(&r, &acct, Folder::Inbox, &[u[0]], validity, Action::Unread).await,
        u[0],
    );
    assert!(
        !flags(&mut s, "INBOX", u[0])
            .await
            .contains(&"\\Seen".into())
    );

    done(
        act(&r, &acct, Folder::Inbox, &[u[2]], validity, Action::Trash).await,
        u[2],
    );
    done(
        act(&r, &acct, Folder::Inbox, &[u[3]], validity, Action::Archive).await,
        u[3],
    );
    done(
        act(
            &r,
            &acct,
            Folder::Inbox,
            &[u[4]],
            validity,
            Action::Move(Folder::Named("Projects".into())),
        )
        .await,
        u[4],
    );

    // Where each one is now, as the server says.
    let (_, inbox) = uids(&mut s, "INBOX").await;
    assert_eq!(inbox, [u[0], u[1], u[5]]);
    assert_eq!(uids(&mut s, "Trash").await.1.len(), 1);
    assert_eq!(uids(&mut s, "Projects").await.1.len(), 1);
    let (archive_validity, archived) = uids(&mut s, "Archive").await;
    assert_eq!(archived.len(), 1);

    // Moved back from Archive to the inbox, where it gets a new UID.
    done(
        act(
            &r,
            &acct,
            Folder::Archive,
            &archived,
            archive_validity,
            Action::Inbox,
        )
        .await,
        archived[0],
    );
    let (_, inbox) = uids(&mut s, "INBOX").await;
    assert_eq!(inbox.len(), 4);
    assert!(uids(&mut s, "Archive").await.1.is_empty());

    // A message already gone from the folder is reported, not "done".
    match act(&r, &acct, Folder::Inbox, &[u[2]], validity, Action::Read).await {
        Acted::Done { done, gone } => {
            assert!(done.is_empty());
            assert_eq!(gone, [u[2]]);
        }
        other => panic!("{other:?}"),
    }
    // A folder that is not there is refused rather than created.
    match act(
        &r,
        &acct,
        Folder::Inbox,
        &[u[5]],
        validity,
        Action::Move(Folder::Named("Nowhere".into())),
    )
    .await
    {
        Acted::NoPlace(_) => {}
        other => panic!("{other:?}"),
    }
    assert_eq!(uids(&mut s, "INBOX").await.1.len(), 4);
    s.logout().await.unwrap();
}

// --------------------------------------------------------------------- send

#[tokio::test]
async fn send_with_an_attachment_cc_and_bcc_leaves_no_bcc_header() {
    let r = resolver();
    let me = user("send");
    let to = user("to");
    let cc = user("cc");
    let bcc = user("bcc");
    let data: Vec<u8> = (0..=255u8).cycle().take(40_000).collect();
    let msg = Outgoing {
        from: Address::parse(&me).unwrap(),
        from_name: Some("Loopback Sender".into()),
        to: vec![Address::parse(&to).unwrap()],
        cc: vec![Address::parse(&cc).unwrap()],
        bcc: vec![Address::parse(&bcc).unwrap()],
        subject: "Quarterly figures".into(),
        body: "Figures attached.".into(),
        in_reply_to: None,
        attachments: vec![File {
            name: "figures.bin".into(),
            mime: "application/octet-stream".into(),
            data: data.clone(),
        }],
    };
    let written = match send(&r, &account(&me), &msg).await {
        Sent::Ok {
            via, message_id, ..
        } => {
            assert_eq!(via, "127.0.0.1:465");
            message_id
        }
        other => panic!("sending failed: {other:?}"),
    };

    // Every recipient, the blind one included, got the same message — and in
    // none of them is there a Bcc header.
    for who in [&to, &cc, &bcc] {
        let raw = received(who).await;
        let text = String::from_utf8_lossy(&raw);
        let head = &text[..text.find("\r\n\r\n").expect("a header section")];
        let lower = head.to_ascii_lowercase();
        assert!(
            !lower.contains("\nbcc:") && !lower.starts_with("bcc:"),
            "a Bcc header reached {who}:\n{head}"
        );
        assert!(!lower.contains(&bcc.to_ascii_lowercase()), "{head}");
        assert!(lower.contains("\ncc:"), "no Cc header:\n{head}");
        assert!(lower.contains(&cc.to_ascii_lowercase()), "{head}");
        assert!(head.contains(&format!("<{written}>")), "{head}");

        let parsed = mail_parser::MessageParser::default()
            .parse(&raw)
            .expect("parse");
        assert_eq!(parsed.subject(), Some("Quarterly figures"));
        let file = parsed.attachments().next().expect("the attachment");
        assert_eq!(file.attachment_name(), Some("figures.bin"));
        assert_eq!(file.contents(), &data[..], "the file arrived intact");
    }
}

// ------------------------------------------------------------------- drafts

/// A draft RATA saves twice leaves exactly one copy of it in Drafts, the
/// newer, with its Bcc and RATA's own id — and a draft another client saved
/// beside it is never touched.
#[tokio::test]
async fn a_draft_saved_again_leaves_one_copy_the_newer_and_nobody_else_s_is_touched() {
    let r = resolver();
    let me = user("draft");
    let acct = account(&me);
    let mut s = seed(&me).await;
    s.append(
        "Drafts",
        Some("(\\Seen \\Draft)"),
        Some(&date(0)),
        message(&me, "bob@rata.test", "Someone else's draft", ""),
    )
    .await
    .unwrap();
    let (_, before) = uids(&mut s, "Drafts").await;
    assert_eq!(before.len(), 1);

    let id = "0f8b6a52-3c1e-4d7a-9b2e-5a4c3d2e1f00";
    let mut msg = Outgoing {
        from: Address::parse(&me).unwrap(),
        from_name: None,
        to: vec![Address::parse("bob@rata.test").unwrap()],
        cc: vec![Address::parse("carol@rata.test").unwrap()],
        bcc: vec![Address::parse("dave@rata.test").unwrap()],
        subject: "Plan".into(),
        body: "First go".into(),
        in_reply_to: None,
        attachments: vec![],
    };
    let first = match save_draft(&r, &acct, &msg, id, 1, None).await {
        DraftSaved::Saved { saved, prior, .. } => {
            assert_eq!(prior, Prior::None);
            saved
        }
        other => panic!("first save failed: {other:?}"),
    };
    msg.body = "Second go".into();
    let (second, key) = match save_draft(&r, &acct, &msg, id, 2, Some(&first)).await {
        DraftSaved::Saved { saved, prior, id } => {
            // Dovecot has UIDPLUS: removed for good, by UID alone.
            assert_eq!(prior, Prior::Replaced);
            (saved, id)
        }
        other => panic!("second save failed: {other:?}"),
    };
    assert!(second.uid > first.uid);
    assert_eq!(second.uidvalidity, first.uidvalidity);
    assert_eq!(second.draft_id, id);

    // In Drafts now: the other client's draft and the newer copy, nothing else.
    let (validity, now) = uids(&mut s, "Drafts").await;
    assert_eq!(validity, second.uidvalidity);
    assert_eq!(now, vec![before[0], second.uid]);
    let got: Vec<_> = s
        .uid_fetch(second.uid.to_string(), "BODY.PEEK[]")
        .await
        .unwrap()
        .collect()
        .await;
    let raw = got[0].as_ref().unwrap().body().unwrap().to_vec();
    let text = String::from_utf8_lossy(&raw);
    assert!(text.contains("X-RATA-Draft-Rev: 2\r\n"), "{text}");
    assert!(text.contains(&format!("X-RATA-Draft: {id}\r\n")), "{text}");
    assert!(text.contains("Second go"), "{text}");
    let f = flags(&mut s, "Drafts", second.uid).await;
    assert!(f.iter().any(|x| x.contains("Draft")), "{f:?}");
    // Seen: RATA's own draft never turns up unread (BUG-M).
    assert!(f.iter().any(|x| x == "\\Seen"), "{f:?}");
    s.logout().await.unwrap();

    // A refresh reads it back as RATA's own, under the id the save gave.
    let fresh = fetch_newest(&r, &acct, 50, &[]).await.expect("sync");
    let drafts: Vec<_> = fresh
        .messages
        .iter()
        .filter(|m| m.folder == Folder::Drafts)
        .collect();
    assert_eq!(drafts.len(), 2, "{drafts:#?}");
    let mine = drafts.iter().find(|m| m.uid == second.uid).unwrap();
    assert_eq!(mine.id, key);
    assert_eq!(mine.draft_id.as_deref(), Some(id));
    assert_eq!(mine.bcc, ["dave@rata.test"]);
    assert!(!mine.unread, "{mine:?}");
    assert_eq!(mine.body.trim(), "Second go");
    let theirs = drafts.iter().find(|m| m.uid == before[0]).unwrap();
    assert_eq!(theirs.draft_id, None);
}

/// What the SMTP sink delivered to `who`, whole — waiting a little, since the
/// sink files it after it has answered.
async fn received(who: &str) -> Vec<u8> {
    for _ in 0..50 {
        let mut s = session(SINK_PORT, who).await;
        let mb = s.select("INBOX").await.unwrap();
        if mb.exists > 0 {
            let got: Vec<_> = s.fetch("1", "RFC822").await.unwrap().collect().await;
            let raw = got[0].as_ref().unwrap().body().unwrap().to_vec();
            let _ = s.logout().await;
            return raw;
        }
        let _ = s.logout().await;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("nothing was delivered to {who}");
}

// --------------------------------------------------------------------- IDLE

#[tokio::test]
async fn idle_wakes_on_an_append_but_not_on_a_flag_change() {
    let r = resolver();
    let me = user("idle");
    let mut s = seed(&me).await;
    put(&mut s, &me, "INBOX", 1, None).await;
    let (_, held) = uids(&mut s, "INBOX").await;

    let mut w = match watch(&r, &account(&me)).await {
        Ok(w) => w,
        Err(e) => panic!("watching failed: {e:?}"),
    };

    // RATA marking a message read on another connection is not new mail.
    let (quiet, _) = tokio::join!(w.wait(Duration::from_secs(4)), async {
        tokio::time::sleep(Duration::from_millis(500)).await;
        s.uid_store(held[0].to_string(), "+FLAGS (\\Seen)")
            .await
            .unwrap()
            .collect::<Vec<_>>()
            .await;
    });
    assert_eq!(quiet, Watched::Quiet);

    // New mail is.
    let started = std::time::Instant::now();
    let (woke, _) = tokio::join!(w.wait(Duration::from_secs(60)), async {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let raw = message("ann@rata.test", &me, "INBOX new", "");
        s.append("INBOX", None, None, raw).await.unwrap();
    });
    assert_eq!(woke, Watched::Arrived);
    assert!(
        started.elapsed() < Duration::from_secs(30),
        "woke only after {:?}",
        started.elapsed()
    );
    w.close().await;
    s.logout().await.unwrap();
}

// -------------------------------------------------------------- UIDVALIDITY

#[tokio::test]
async fn a_rebuilt_folder_is_read_afresh_and_its_old_uids_are_refused() {
    let r = resolver();
    let me = user("validity");
    let acct = account(&me);
    let mut s = seed(&me).await;
    put(&mut s, &me, "Junk", 2, None).await;

    let first = fetch_newest(&r, &acct, 50, &[]).await.unwrap();
    let junk: Vec<_> = first
        .messages
        .iter()
        .filter(|m| m.folder == Folder::Junk)
        .cloned()
        .collect();
    assert_eq!(junk.len(), 2);
    let old = junk[0].uidvalidity;
    let old_uids: Vec<u32> = junk.iter().map(|m| m.uid).collect();
    let known = [Known {
        folder: Folder::Junk,
        uidvalidity: old,
        since: *old_uids.iter().max().unwrap(),
    }];

    // Another client deletes the folder and makes it again: the same name, a
    // new generation, and message numbers that start over.
    s.select("INBOX").await.unwrap();
    s.delete("Junk").await.unwrap();
    let _ = s.create("Junk").await; // may already be back: Dovecot auto-creates it
    put(&mut s, &me, "Junk", 1, None).await;
    let (new, now) = uids(&mut s, "Junk").await;
    assert_ne!(
        new, old,
        "the server did not give the folder a new UIDVALIDITY"
    );
    s.logout().await.unwrap();

    // A refresh reads it afresh rather than "newer than" a number from before.
    let next = fetch_newest(&r, &acct, 50, &known).await.unwrap();
    let fresh: Vec<_> = next
        .messages
        .iter()
        .filter(|m| m.folder == Folder::Junk)
        .collect();
    assert_eq!(fresh.len(), 1);
    assert_eq!(fresh[0].uidvalidity, new);
    assert_eq!(fresh[0].uid, now[0]);

    // And nothing acts on, or reads under, the old numbers.
    assert!(matches!(
        act(&r, &acct, Folder::Junk, &old_uids, old, Action::Trash).await,
        Acted::Stale(_)
    ));
    assert!(matches!(
        fetch_older(&r, &acct, Folder::Junk, old_uids[0], old, 50).await,
        Fetched::Stale(_)
    ));
    assert!(matches!(
        fetch_uids(&r, &acct, Folder::Junk, &old_uids, old).await,
        Fetched::Stale(_)
    ));
    assert!(matches!(
        fetch_whole(&r, &acct, Folder::Junk, old_uids[0], old).await,
        Whole::Stale(_)
    ));
    // The message that is there now was not touched.
    assert_eq!(
        messages(fetch_uids(&r, &acct, Folder::Junk, &now, new).await).len(),
        1
    );
}

// ---------------------------------------------------------------- GreenMail
//
// A server unlike Dovecot in the two ways that found bugs: it declares no
// special-use folders, so Archive is known only by its name, and it answers a
// partial fetch as `BODY[]<0>{n}`, which the parser RATA uses cannot read.

/// GreenMail's IMAP over TLS, for the engine.
const GREENMAIL_TLS: u16 = 3993;

fn greenmail(email: &str) -> Account {
    Account {
        port: GREENMAIL_TLS,
        ..account(email)
    }
}

#[tokio::test]
async fn greenmail_a_refresh_lists_every_message_it_cannot_read_and_says_so() {
    let r = resolver();
    let me = user("gm-read");
    let mut s = session(SINK_PORT, &me).await;
    put(&mut s, &me, "INBOX", 3, None).await;
    s.logout().await.unwrap();

    // Every reply with text in it is one RATA cannot read here. The refresh
    // still succeeds, lists all three, and says each has not been read.
    let got = fetch_newest(&r, &greenmail(&me), 50, &[])
        .await
        .unwrap_or_else(|e| panic!("the refresh failed: {e:?}"));
    assert_eq!(
        in_folder(&got.messages, &Folder::Inbox),
        ["INBOX 0", "INBOX 1", "INBOX 2"]
    );
    for m in &got.messages {
        assert!(m.uid > 0 && m.uidvalidity > 0, "can be found again: {m:?}");
        assert!(m.truncated, "opening it fetches it whole: {m:?}");
        assert!(m.body.contains("RATA"), "says why: {}", m.body);
        assert!(!m.body.contains("The text of"), "{}", m.body);
    }
    eprintln!(
        "marked: {:?}",
        got.messages.iter().map(|m| &m.body).collect::<Vec<_>>()
    );

    // Opening one asks for it whole, which GreenMail sends in a form that
    // can be read.
    let m = &got.messages[0];
    match fetch_whole(&r, &greenmail(&me), Folder::Inbox, m.uid, m.uidvalidity).await {
        Whole::Raw(raw) => {
            let text = String::from_utf8_lossy(&raw);
            let want = format!("The text of {}.", m.subject);
            assert!(text.contains(&want), "{text}");
        }
        other => panic!("opening it failed: {other:?}"),
    }
}

#[tokio::test]
async fn greenmail_archive_by_name_is_read_and_archived_to() {
    let r = resolver();
    let me = user("gm-archive");
    let acct = greenmail(&me);
    let mut s = session(SINK_PORT, &me).await;
    s.create("Archive").await.unwrap();
    put(&mut s, &me, "INBOX", 2, None).await;
    let (uv, inbox) = uids(&mut s, "INBOX").await;
    assert_eq!(inbox.len(), 2);

    // Delete still wants a folder the server declares as Trash, and there
    // is none: refused, nothing touched.
    assert!(matches!(
        act(&r, &acct, Folder::Inbox, &inbox, uv, Action::Trash).await,
        Acted::NoPlace(_)
    ));
    assert_eq!(uids(&mut s, "INBOX").await.1, inbox);

    // Archive, on a server that declares no \Archive, only a folder named so.
    match act(&r, &acct, Folder::Inbox, &inbox, uv, Action::Archive).await {
        Acted::Done { done, gone } => {
            assert_eq!(done, inbox);
            assert!(gone.is_empty());
        }
        other => panic!("archiving was refused: {other:?}"),
    }
    let (_, archived) = uids(&mut s, "Archive").await;
    assert_eq!(archived.len(), 2, "both are in Archive now");
    s.logout().await.unwrap();

    // A refresh reads that folder as the archive, so the mail is shown where
    // it was put, and not in the inbox.
    let got = fetch_newest(&r, &acct, 50, &[])
        .await
        .unwrap_or_else(|e| panic!("the refresh failed: {e:?}"));
    assert_eq!(
        in_folder(&got.messages, &Folder::Archive),
        ["INBOX 0", "INBOX 1"]
    );
    assert!(in_folder(&got.messages, &Folder::Inbox).is_empty());
}

#[tokio::test]
async fn greenmail_delete_moves_to_a_folder_named_trash() {
    let r = resolver();
    let me = user("gm-trash");
    let acct = greenmail(&me);
    let mut s = session(SINK_PORT, &me).await;
    // GreenMail declares no \Trash: this folder is Trash by its name alone.
    s.create("Trash").await.unwrap();
    put(&mut s, &me, "INBOX", 2, None).await;
    let (uv, inbox) = uids(&mut s, "INBOX").await;
    assert_eq!(inbox.len(), 2);

    match act(&r, &acct, Folder::Inbox, &inbox[..1], uv, Action::Trash).await {
        Acted::Done { done, gone } => {
            assert_eq!(done, inbox[..1]);
            assert!(gone.is_empty());
        }
        other => panic!("delete was refused: {other:?}"),
    }
    // Moved, not deleted: in Trash now, and only that one left the inbox.
    let (_, trashed) = uids(&mut s, "Trash").await;
    assert_eq!(trashed.len(), 1, "one message is in Trash");
    let (_, left) = uids(&mut s, "INBOX").await;
    assert_eq!(left, inbox[1..]);
    s.logout().await.unwrap();
}
