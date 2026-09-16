import { ImapFlow } from 'imapflow';
import { createHash } from 'node:crypto';
import { lookup, resolveMx, resolveSrv } from 'node:dns/promises';
import { isIP } from 'node:net';

/* ---------------------------------------------------------------------------
   Mail accounts — any provider, more than one of them.

   RATA reaches mail over IMAP with an app-specific password. That is not a
   fallback for Gmail and iCloud alone: it is the one mechanism every mail host
   on earth supports, needs no per-provider OAuth app, no verification review,
   and no annual CASA assessment. A user brings an address and a revocable
   password and their mail appears — whether that address is at Gmail, Fastmail,
   Zoho, or their accountant's own server.

   Two things this file exists to solve:

   1. Finding the server. People know their email address; almost nobody knows
      their IMAP hostname. Known domains resolve from the table below, and
      anything else is probed at the conventional names before we give up and
      ask. The user can always override.

   2. Holding more than one account. provider_tokens is keyed (user_id,
      provider), so a literal provider name like 'gmail_imap' allows exactly one
      Gmail per person. Each account therefore gets its own synthetic provider
      key — 'mail:<slug of the address>' — which keeps one row per account
      inside the existing primary key. No migration, and the address stays
      readable in the table.
   --------------------------------------------------------------------------- */

export const MAIL_HOSTS = {
  'gmail.com':      { host: 'imap.gmail.com',        label: 'Gmail',        help: 'myaccount.google.com/apppasswords — needs 2-Step Verification on' },
  'googlemail.com': { host: 'imap.gmail.com',        label: 'Gmail',        help: 'myaccount.google.com/apppasswords — needs 2-Step Verification on' },
  'icloud.com':     { host: 'imap.mail.me.com',      label: 'iCloud Mail',  help: 'appleid.apple.com → Sign-In and Security → App-Specific Passwords' },
  'me.com':         { host: 'imap.mail.me.com',      label: 'iCloud Mail',  help: 'appleid.apple.com → Sign-In and Security → App-Specific Passwords' },
  'mac.com':        { host: 'imap.mail.me.com',      label: 'iCloud Mail',  help: 'appleid.apple.com → Sign-In and Security → App-Specific Passwords' },
  'outlook.com':    { host: 'outlook.office365.com', label: 'Outlook',      help: 'account.microsoft.com → Security → App passwords (needs two-step verification)' },
  'hotmail.com':    { host: 'outlook.office365.com', label: 'Outlook',      help: 'account.microsoft.com → Security → App passwords (needs two-step verification)' },
  'live.com':       { host: 'outlook.office365.com', label: 'Outlook',      help: 'account.microsoft.com → Security → App passwords (needs two-step verification)' },
  'msn.com':        { host: 'outlook.office365.com', label: 'Outlook',      help: 'account.microsoft.com → Security → App passwords (needs two-step verification)' },
  'yahoo.com':      { host: 'imap.mail.yahoo.com',   label: 'Yahoo Mail',   help: 'login.yahoo.com → Account security → Generate app password' },
  'ymail.com':      { host: 'imap.mail.yahoo.com',   label: 'Yahoo Mail',   help: 'login.yahoo.com → Account security → Generate app password' },
  'aol.com':        { host: 'imap.aol.com',          label: 'AOL Mail',     help: 'login.aol.com → Account security → Generate app password' },
  'zoho.com':       { host: 'imap.zoho.com',         label: 'Zoho Mail',    help: 'accounts.zoho.com → Security → App passwords' },
  'fastmail.com':   { host: 'imap.fastmail.com',     label: 'Fastmail',     help: 'fastmail.com → Settings → Privacy & Security → App passwords' },
  'fastmail.fm':    { host: 'imap.fastmail.com',     label: 'Fastmail',     help: 'fastmail.com → Settings → Privacy & Security → App passwords' },
  'gmx.com':        { host: 'imap.gmx.com',          label: 'GMX',          help: 'Enable IMAP in GMX settings' },
  'gmx.net':        { host: 'imap.gmx.net',          label: 'GMX',          help: 'Enable IMAP in GMX settings' },
  'mail.com':       { host: 'imap.mail.com',         label: 'Mail.com',     help: 'Enable IMAP in your Mail.com settings' },
  'web.de':         { host: 'imap.web.de',           label: 'WEB.DE',       help: 'Enable IMAP in your WEB.DE settings' },
  'yandex.com':     { host: 'imap.yandex.com',       label: 'Yandex Mail',  help: 'id.yandex.com → Security → App passwords' },
  'comcast.net':    { host: 'imap.comcast.net',      label: 'Comcast',      help: 'Use your Xfinity password, with third-party access enabled' },
  'att.net':        { host: 'imap.mail.att.net',     label: 'AT&T Mail',    help: 'currently.att.net → Profile → Sign-in info → Manage secure mail key' },
  'verizon.net':    { host: 'imap.aol.com',          label: 'Verizon Mail', help: 'Verizon mail is served by AOL — generate an AOL app password' },
  'rogers.com':     { host: 'imap.broadband.rogers.com', label: 'Rogers',   help: 'Use your Rogers email password' },
  'shaw.ca':        { host: 'imap.shaw.ca',          label: 'Shaw',         help: 'Use your Shaw email password' },
  'bell.net':       { host: 'imap.bell.net',         label: 'Bell',         help: 'Use your Bell email password' },
  'sympatico.ca':   { host: 'imap.bell.net',         label: 'Bell',         help: 'Use your Bell email password' },
};

