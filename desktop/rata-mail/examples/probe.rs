//! A manual probe against a real mail server. Not a test — it needs a real
//! account on a network that allows 993 and 465.
//!
//!   cargo run --example probe -- link  you@example.com 'app password' [server]
//!   cargo run --example probe -- fetch you@example.com 'app password' imap.example.com
//!   cargo run --example probe -- send  you@example.com 'app password' imap.example.com them@elsewhere.org
use rata_mail::{
    fetch_inbox, send, verify, Account, Address, Fetched, Outgoing, Resolver, Sent, Verify,
};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let what = a.first().map(String::as_str).unwrap_or("link");
    let email = a.get(1).cloned().unwrap_or_default();
    let pass = a.get(2).cloned().unwrap_or_else(|| "definitely-wrong".into());
    let host = a.get(3).cloned().unwrap_or_default();
    let r = Resolver::system().expect("resolver");

    let acct = Account {
        email: email.clone(),
        pass: pass.clone(),
        host: host.clone(),
        port: 993,
        label: String::new(),
    };

    match what {
        "link" => match verify(&r, &email, &pass, (!host.is_empty()).then_some(&host[..])).await {
            Verify::Ok(v) => println!("OK      {}:{} ({:?}) {}", v.host, v.port, v.source, v.label),
            Verify::Refused(w) => println!("REFUSED {w}"),
            Verify::NeedsHost(w) => println!("NEEDS   {w}"),
            Verify::Failed(w) => println!("FAILED  {w}"),
        },
        "fetch" => match fetch_inbox(&r, &acct, 15).await {
            Fetched::Messages(m) => {
                println!("{} message(s)", m.len());
                for x in m {
                    println!(
                        "  [{}{}] {:<28} {}",
                        if x.unread { "•" } else { " " },
                        if x.starred { "★" } else { " " },
                        rata_mail::words::clip(&x.from_name, 28),
                        rata_mail::words::clip(&x.subject, 70)
                    );
                }
            }
            other => println!("{other:?}"),
        },
        "send" => {
            let to = a.get(4).cloned().unwrap_or_default();
            let msg = Outgoing {
                from: Address::parse(&email).expect("from address"),
                from_name: None,
                to: Address::parse_list(&to).expect("recipient"),
                subject: "RATA test — please ignore".into(),
                body: "Sent by the RATA desktop mail layer.\n".into(),
                in_reply_to: None,
            };
            match send(&r, &acct, &msg).await {
                Sent::Ok { via, id } => println!("SENT    via {via} — {id}"),
                other => println!("{other:?}"),
            }
        }
        _ => eprintln!("link | fetch | send"),
    }
}
