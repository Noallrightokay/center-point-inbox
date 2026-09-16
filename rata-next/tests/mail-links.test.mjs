/* Linking any mailbox, and the plan limits that decide how many.

   The network is not available to these tests, so what is exercised here is
   everything that decides the answer before a socket is opened: which server
   an address resolves to, which allowance a link draws from, and what the user
   is told when the allowance runs out. The IMAP conversation itself is proved
   against a real server by the live checks. */
import { startServer, makeChecker } from './helpers.mjs';
import { describe, candidateHosts, discoverHosts, mailHostForMx, mailKey, isMailKey, domainOf, checkHost, isAuthFailure, smtpCandidates, isRatamail, ratamailDomain } from '../lib/mail.js';
import { allowed, failed, succeeded, reset, LINK_ATTEMPTS, waitPhrase } from '../lib/ratelimit.js';
import { PLANS, UNLIMITED, SELLABLE, bucketOf, countLinks, refusal, domainRefusal, domainsAllowed, money, DOMAIN_ADDON } from '../lib/plan.js';

export default async function run(state) {
  const check = makeChecker(state);

  console.log('\n— finding the mail server from the address alone —');
  {
    for (const [addr, host] of [
      ['someone@gmail.com', 'imap.gmail.com'],
      ['someone@icloud.com', 'imap.mail.me.com'],
      ['someone@outlook.com', 'outlook.office365.com'],
      ['someone@yahoo.com', 'imap.mail.yahoo.com'],
      ['someone@fastmail.com', 'imap.fastmail.com'],
      ['bookkeeper@sympatico.ca', 'imap.bell.net'],
    ]) {
      check(describe(addr).host === host, `${addr} -> ${describe(addr).host}`);
    }

    const unknown = describe('owner@thesherwood.group');
    check(unknown.guessed === true, 'an unknown domain is marked as a guess, not a fact');
    check(unknown.host === 'imap.thesherwood.group', `guessed host: ${unknown.host}`);
    check(candidateHosts('owner@thesherwood.group').join(', ') === 'imap.thesherwood.group, mail.thesherwood.group, thesherwood.group',
      'and three conventional names get tried before giving up');

    const proton = describe('someone@proton.me');
    check(proton.unsupported === true, 'Proton is refused up front rather than timing out');
    check(/no IMAP server/i.test(proton.why), `and says why: "${proton.why.slice(0, 58)}…"`);

    check(domainOf('Mixed.Case@Example.COM') === 'example.com', 'the address is matched case-insensitively');
  }

  console.log('\n— a business address on its own domain —');
  {
    /* The case RATA exists for, and the one guessing gets wrong. Most companies
       do not run a mail server; they point their domain at one. There is no
       imap.<their domain> to find — the MX record is where the answer is. */
    for (const [mx, host, label] of [
      ['aspmx.l.google.com', 'imap.gmail.com', 'Google Workspace'],
      ['alt2.aspmx.l.google.com', 'imap.gmail.com', 'Google Workspace'],
      ['acme-com.mail.protection.outlook.com', 'outlook.office365.com', 'Microsoft 365'],
      ['mx.zoho.eu', 'imap.zoho.com', 'Zoho Mail'],
      ['in1-smtp.messagingengine.com', 'imap.fastmail.com', 'Fastmail'],
      ['mx1.titan.email', 'imap.titan.email', 'Titan'],
      ['mx.emailsrvr.com', 'secure.emailsrvr.com', 'Rackspace Email'],
      ['mx1.registrar-servers.com', 'mail.privateemail.com', 'Namecheap Private Email'],
    ]) {
      const hit = mailHostForMx(mx);
      check(hit && hit.host === host && hit.label === label, `MX ${mx} -> ${label} (${hit && hit.host})`);
      check(!!(hit && hit.help), 'and comes with where that provider keeps its app passwords');
    }

    /* Two answers that are not a hostname, because connecting would be wrong. */
    check(mailHostForMx('mail.protonmail.ch').refuse, 'a domain hosted at Proton is refused with the reason, not probed');
    check(/forwards its mail/i.test(mailHostForMx('mx1.improvmx.com').refuse || ''),
      'a forwarding-only domain is told to link the address the mail actually lands in');
    check(mailHostForMx('mx0a-000abc01.pphosted.com').filtered === true,
      'and a spam filter in front of the mailbox is not mistaken for the mailbox');

    check(mailHostForMx('mail.somecompany.example') === null,
      'an MX nobody recognises resolves to nothing, so the conventional names still get their turn');

    /* Order matters: what the domain says beats what we would have guessed. */
    const found = await discoverHosts('someone@gmail.com');
    check(found.hosts[0].host === 'imap.gmail.com' && found.hosts[0].source === 'table',
      'a consumer address is answered from the table without a lookup');

    const nowhere = await discoverHosts('someone@nx-' + Date.now() + '.invalid');
    check(nowhere.hosts.every(h => h.source === 'guess'),
      'a domain with no DNS at all falls back to the conventional names rather than failing');

    const forced = await discoverHosts('someone@example.com', 'mail.example.org');
    check(forced.hosts.length === 1 && forced.hosts[0].source === 'override',
      'and a server the user typed in is the only one tried');
  }

  console.log('\n— one row per mailbox, inside the existing primary key —');
  {
    const a = mailKey('owner@example.com'), b = mailKey('owner2@example.com');
    check(a !== b, `two addresses, two keys: ${a} / ${b}`);
    check(mailKey('Owner@Example.com') === a, 'the same address always lands on the same key');
    check(isMailKey(a) && !isMailKey('slack'), 'mail keys are distinguishable from the OAuth providers');
    check(/^mail:[a-z0-9_]+\.[0-9a-f]{10}$/.test(a), `and stay a single readable token: ${a}`);

    /* The slug alone collides, and (user_id, provider) is the primary key — so
       a collision is not a cosmetic clash, it is an upsert straight over
       somebody's other mailbox. */
    check(mailKey('a.b@x.com') !== mailKey('a-b@x.com'),
      'addresses that differ only in punctuation get different keys');
    check(mailKey('a.b@x.com') !== mailKey('ab@x.com'),
      'and so do ones that differ by a character the slug drops');

    const long = 'x'.repeat(200) + '@example.com';
    check(mailKey(long) !== mailKey('x'.repeat(201) + '@example.com'),
      'a long address is not truncated onto its neighbour');

    const seen = new Set();
    for (const addr of ['a.b@x.com', 'a-b@x.com', 'a+b@x.com', 'a_b@x.com', 'ab@x.com', 'A.B@X.com'])
      seen.add(mailKey(addr));
    check(seen.size === 5, `six addresses, five distinct mailboxes (the sixth is the same one in capitals): ${seen.size}`);
  }

  console.log('\n— mail RATA hosts itself —');
  {
    /* The server does not exist yet. Until it does, an @mailrata.org address
       must behave like anyone else's unknown domain — pointing users at a host
       that is not answering and calling it a mail problem is worse than not
       offering it. */
    delete process.env.RATA_MAIL_HOST;
    check(ratamailDomain() === null, 'with no server configured, RATA hosts no mail');
    check(!isRatamail('me@mailrata.org'), 'and its own domain gets no special treatment');
    check(describe('me@mailrata.org').guessed === true,
      'an @mailrata.org address falls through to ordinary discovery, which will ask');

    process.env.RATA_MAIL_HOST = 'imap.mailrata.org';
    try {
      check(isRatamail('me@mailrata.org') && !isRatamail('me@gmail.com'),
        'configured, the domain is recognised as RATA\u2019s own');
      const d = describe('me@mailrata.org');
      check(d.hostedByRata === true && d.host === 'imap.mailrata.org' && !d.guessed,
        `and resolves without a lookup: ${d.host} (${d.label})`);
      check(/separate from your RATA account password/i.test(d.help),
        'saying plainly that the mailbox password is not the account password');

      const found = await discoverHosts('me@mailrata.org');
      check(found.hosts.length === 1 && found.hosts[0].source === 'rata',
        'discovery stops there rather than asking DNS about our own domain');

      check(smtpCandidates(null, 'me@mailrata.org')[0] === 'imap.mailrata.org',
        'and sending goes to RATA\u2019s own submission host');
      process.env.RATA_SMTP_HOST = 'smtp.mailrata.org';
      check(smtpCandidates(null, 'me@mailrata.org')[0] === 'smtp.mailrata.org',
        'which a deployment can name separately');

      /* Everyone else must be untouched by any of this. */
      check(describe('someone@gmail.com').host === 'imap.gmail.com',
        'no other provider changes behaviour when RATA hosts mail');
    } finally {
      delete process.env.RATA_MAIL_HOST;
      delete process.env.RATA_SMTP_HOST;
    }
  }

  console.log('\n— where RATA is willing to open a socket —');
  {
    /* The server address is supplied by the user. Without this, a mailbox form
       is a way to reach whatever else is on this machine or this network, and
       to learn what is there from which error comes back. */
    for (const host of ['127.0.0.1', 'localhost', '10.0.0.5', '192.168.1.1', '169.254.169.254',
                        '172.16.4.4', '100.64.0.1', '[::1]', 'db.internal', 'nas.local']) {
      const r = await checkHost(host);
      check(!r.ok && !r.notFound, `${host} — refused${r.error ? '' : ' (no reason given!)'}`);
    }
    check(/private network/i.test((await checkHost('169.254.169.254')).error),
      'and the refusal says why rather than looking like a lookup failure');

    check((await checkHost('8.8.8.8')).ok, 'a public address is allowed');
    check((await checkHost('imap.gmail.com')).ok, 'and so is a real mail server');

    const bad = await checkHost('not a host name');
    check(!bad.ok, 'a hostname that is not one is refused before any lookup');

    /* Distinguishing the two matters to the caller: "nothing there" means try
       the next candidate name, "refused" means stop. */
    const missing = await checkHost('imap.nx-' + Date.now() + '.invalid');
    check(missing.notFound === true, 'a name that resolves to nothing is a miss, not a refusal');
  }

  console.log('\n— a rejected sign-in is told apart from an unreachable server —');
  {
    check(isAuthFailure(new Error('Invalid credentials (Failure)')), 'AUTHENTICATIONFAILED: the password is wrong');
    check(isAuthFailure(new Error('535 5.7.8 Username and Password not accepted')), 'so is an SMTP 535');
    check(!isAuthFailure(new Error('getaddrinfo ENOTFOUND imap.example.com')), 'a name that does not resolve is not');
    check(!isAuthFailure(new Error('Socket timeout')), 'nor is a timeout — retrying that one is fine');
  }

  console.log('\n— failed sign-ins get slower —');
  {
    reset();
    const k = 'link:test-user';
    check(allowed(k, LINK_ATTEMPTS).ok, 'the first attempt is allowed');
    for (let i = 0; i < LINK_ATTEMPTS.limit; i++) failed(k, LINK_ATTEMPTS);
    const stop = allowed(k, LINK_ATTEMPTS);
    check(!stop.ok, `after ${LINK_ATTEMPTS.limit} failures the next is refused`);
    check(stop.retryAfterMs > 0 && /minute/.test(waitPhrase(stop.retryAfterMs)),
      `and the wait is expressed in something actionable: "${waitPhrase(stop.retryAfterMs)}"`);

    /* Otherwise somebody linking six mailboxes in a row would meet this. */
    reset();
    for (let i = 0; i < 20; i++) succeeded('link:happy-user');
    check(allowed('link:happy-user', LINK_ATTEMPTS).ok, 'successes are not counted at all');

    reset();
    const u = 'link:someone';
    failed(u, LINK_ATTEMPTS); failed(u, LINK_ATTEMPTS);
    succeeded(u);
    check(allowed(u, LINK_ATTEMPTS).remaining === LINK_ATTEMPTS.limit,
      'and getting it right clears what came before, so a typo is not held against you');

    reset();
    const short = { limit: 1, windowMs: 40 };
    failed('link:brief', short);
    check(!allowed('link:brief', short).ok, 'the window holds while it is open');
    await new Promise(r => setTimeout(r, 60));
    check(allowed('link:brief', short).ok, 'and lets go when it closes');
    reset();
  }

  console.log('\n— what each plan includes —');
  {
    check(PLANS.base.price === 12.99 && PLANS.pro.price === 23.99 && PLANS.enterprise.price === 72,
      `the ladder: ${money(PLANS.base.price)} / ${money(PLANS.pro.price)} / ${money(PLANS.enterprise.price)}`);
    check(!('free' in PLANS), 'there is no free tier to fall back to');

    check(PLANS.base.mail === 2, 'Base: two mailboxes');
    check(PLANS.base.split === false, 'and one inbox — side by side is not part of it');
    check(PLANS.base.translate && PLANS.base.convert,
      'but translation and the Format Bridge are, because that is what Base is for');

    check(PLANS.pro.mail === UNLIMITED, 'Pro: as many mailboxes as you have');
    check(PLANS.pro.split && PLANS.pro.ai, 'with side by side and summaries');
    check(!PLANS.pro.crm && !PLANS.pro.sms && !PLANS.pro.automations,
      'and none of the team features');

    check(PLANS.enterprise.crm && PLANS.enterprise.sms && PLANS.enterprise.automations,
      'Enterprise: the CRM, texts and automations');

    /* Each plan has to contain the one below it, or upgrading could take
       something away. */
    const strictly = ['split', 'translate', 'convert', 'ai', 'files', 'crm', 'sms', 'automations']
      .every(f => !PLANS.base[f] || PLANS.pro[f]) &&
      ['split', 'translate', 'convert', 'ai', 'files', 'crm', 'sms', 'automations']
      .every(f => !PLANS.pro[f] || PLANS.enterprise[f]);
    check(strictly, 'each plan contains the one below it — upgrading never takes anything away');

    check(bucketOf(mailKey('a@b.com')) === 'mail', 'a new mailbox draws on the mail allowance');
    check(bucketOf('gmail_imap') === 'mail' && bucketOf('apple') === 'mail',
      'so do mailboxes linked before this change — changing plan must not drop them');
    const used = countLinks([{ provider: mailKey('a@b.com') }, { provider: 'apple' }, { provider: 'slack' }]);
    check(used.mail === 2 && used.chat === 1, `counted separately: ${used.mail} mail, ${used.chat} chat`);
  }

  console.log('\n— what the user is told at the limit —');
  {
    check(refusal('base', 'mail', 1) === null, 'one mailbox on Base: the second is allowed');
    const stop = refusal('base', 'mail', 2);
    check(!!stop, 'the third is refused');
    check(/RATA Pro/.test(stop) && /\$23\.99/.test(stop),
      `and names the plan that lifts it, with its price: "${stop}"`);
    check(/up to 2/.test(stop), 'Base is up to two mailboxes, stated as a ceiling not a quota');

    const none0 = refusal(null, 'mail', 0);
    check(/Center Point inbox/.test(none0),
      'and the pitch names what the mailboxes arrive in');

    check(refusal('pro', 'mail', 500) === null, 'Pro does not run out of mailboxes');
    check(refusal('enterprise', 'chat', 99) === null, 'nor Enterprise of chat workspaces');

    const none = refusal(null, 'mail', 0);
    check(/Choose a plan/.test(none) && /\$12\.99/.test(none),
      `an account with no plan is told the price, not handed a free tier: "${none}"`);

    const noChat = refusal('base', 'chat', 0);
    check(/does not include/.test(noChat) && /RATA Pro/.test(noChat), `Base, chat: "${noChat}"`);
  }

  console.log('\n— RATA\u2019s own addresses, and mail on a domain you own —');
  {
    check(PLANS.base.ratamail === 0, 'Base connects mailboxes you already have; it does not hand out new ones');
    check(PLANS.pro.ratamail === UNLIMITED && PLANS.enterprise.ratamail === UNLIMITED,
      'Pro and Enterprise get addresses at mailrata.org');

    check(PLANS.pro.domains === 0, 'Pro includes no custom domain');
    check(PLANS.enterprise.domains === UNLIMITED, 'Enterprise includes as many as they like');
    check(DOMAIN_ADDON.price === 1.5, `and Pro buys them at ${money(DOMAIN_ADDON.price)} a month each`);

    check(domainsAllowed('pro', 0) === 0 && domainsAllowed('pro', 3) === 3,
      'what is bought is what is allowed');
    check(domainsAllowed('enterprise', 0) === UNLIMITED, 'Enterprise ignores the counter entirely');

    /* The point of a separate refusal: every other limit is lifted by moving
       up a plan, and telling a Pro member to pay $56 more for one domain
       would be both wrong and a good way to lose them. */
    const ask = domainRefusal('pro', 0, 0);
    check(/\$1\.50/.test(ask), `Pro is offered the add-on, with its price: "${ask}"`);
    /* It used to name Enterprise as the alternative. It must not while
       Enterprise cannot be bought — the add-on is the whole answer now. */
    check(!/Enterprise/.test(ask),
      'and does not send them to a plan that is not on sale');
    check(domainRefusal('pro', 1, 1) === null ? false : /another/.test(domainRefusal('pro', 1, 1)),
      'a second domain on top of one bought is offered as another add-on');
    check(domainRefusal('pro', 0, 1) === null, 'and a domain already paid for is simply allowed');
    check(domainRefusal('enterprise', 400, 0) === null, 'Enterprise never meets this at all');
    check(/Choose a plan/.test(domainRefusal(null, 0, 0)), 'an account with no plan is told what to buy first');

    /* $1.50, never $1.5. */
    check(money(1.5) === '$1.50' && money(12.99) === '$12.99' && money(72) === '$72',
      `prices are quoted whole: ${money(12.99)} / ${money(1.5)} / ${money(72)}`);
    /* The one that actually bites with .99 pricing: a price ending in a zero
       renders as "$12.9" under raw interpolation. */
    check(money(12.9) === '$12.90' && money(23.5) === '$23.50',
      `a trailing zero survives: ${money(12.9)} / ${money(23.5)}`);
    check(!/\$\d+\.\d(?!\d)/.test(ask), 'and no refusal quotes a price with a digit missing');
  }

  console.log('\n— nothing offers a plan that cannot be bought —');
  {
    /* Enterprise's three headline features are flags nothing in the app reads.
       Until they exist, taking $72 a month for them is the problem — so it
       stays defined and stays out of everything that sells. */
    check(SELLABLE.join(',') === 'base,pro', `on sale: ${SELLABLE.join(', ')}`);
    check(PLANS.enterprise.sellable === false, 'Enterprise is defined but not for sale');
    check(!!PLANS.enterprise.crm, 'and still defined, so an account holding it resolves to a tier');

    /* The trap this closes: a limit whose only way out is a plan with no way
       to buy it. Better to state the limit and stop. */
    const chat = refusal('pro', 'chat', 3);
    check(!/Enterprise/.test(chat), `a Pro chat limit does not point at Enterprise: "${chat}"`);
    check(/RATA Pro includes up to 3/.test(chat), 'it just says what the limit is');

    const dom = domainRefusal('pro', 0, 0);
    check(!/Enterprise/.test(dom) && /\$1\.50/.test(dom),
      `nor does the domain add-on: "${dom}"`);

    /* Base -> Pro still works, or the ladder would have no rungs at all. */
    check(/RATA Pro/.test(refusal('base', 'mail', 2)), 'Base is still told about Pro');

    const { readFileSync } = await import('node:fs');
    const { fileURLToPath } = await import('node:url');
    const here = fileURLToPath(new URL('.', import.meta.url));
    const visible = readFileSync(here + '../public/index.html', 'utf8').replace(/<!--[\s\S]*?-->/g, '');
    check(!/RATA Enterprise/.test(visible), 'and the pricing page does not advertise it');
    check(/RATA Base/.test(visible) && /RATA Pro/.test(visible), 'while still selling the two that are real');
  }

  console.log('\n— the price on the website is the price in the product —');
  {
    /* These were three separate copies of the same numbers, and a price change
       that updates the plan but not the pricing page is a change the customer
       finds first. The product is the source; the page has to agree with it. */
    const { readFileSync } = await import('node:fs');
    const { fileURLToPath } = await import('node:url');
    const here = fileURLToPath(new URL('.', import.meta.url));
    const home = readFileSync(here + '../public/index.html', 'utf8');

    for (const k of SELLABLE) {
      const shown = money(PLANS[k].price);
      check(home.includes(shown), `the pricing page quotes ${PLANS[k].label} at ${shown}`);
    }
    for (const stale of ['$8<', '$16<', 'from $8/', 'Get Base — $8', 'Get Pro — $16']) {
      check(!home.includes(stale), `and no longer says "${stale}"`);
    }

    const app = readFileSync(here + '../public/app.html', 'utf8');
    check(/price:12\.99/.test(app) && /price:23\.99/.test(app),
      'and the client tier table carries the same numbers as lib/plan.js');
  }

  console.log('\n— the endpoints exist and refuse strangers —');
  {
    const s = await startServer();
    try {
      for (const [path, init] of [
        ['/api/links', {}],
        ['/api/sync/mail', {}],
        ['/api/link/mail', { method: 'POST', body: '{}' }],
      ]) {
        const r = await fetch(s.url + path, init);
        const body = await r.json().catch(() => null);
        check(!!body?.error, `${path} without a session -> ${JSON.stringify(body?.error)}`);
      }
    } finally { await s.stop(); }
  }
}