/* Proton and Tuta encrypt mail client-side, so there is no IMAP server on the
   internet to reach — only a bridge running on the user's own machine, which a
   web app cannot see. Saying so plainly beats a generic timeout. */
export const NO_IMAP = {
  'proton.me':      'Proton Mail',
  'protonmail.com': 'Proton Mail',
  'pm.me':          'Proton Mail',
  'tuta.com':       'Tuta',
  'tutanota.com':   'Tuta',
};

export const PORT = 993;

/* ---------------------------------------------------------------------------
   Sending.

   Reading mail is IMAP; sending it is SMTP, and the same app password almost
   always works for both. Most providers name their submission host by swapping
   imap for smtp, so that is the rule, with a table for the ones that do not
   follow it. Port 465 is implicit TLS and 587 is STARTTLS; providers differ, so
   both are tried rather than asking the user which their host prefers.
   --------------------------------------------------------------------------- */
export const SMTP_HOSTS = {
  'imap.gmail.com':        'smtp.gmail.com',
  'imap.mail.me.com':      'smtp.mail.me.com',
  'outlook.office365.com': 'smtp-mail.outlook.com',
  'imap.mail.yahoo.com':   'smtp.mail.yahoo.com',
  'imap.aol.com':          'smtp.aol.com',
  'imap.zoho.com':         'smtp.zoho.com',
  'imap.fastmail.com':     'smtp.fastmail.com',
  'imap.gmx.com':          'mail.gmx.com',
  'imap.gmx.net':          'mail.gmx.net',
  'imap.mail.com':         'smtp.mail.com',
  'imap.web.de':           'smtp.web.de',
  'imap.yandex.com':       'smtp.yandex.com',
  'imap.comcast.net':      'smtp.comcast.net',
  'imap.mail.att.net':     'smtp.mail.att.net',
  'imap.shaw.ca':          'smtp.shaw.ca',
  'imap.bell.net':         'smtp.bell.net',
  'imap.broadband.rogers.com': 'smtp.broadband.rogers.com',
};

/* Submission hosts to try, best first.

   One name is not always enough. outlook.office365.com serves both a consumer
   Outlook address and a company's Microsoft 365 mailbox, and the two submit to
   different hosts — so both are offered and the one that accepts wins. Most
   providers need only the first. */
export function smtpCandidates(imapHost, email, env = process.env) {
  const out = [];
  const add = h => { if (h && !out.includes(h)) out.push(h); };

  /* A RATA-hosted address submits to RATA's own server, which is the same host
     it reads from unless the deployment says otherwise. */
  if (isRatamail(email, env)) {
    add(env.RATA_SMTP_HOST || env.RATA_MAIL_HOST);
    return out.filter(Boolean);
  }

  if (imapHost === 'outlook.office365.com') { add('smtp.office365.com'); add('smtp-mail.outlook.com'); }
  else add(SMTP_HOSTS[imapHost]);

  if (imapHost && imapHost.startsWith('imap.')) add('smtp.' + imapHost.slice(5));
  /* A host that is not named imap.<something> is usually the submission host
     too — mail.privateemail.com and secure.emailsrvr.com both are. */
  else add(imapHost);

  /* Only when nothing above produced a name. Every extra candidate is two more
     connection timeouts on the way to the same failure. */
  if (!out.length) add('smtp.' + domainOf(email));
  return out.filter(Boolean);
}

