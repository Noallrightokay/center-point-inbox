#!/usr/bin/env bash
# Real mail servers on this machine, for `cargo test --features loopback-tests`.
#
# Every other test in this crate talks to a scripted server that says what the
# test author expected a server to say. These talk to real ones:
#
#   Dovecot    IMAP over TLS on 127.0.0.1:993, the port RATA always uses, with
#              RFC 6154 special-use folders (Sent, Drafts, Junk, Trash,
#              Archive), IDLE, MOVE and UIDPLUS — what most hosted mail runs.
#              Also plain IMAP on 127.0.0.1:1143, which only the tests use, to
#              put mail in place and to rebuild folders behind RATA's back.
#   GreenMail  SMTP over TLS on 127.0.0.1:465, the first port RATA tries, as the
#              sink a sent message lands in; plain IMAP on 127.0.0.1:3143 so a
#              test can read back exactly what the sink received.
#
# Not GreenMail for RATA's IMAP too: it declares no special-use folders, and it
# answers a partial fetch as `BODY[]<0>{71}` with no space before the literal,
# which RFC 3501 requires and the IMAP parser RATA uses rejects, so no refresh
# against it gets past the first message.
#
# Both present a certificate for 127.0.0.1 signed by a CA made here, now, and
# thrown away with the directory. RATA trusts it only because the test process
# is started with SSL_CERT_FILE pointing at that CA (rustls-platform-verifier
# reads it on Linux, as OpenSSL does): nothing in the engine knows about it.
#
# Usage (as root — Dovecot binds 993 and drops to its own users, GreenMail binds
# 465):
#
#   sudo desktop/rata-mail/tests/loopback/servers.sh start <dir> [greenmail.jar]
#   SSL_CERT_FILE=<dir>/ca.crt cargo test --features loopback-tests --test loopback
#   sudo desktop/rata-mail/tests/loopback/servers.sh stop <dir>
#
# `--test loopback`, not every target: with the feature on, the four unit tests
# that use 127.0.0.1 as their example of a refused address fail, as they should
# in a build that allows it. They run, and pass, in the default build.
#
# Needs: dovecot-imapd (apt), a Java runtime, openssl. Without a jar, GreenMail
# is downloaded from Maven Central and checked against the SHA-256 below.
set -euo pipefail

GREENMAIL_VERSION=2.1.14
GREENMAIL_SHA256=0381392f3a44e4d8ae78051778440f58820d01acaf5757c3f43649deda8c1d23
# The password every test account uses. Test-only: these servers accept any
# user name with it, and exist for the length of one CI job.
PASS=rata-loopback

cmd=${1:-}
dir=${2:-}
if [[ -z "$cmd" || -z "$dir" ]]; then
  echo "usage: $0 start|stop <dir> [greenmail.jar]" >&2
  exit 2
fi

stop() {
  if [[ -f "$dir/dovecot.conf" ]]; then
    doveadm -c "$dir/dovecot.conf" stop 2>/dev/null || true
    rm -rf "$(sed -n 's/^base_dir = //p' "$dir/dovecot.conf")"
  fi
  if [[ -f "$dir/greenmail.pid" ]]; then
    kill "$(cat "$dir/greenmail.pid")" 2>/dev/null || true
    rm -f "$dir/greenmail.pid"
  fi
}

if [[ "$cmd" == stop ]]; then
  stop
  exit 0
fi
if [[ "$cmd" != start ]]; then
  echo "unknown command: $cmd" >&2
  exit 2
fi

mkdir -p "$dir"
dir=$(cd "$dir" && pwd)

# ------------------------------------------------------------ certificates
# A CA, and a server certificate for the address 127.0.0.1 signed by it. The
# CA is separate from the leaf on purpose: webpki refuses a CA certificate used
# as a server's own (CaUsedAsEndEntity), so a lone self-signed certificate
# would fail for a reason no customer's server has.
openssl req -x509 -newkey rsa:2048 -nodes -days 2 \
  -keyout "$dir/ca.key" -out "$dir/ca.crt" \
  -subj "/CN=RATA loopback test CA" \
  -addext "basicConstraints=critical,CA:TRUE" \
  -addext "keyUsage=critical,keyCertSign,cRLSign" 2>/dev/null
openssl req -newkey rsa:2048 -nodes \
  -keyout "$dir/server.key" -out "$dir/server.csr" \
  -subj "/CN=127.0.0.1" 2>/dev/null
cat > "$dir/server.ext" <<'EOF'
basicConstraints=critical,CA:FALSE
keyUsage=critical,digitalSignature,keyEncipherment
extendedKeyUsage=serverAuth
subjectAltName=IP:127.0.0.1
EOF
openssl x509 -req -in "$dir/server.csr" -CA "$dir/ca.crt" -CAkey "$dir/ca.key" \
  -CAcreateserial -days 2 -extfile "$dir/server.ext" -out "$dir/server.crt" 2>/dev/null
