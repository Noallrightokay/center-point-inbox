/* Linking any mailbox, and the plan limits that decide how many.

   The network is not available to these tests, so what is exercised here is
   everything that decides the answer before a socket is opened: which server
   an address resolves to, which allowance a link draws from, and what the user
   is told when the allowance runs out. The IMAP conversation itself is proved
   against a real server by the live checks. */
import { startServer, makeChecker } from './helpers.mjs';
import { describe, candidateHosts, mailKey, isMailKey, domainOf } from '../lib/mail.js';
import { PLANS, UNLIMITED, bucketOf, countLinks, refusal } from '../lib/plan.js';

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

  console.log('\n— one row per mailbox, inside the existing primary key —');
  {
    const a = mailKey('owner@example.com'), b = mailKey('owner2@example.com');
    check(a !== b, `two addresses, two keys: ${a} / ${b}`);
    check(mailKey('Owner@Example.com') === a, 'the same address always lands on the same key');
    check(isMailKey(a) && !isMailKey('slack'), 'mail keys are distinguishable from the OAuth providers');
    check(/^mail:[a-z0-9_]+$/.test(a), `and stay a single readable token: ${a}`);
  }

  console.log('\n— what each plan includes —');
  {
    check(PLANS.base.price === 8 && PLANS.pro.price === 16 && PLANS.enterprise.price === 72,
      `the ladder: $${PLANS.base.price} / $${PLANS.pro.price} / $${PLANS.enterprise.price}`);
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
    check(/RATA Pro/.test(stop) && /\$16/.test(stop),
      `and names the plan that lifts it, with its price: "${stop}"`);
    check(/up to 2/.test(stop), 'Base is up to two mailboxes, stated as a ceiling not a quota');

    const none0 = refusal(null, 'mail', 0);
    check(/Center Point inbox/.test(none0),
      'and the pitch names what the mailboxes arrive in');

    check(refusal('pro', 'mail', 500) === null, 'Pro does not run out of mailboxes');
    check(refusal('enterprise', 'chat', 99) === null, 'nor Enterprise of chat workspaces');

    const none = refusal(null, 'mail', 0);
    check(/Choose a plan/.test(none) && /\$8/.test(none),
      `an account with no plan is told the price, not handed a free tier: "${none}"`);

    const noChat = refusal('base', 'chat', 0);
    check(/does not include/.test(noChat) && /RATA Pro/.test(noChat), `Base, chat: "${noChat}"`);
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