export function smtpHostFor(imapHost, email) {
  if (imapHost && SMTP_HOSTS[imapHost]) return SMTP_HOSTS[imapHost];
  if (imapHost && imapHost.startsWith('imap.')) return 'smtp.' + imapHost.slice(5);
  if (imapHost) return imapHost;
  return 'smtp.' + domainOf(email);
}

/* 465 first: implicit TLS is encrypted from the first byte, where 587 starts in
   the clear and upgrades. If a host only offers 587 we still get there. */
export const SMTP_PORTS = [
  { port: 465, secure: true },
  { port: 587, secure: false },
];


export function domainOf(email) {
  return String(email || '').trim().toLowerCase().split('@')[1] || '';
}

/* The provider key for one account: a readable slug of the address, plus a
   digest of the exact address.

   The slug alone is not enough, and the way it fails is silent. Folding
   everything outside [a-z0-9] to '_' maps 'a.b@x.com' and 'a-b@x.com' onto the
   same key — and since (user_id, provider) is the primary key, linking the
   second would upsert straight over the first: one mailbox quietly replaced by
   another, and a later unlink removing the wrong one. Truncation did the same
   to long addresses.

   The digest is of the address as written, so distinct addresses cannot share
   a key, while the slug keeps the row identifiable by eye in the table. */
export function mailKey(email) {
  const addr = String(email).trim().toLowerCase();
  const slug = addr.replace(/[^a-z0-9]+/g, '_').slice(0, 120);
  const tag = createHash('sha256').update(addr).digest('hex').slice(0, 10);
  return `mail:${slug}.${tag}`;
}

export function isMailKey(provider) {
  return typeof provider === 'string' && provider.startsWith('mail:');
}

/* What we know about an address before anyone types a password: which server
   to try, and where that provider hides its app-password screen. */
/* ---------------------------------------------------------------------------
   Mail RATA hosts itself.

   Pro and Enterprise can be given addresses on RATA's own server. From
   everything above, such an address is an ordinary mailbox at an ordinary IMAP
   host — the only unusual thing is who runs the far end.

   Which is why it is configured rather than written in. RATA_MAIL_HOST is the
   IMAP host of RATA's mail server, and until it is set, nothing here is true:
   an @mailrata.org address falls through to the same DNS discovery as anybody
   else's, finds nothing, and asks. Hard-coding it before the server exists
   would point users at a host that is not answering and call it a mail
   problem.

   RATA_MAIL_DOMAIN exists so a staging deployment can host mail somewhere else
   without pretending to be the live one. */
export function ratamailDomain(env = process.env) {
  return env.RATA_MAIL_HOST ? String(env.RATA_MAIL_DOMAIN || 'mailrata.org').toLowerCase() : null;
}

export function ratamailHost(env = process.env) {
  const d = ratamailDomain(env);
  if (!d) return null;
  return {
    host: String(env.RATA_MAIL_HOST).toLowerCase(),
    label: 'RATA Mail',
    help: 'Settings → Your RATA addresses → the password shown there. It is separate from your RATA account password.',
  };
}

export function isRatamail(email, env = process.env) {
  const d = ratamailDomain(env);
  return !!d && domainOf(email) === d;
}

export function describe(email) {
  const domain = domainOf(email);
  if (!domain) return null;

  /* Before the table, because RATA's own domain is not in it — and should not
     be, since which domain that is depends on the deployment. */
  const own = isRatamail(email) ? ratamailHost() : null;
  if (own) return { domain, ...own, guessed: false, hostedByRata: true };

  if (NO_IMAP[domain]) {
    return { domain, unsupported: true, label: NO_IMAP[domain],
      why: `${NO_IMAP[domain]} encrypts mail on your device and offers no IMAP server RATA can reach. Their bridge only runs on your own computer.` };
  }
  const known = MAIL_HOSTS[domain];
  if (known) return { domain, ...known, guessed: false };
  /* Everything else is settled by the domain's own DNS at link time — see
     discoverHosts. This is only what can be said before that lookup runs. */
  return {
    domain,
    host: 'imap.' + domain,
    label: domain,
    help: 'Your mail provider or IT administrator issues this — look for "app password", "IMAP password", or "mail client access".',
    guessed: true,
  };
}

