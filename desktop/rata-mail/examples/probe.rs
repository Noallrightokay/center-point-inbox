//! A manual probe: connect to a real IMAP server and see what comes back.
//! Not a test — it depends on somebody else's server being up.
//!   cargo run --example probe -- someone@gmail.com wrong-password
use rata_mail::{verify, Resolver, Verify};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let mut args = std::env::args().skip(1);
    let email = args.next().expect("email");
    let pass = args.next().unwrap_or_else(|| "definitely-wrong".into());
    let over = args.next();
    let r = Resolver::system().expect("resolver");
    match verify(&r, &email, &pass, over.as_deref()).await {
        Verify::Ok(v) => println!("OK      {}:{} ({:?}) {}", v.host, v.port, v.source, v.label),
        Verify::Refused(w) => println!("REFUSED {w}"),
        Verify::NeedsHost(w) => println!("NEEDS   {w}"),
        Verify::Failed(w) => println!("FAILED  {w}"),
    }
}