cat "$dir/server.crt" "$dir/ca.crt" > "$dir/chain.crt"
openssl pkcs12 -export -name server -inkey "$dir/server.key" -in "$dir/server.crt" \
  -certfile "$dir/ca.crt" -out "$dir/greenmail.p12" -passout "pass:$PASS"
chmod 644 "$dir"/*.crt "$dir/greenmail.p12"

# ------------------------------------------------------------------ Dovecot
# Dovecot's sockets and mail live under a short path of their own: a Unix
# socket's path is limited to about a hundred bytes, <dir> may be deep, and the
# mail processes run as nobody, who must be able to reach the mail.
run=$(mktemp -d /tmp/rata-dovecot.XXXXXX)
chmod 755 "$run"
mkdir -p "$dir/dovecot/state" "$run/mail"
chown nobody:nogroup "$run/mail"
dh=""
if [[ -f /usr/share/dovecot/dh.pem ]]; then
  dh="ssl_dh = </usr/share/dovecot/dh.pem"
fi
cat > "$dir/dovecot.conf" <<EOF
# Written by servers.sh for RATA's loopback tests. Not for any real use.
base_dir = $run
state_dir = $dir/dovecot/state
log_path = $dir/dovecot.log
protocols = imap
listen = 127.0.0.1

ssl = yes
ssl_cert = <$dir/chain.crt
ssl_key = <$dir/server.key
$dh
# Plain IMAP is only on 1143, for the tests' own seeding client.
disable_plaintext_auth = no
auth_mechanisms = plain login

passdb {
  driver = static
  args = password=$PASS
}
userdb {
  driver = static
  args = uid=nobody gid=nogroup home=$run/mail/%u
}
first_valid_uid = 1
mail_location = maildir:~/Maildir

namespace inbox {
  inbox = yes
  mailbox Drafts {
    special_use = \\Drafts
    auto = subscribe
  }
  mailbox Sent {
    special_use = \\Sent
    auto = subscribe
  }
  mailbox Junk {
    special_use = \\Junk
    auto = subscribe
  }
  mailbox Trash {
    special_use = \\Trash
    auto = subscribe
  }
  mailbox Archive {
    special_use = \\Archive
    auto = subscribe
  }
}

service imap-login {
  inet_listener imap {
    address = 127.0.0.1
    port = 1143
  }
  inet_listener imaps {
    address = 127.0.0.1
    port = 993
    ssl = yes
  }
}
EOF
dovecot -c "$dir/dovecot.conf"

# ---------------------------------------------------------------- GreenMail
jar=${3:-}
if [[ -z "$jar" ]]; then
  jar="$dir/greenmail-standalone-$GREENMAIL_VERSION.jar"
  curl -fsSL -o "$jar" \
    "https://repo1.maven.org/maven2/com/icegreen/greenmail-standalone/$GREENMAIL_VERSION/greenmail-standalone-$GREENMAIL_VERSION.jar"
fi
echo "$GREENMAIL_SHA256  $jar" | sha256sum -c --quiet -

# Standard SMTPS (465) for the engine; the test-offset plain IMAP (3143) for
# the test to read the sink back. auth.disabled: any user name and password is
# accepted and an account is made on first delivery, which is what a sink is.
nohup java \
  -Dgreenmail.setup.smtps \
  -Dgreenmail.setup.test.imap \
  -Dgreenmail.hostname=127.0.0.1 \
  -Dgreenmail.auth.disabled \
  -Dgreenmail.tls.keystore.file="$dir/greenmail.p12" \
  -Dgreenmail.tls.keystore.password="$PASS" \
  -Dgreenmail.verbose \
  -jar "$jar" > "$dir/greenmail.log" 2>&1 &
echo $! > "$dir/greenmail.pid"

# ----------------------------------------------------------------- wait
ready() {
  (exec 3<>"/dev/tcp/127.0.0.1/$1") 2>/dev/null
}
for port in 993 1143 465 3143; do
  for _ in $(seq 1 60); do
    if ready "$port"; then
      continue 2
    fi
    sleep 0.5
  done
  echo "nothing is listening on 127.0.0.1:$port" >&2
  tail -n 40 "$dir/dovecot.log" "$dir/greenmail.log" >&2 || true
  exit 1
done
echo "Dovecot on 127.0.0.1:993 (TLS) and :1143, GreenMail on 127.0.0.1:465 (TLS) and :3143."
echo "Run the tests with SSL_CERT_FILE=$dir/ca.crt"