/* ---------------------------------------------------------------------------
   Finding the server for a domain nobody has heard of.

   The table above covers consumer mail. It does not cover the case RATA exists
   for: someone whose address is at their own company's domain. Guessing
   imap.<domain> works for a small host running its own server and fails for
   almost every business, because most businesses do not run a mail server —
   they point their domain at one. thesherwood.group has no imap.thesherwood
   .group; it has an MX record pointing at Google.

   So the domain's DNS is asked, which is the same thing a mail client does:

     1. an SRV record (RFC 6186), if the domain publishes where its IMAP is
     2. the MX records, mapped to the IMAP host of whoever serves the mail
     3. the conventional names, for a host that really does run its own

   Two things the MX deliberately does not resolve to a guess. A filtering
   service in front of the real mailbox (Proofpoint, Mimecast, Barracuda) says
   nothing about where the mailbox is, and connecting to a filter's own host
   would fail confusingly. A forwarding service has no mailbox at all. Both are
   named rather than probed, so the user is told what is actually going on.
   --------------------------------------------------------------------------- */

const MX_HOSTS = [
  { match: /(^|\.)(google|googlemail)\.com$/,        host: 'imap.gmail.com',        label: 'Google Workspace',
    help: 'myaccount.google.com/apppasswords, signed in with your work address — needs 2-Step Verification on' },
  { match: /(^|\.)(outlook\.com|office365\.com)$/,   host: 'outlook.office365.com', label: 'Microsoft 365',
    help: 'Your Microsoft 365 account → Security → App passwords. Some organisations disable these — your IT administrator can tell you.' },
  { match: /(^|\.)zoho\.(com|eu|in)$/,               host: 'imap.zoho.com',         label: 'Zoho Mail',
    help: 'accounts.zoho.com → Security → App passwords' },
  { match: /(^|\.)messagingengine\.com$/,            host: 'imap.fastmail.com',     label: 'Fastmail',
    help: 'fastmail.com → Settings → Privacy & Security → App passwords' },
  { match: /(^|\.)icloud\.com$/,                     host: 'imap.mail.me.com',      label: 'iCloud Mail',
    help: 'appleid.apple.com → Sign-In and Security → App-Specific Passwords' },
  { match: /(^|\.)yahoodns\.net$/,                   host: 'imap.mail.yahoo.com',   label: 'Yahoo Mail',
    help: 'login.yahoo.com → Account security → Generate app password' },
  { match: /(^|\.)secureserver\.net$/,               host: 'imap.secureserver.net',  label: 'GoDaddy',
    help: 'Use your mailbox password, or an app password if your plan issues them' },
  { match: /(^|\.)registrar-servers\.com$/,          host: 'mail.privateemail.com', label: 'Namecheap Private Email',
    help: 'Use your Private Email mailbox password' },
  { match: /(^|\.)titan\.email$/,                    host: 'imap.titan.email',      label: 'Titan',
    help: 'Use your Titan mailbox password' },
  { match: /(^|\.)(ionos|1and1)\.(com|co\.uk)$|(^|\.)(kundenserver|perfora)\.(de|net)$/,
                                                     host: 'imap.ionos.com',        label: 'IONOS',
    help: 'Use your IONOS mailbox password' },
  { match: /(^|\.)emailsrvr\.com$/,                  host: 'secure.emailsrvr.com',  label: 'Rackspace Email',
    help: 'Use your Rackspace mailbox password' },
  { match: /(^|\.)hostinger\.com$/,                  host: 'imap.hostinger.com',    label: 'Hostinger Email',
    help: 'hPanel → Emails → the mailbox password' },
  { match: /(^|\.)migadu\.com$/,                     host: 'imap.migadu.com',       label: 'Migadu',
    help: 'admin.migadu.com → the mailbox → its password' },
  { match: /(^|\.)mailbox\.org$/,                    host: 'imap.mailbox.org',      label: 'mailbox.org',
    help: 'Use your mailbox.org password' },
  { match: /(^|\.)mail\.de$/,                        host: 'imap.mail.de',          label: 'mail.de',
    help: 'Use your mail.de password' },

  /* Ends the search rather than pointing at a server. */
  { match: /(^|\.)(protonmail\.ch|proton\.me|protonmail\.com)$/, refuse:
    'That domain’s mail is hosted by Proton, which encrypts it on the device and offers no IMAP server RATA can reach. Their bridge only runs on your own computer.' },
  { match: /(^|\.)tuta(nota)?\.(com|de)$/, refuse:
    'That domain’s mail is hosted by Tuta, which encrypts it on the device and offers no IMAP server RATA can reach.' },
  { match: /(^|\.)(improvmx|forwardemail\.net|mailgun\.org|sendgrid\.net|mxroute)\.?/, refuse:
    'That domain forwards its mail somewhere else rather than keeping a mailbox of its own. Link the address the mail is forwarded to — that is where it actually lands.' },

  /* A filter in front of the real mailbox. Where the mailbox itself is, the MX
     does not say — so say that, instead of connecting to the filter. */
  { match: /(^|\.)(pphosted\.com|mimecast\.com|barracudanetworks\.com|messagelabs\.com|iphmx\.com|trendmicro\.com)$/,
    filtered: true },
];

export function mailHostForMx(mx) {
  const h = String(mx || '').toLowerCase().replace(/\.$/, '');
  for (const rule of MX_HOSTS) if (rule.match.test(h)) return { ...rule, mx: h };
  return null;
}

/* Hosts to try for an unknown domain, in the order they are conventionally
   used. Probing beats making someone find their own IMAP hostname. Kept
   synchronous and DNS-free: discoverHosts() below uses it as the last resort,
   and the sync path uses it to reconstruct a host for a row written before
   `extra.host` existed. */
export function candidateHosts(email, override) {
  if (override) return [String(override).trim().toLowerCase()];
  const d = domainOf(email);
  const known = MAIL_HOSTS[d];
  if (known) return [known.host];
  return [`imap.${d}`, `mail.${d}`, d];
}

/* What the domain's own DNS says about where its IMAP is. RFC 6186: a domain
   may publish _imaps._tcp pointing at host and port. Few do, but the ones that
   do are telling us the answer directly, so it is asked first. */
async function fromSrv(domain) {
  let recs;
  try { recs = await resolveSrv(`_imaps._tcp.${domain}`); } catch { return null; }
  const best = (recs || [])
    .filter(r => r.name && r.name !== '.')              // '.' means "explicitly none"
    .sort((a, b) => a.priority - b.priority || b.weight - a.weight)[0];
  if (!best) return null;
  return { host: String(best.name).toLowerCase().replace(/\.$/, ''), port: best.port || PORT, source: 'srv' };
}

/* Every server worth trying for an address, best first.

   Returns candidates, or a single `refuse` when the domain's DNS says there is
   nothing to connect to — a Proton-hosted domain, or one that only forwards.
   Saying so beats three timeouts and a box asking for a server name. */
export async function discoverHosts(email, override) {
  const domain = domainOf(email);
  if (override) {
    return { hosts: [{ host: String(override).trim().toLowerCase(), port: PORT, label: domain, source: 'override' }] };
  }
  if (NO_IMAP[domain]) return { refuse: describe(email).why };

  const own = isRatamail(email) ? ratamailHost() : null;
  if (own) return { hosts: [{ ...own, port: PORT, source: 'rata' }] };

  const known = MAIL_HOSTS[domain];
  if (known) return { hosts: [{ host: known.host, port: PORT, label: known.label, help: known.help, source: 'table' }] };

  const hosts = [];
  const srv = await fromSrv(domain);
  if (srv) hosts.push({ ...srv, label: domain });

  let mx = [];
  try { mx = await resolveMx(domain); } catch { /* no MX, or no such domain */ }
  mx.sort((a, b) => a.priority - b.priority);

  let filtered = null;
  for (const r of mx) {
    const hit = mailHostForMx(r.exchange);
    if (!hit) continue;
    if (hit.refuse) return { refuse: hit.refuse };
    if (hit.filtered) { filtered = hit.mx; continue; }
    if (!hosts.some(h => h.host === hit.host)) {
      hosts.push({ host: hit.host, port: PORT, label: hit.label, help: hit.help, source: 'mx' });
    }
  }

  /* A domain that runs its own server: the conventional names, tried last so a
     stale imap.<domain> cannot outrank what the MX actually says. */
  for (const h of candidateHosts(email)) {
    if (!hosts.some(x => x.host === h)) hosts.push({ host: h, port: PORT, label: domain, source: 'guess' });
  }

  return { hosts, filtered, mx: mx.map(r => r.exchange) };
}

/* ---------------------------------------------------------------------------
   Where RATA is willing to connect.

   The mail server is chosen by the user: either guessed from their domain, or
   typed into the "server address" box for a host we cannot guess. Both mean an
   address supplied from outside decides where the server opens a socket — and
   a server that will connect anywhere its users name is a probe with a public
   front door. `host: '127.0.0.1'`, or a domain whose A record points at
   169.254.169.254, turns a mailbox form into a way to reach whatever else runs
   on this machine or in this network and to learn, from which error comes
   back, what is listening there.

   So every hostname is resolved first, and refused if any address it answers
   with is loopback, link-local, private, carrier-grade NAT or otherwise not on
   the public internet. A mail server the whole world has to reach is public by
   definition, so nothing legitimate is lost.

   This resolves and then connects, which is two lookups and therefore not proof
   against a record that changes in between. It closes the practical hole — a
   private address handed straight over — rather than pretending to be more.
   --------------------------------------------------------------------------- */

const PRIVATE_NAME = /^(localhost|.*\.localhost|.*\.local|.*\.internal|.*\.home\.arpa)$/i;

function v4Private(ip) {
  const p = String(ip).split('.').map(Number);
  if (p.length !== 4 || p.some(n => !Number.isInteger(n) || n < 0 || n > 255)) return true;
  const [a, b] = p;
  if (a === 0 || a === 127 || a >= 224) return true;          // this host, loopback, multicast, reserved
  if (a === 10) return true;                                   // private
  if (a === 172 && b >= 16 && b <= 31) return true;            // private
  if (a === 192 && b === 168) return true;                     // private
  if (a === 192 && b === 0) return true;                       // IETF protocol assignments
  if (a === 169 && b === 254) return true;                     // link-local, incl. cloud metadata
  if (a === 100 && b >= 64 && b <= 127) return true;           // carrier-grade NAT
  if (a === 198 && (b === 18 || b === 19)) return true;         // benchmarking
  return false;
}

/* IPv6, expanded to its eight groups before anything is decided about it.

   Pattern-matching the text does not work, and the way it fails is silent.
   ::ffff:127.0.0.1 and ::ffff:7f00:1 are the same address; so is
   0:0:0:0:0:ffff:127.0.0.1. A check that only recognises the dotted spelling
   blocks one and waves the other two through, and a dual-stack host connects
   an IPv4-mapped address straight to the IPv4 one. Canonicalise, then judge. */
function v6Groups(ip) {
  let s = String(ip).toLowerCase().split('%')[0].replace(/^\[|\]$/g, '');
  if (!s) return null;

  /* A trailing dotted quad is two groups written the other way round. Rewrite
     it as hex so the rest of this deals with one notation. */
  const dq = s.match(/(\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3})$/);
  if (dq) {
    const p = dq[1].split('.').map(Number);
    if (p.some(n => !Number.isInteger(n) || n < 0 || n > 255)) return null;
    s = s.slice(0, s.length - dq[1].length) +
        (((p[0] << 8) | p[1]).toString(16) + ':' + ((p[2] << 8) | p[3]).toString(16));
  }

  const parts = s.split('::');
  if (parts.length > 2) return null;
  const head = parts[0] ? parts[0].split(':') : [];
  const tail = parts.length === 2 ? (parts[1] ? parts[1].split(':') : []) : [];
  if (parts.length === 1 ? head.length !== 8 : head.length + tail.length > 7) return null;

  const fill = new Array(8 - head.length - tail.length).fill('0');
  const groups = head.concat(fill, tail).map(h => (/^[0-9a-f]{1,4}$/.test(h) ? parseInt(h, 16) : NaN));
  return groups.some(n => !Number.isInteger(n)) ? null : groups;
}

function v6Private(ip) {
  const g = v6Groups(ip);
  if (!g) return true;                                   // unparseable: refuse
  const zeroTo = n => g.slice(0, n).every(x => x === 0);
  const embedded = (a, b) => v4Private([a >> 8, a & 255, b >> 8, b & 255].join('.'));

  if (zeroTo(7) && (g[7] === 0 || g[7] === 1)) return true;      // :: and ::1
  if (zeroTo(5) && g[5] === 0xffff) return embedded(g[6], g[7]); // ::ffff:a.b.c.d
  if (zeroTo(6)) return embedded(g[6], g[7]);                     // ::a.b.c.d (deprecated)
  if (g[0] === 0x64 && g[1] === 0xff9b) return true;              // 64:ff9b::/96 NAT64
  if (g[0] === 0x2002) return embedded(g[1], g[2]);               // 2002::/16 6to4
  if ((g[0] & 0xfe00) === 0xfc00) return true;                    // fc00::/7 unique local
  if ((g[0] & 0xffc0) === 0xfe80) return true;                    // fe80::/10 link local
  if ((g[0] & 0xff00) === 0xff00) return true;                    // ff00::/8 multicast
  return false;
}

const NOT_PUBLIC = h => `${h} is not a public mail server — it resolves inside a private network, and RATA will not connect there.`;

/* { ok: true } to go ahead; { notFound: true } to try the next candidate;
   anything else is a refusal to show the user. */
export async function checkHost(host) {
  const h = String(host || '').trim().toLowerCase().replace(/\.$/, '');
  if (!h || h.length > 253 || !/^[a-z0-9.:\[\]-]+$/.test(h)) {
    return { ok: false, error: `"${host}" is not a valid mail server address.` };
  }
  if (PRIVATE_NAME.test(h)) return { ok: false, error: NOT_PUBLIC(h) };

  const bare = h.replace(/^\[|\]$/g, '');
  const literal = isIP(bare);
  if (literal) {
    const bad = literal === 4 ? v4Private(bare) : v6Private(bare);
    return bad ? { ok: false, error: NOT_PUBLIC(h) } : { ok: true, host: h };
  }

  let addrs;
  try { addrs = await lookup(h, { all: true }); }
  catch { return { ok: false, notFound: true, error: `No mail server answers at ${h}.` }; }
  if (!addrs.length) return { ok: false, notFound: true, error: `No mail server answers at ${h}.` };
  for (const a of addrs) {
    if (a.family === 4 ? v4Private(a.address) : v6Private(a.address)) {
      return { ok: false, error: NOT_PUBLIC(h) };
    }
  }
  return { ok: true, host: h };
}

/* "The server said no" and "there is no server" need different answers: the
   first means the password is wrong and trying again would only push the
   account closer to being locked; the second is worth retrying. */
const AUTH_REFUSED = /auth|credential|login|password|AUTHENTICATIONFAILED|\b535\b|\b534\b/i;
export function isAuthFailure(e) {
  return AUTH_REFUSED.test(String(e?.message || e || ''));
}

function client(host, email, pass, port = PORT) {
  return new ImapFlow({
    host, port: port || PORT, secure: true,
    auth: { user: email, pass },
    logger: false,
    /* A wrong hostname should fail in seconds, not hang the request while we
       try three of them. */
    socketTimeout: 20000,
    greetingTimeout: 10000,
    connectionTimeout: 10000,
  });
}

/* Prove the credentials work — and find the server while we are at it — before
   anything is written. A typo then surfaces as a sign-in error rather than an
   inbox that silently never syncs. */
export async function verifyMail(email, pass, override) {
  const info = describe(email);
  if (info?.unsupported) return { ok: false, error: info.why };

  const found = await discoverHosts(email, override);
  /* The domain's DNS already answered the question, and the answer was "there
     is no mailbox here". Trying anyway would spend three timeouts to arrive at
     a worse version of the same sentence. */
  if (found.refuse) return { ok: false, error: found.refuse };

  let refused = null;
  let blocked = null;
  const tried = [];

  for (const cand of found.hosts) {
    /* Checked before the socket, not after: the point is not to open it. An MX
       or SRV record is published by the address's own domain, so this is a
       hostname an outsider chooses — the same reason the override is checked. */
    const allowed = await checkHost(cand.host);
    if (!allowed.ok) {
      if (!allowed.notFound) { blocked = allowed.error; break; }
      continue;
    }
    tried.push(cand.host);

    const c = client(cand.host, email, pass, cand.port);
    try {
      await c.connect();
      await c.logout();
      return {
        ok: true,
        host: cand.host,
        port: cand.port || PORT,
        label: cand.label || info?.label || cand.host,
        help: cand.help || info?.help || null,
        /* How it was found, so the app can say "that is Google Workspace"
           rather than showing a hostname nobody recognises. */
        source: cand.source,
      };
    } catch (e) {
      try { await c.logout(); } catch {}
      /* Distinguish "no such server" from "server said no": the first means
         keep looking, the second means the password is wrong and trying other
         hostnames would only lock the account faster. */
      if (isAuthFailure(e)) {
        refused = cand;
        break;
      }
    }
  }

  if (blocked) return { ok: false, error: blocked };

  if (refused) {
    const help = refused.help || info?.help;
    return { ok: false, error: `${refused.label || 'The mail server'} rejected the sign-in. Use an app password, not your normal account password${help ? ` — ${help}` : ''}.` };
  }

  if (override) {
    return { ok: false, error: `Could not reach ${override}. Check the server address with your mail provider.` };
  }

  /* A filter in front of the mailbox is the one failure where the domain's DNS
     tells us something useful about why. */
  if (found.filtered) {
    return { ok: false, needsHost: true, error:
      `${domainOf(email)} filters its mail through ${found.filtered}, which does not say where the mailbox itself is. Enter the IMAP server address below — your IT administrator will know it.` };
  }

  return { ok: false, needsHost: true, error:
    `RATA checked ${domainOf(email)}’s DNS and tried ${tried.length ? tried.join(', ') : 'the usual server names'} without finding a mailbox. Enter the IMAP server address below — your provider or IT administrator lists it as "IMAP server" or "incoming mail server".` };
}

const strip = s => String(s || '').replace(/<[^>]+>/g, ' ').replace(/\s+/g, ' ').trim();

export async function fetchInbox(acct, { limit = 15 } = {}) {
  const { host, email, pass, label, port } = acct;

  const allowed = await checkHost(host);
  if (!allowed.ok) return { kind: 'host', error: allowed.error || `Will not connect to ${host}.` };

  const c = client(host, email, pass, port);
  const messages = [];
  try {
    await c.connect();
    const lock = await c.getMailboxLock('INBOX');
    try {
      const total = c.mailbox.exists || 0;
      if (total > 0) {
        const from = Math.max(1, total - (limit - 1));
        for await (const msg of c.fetch(`${from}:*`, { envelope: true, flags: true, uid: true })) {
          const env = msg.envelope || {};
          const sender = (env.from && env.from[0]) || {};
          let preview = '';
          try {
            const dl = await c.download(msg.uid, 'TEXT', { uid: true, maxBytes: 800 });
            if (dl && dl.content) {
              const chunks = [];
              for await (const chunk of dl.content) {
                chunks.push(chunk);
                if (Buffer.concat(chunks).length > 800) break;
              }
              preview = strip(Buffer.concat(chunks).toString('utf8')).slice(0, 200);
            }
          } catch { /* preview is best-effort; the envelope still lands */ }
          messages.push({
            /* The account is part of the id: the same message uid in two
               different mailboxes must not collide in a combined inbox. */
            id: `${mailKey(email)}_${msg.uid}`,
            acct: email,
            acctLabel: label || email,
            ch: 'email',
            prov: 'imap',
            fromName: sender.name || sender.address || 'Unknown',
            fromAddr: (sender.address || '').toLowerCase(),
            subj: env.subject || '(no subject)',
            prev: (preview || '').slice(0, 120),
            body: (preview || '(preview unavailable)') + `\n\n— Synced from ${email}.`,
            ts: env.date ? new Date(env.date).getTime() : Date.now(),
            unread: !(msg.flags && msg.flags.has('\\Seen')),
            starred: !!(msg.flags && msg.flags.has('\\Flagged')),
          });
        }
      }
    } finally { lock.release(); }
    await c.logout();
  } catch (e) {
    try { await c.logout(); } catch {}
    /* An app password that was revoked will be revoked on the next refresh
       too, and repeating a rejected sign-in is how a provider decides to lock
       the account. The caller uses `kind` to stop retrying that one. */
    if (isAuthFailure(e)) {
      return { kind: 'auth', error: `${email} rejected the sign-in — the app password has probably been revoked. Relink it in Accounts.` };
    }
    return { kind: 'net', error: `${email} did not sync — ${host} could not be reached. It will be tried again on the next refresh.` };
  }
  messages.sort((a, b) => b.ts - a.ts);
  return { messages };
}
