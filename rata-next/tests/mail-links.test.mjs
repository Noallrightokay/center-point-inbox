/* Linking any mailbox, and the plan limits that decide how many.

   The network is not available to these tests, so what is exercised here is
   everything that decides the answer before a socket is opened: which server
   an address resolves to, which allowance a link draws from, and what the user
   is told when the allowance runs out. The IMAP conversation itself is proved
   against a real server by the live checks. */
import { startServer, makeChecker } from './helpers.mjs';
import { describe, candidateHosts, mailKey, isMailKey, domainOf } from '../lib/mail.js';
import { PLANS, bucketOf, countLinks, refusal } from '../lib/plan.js';

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

  console.log('\n— what a plan includes —');
  {
    check(PLANS.base.mail === 2 && PLANS.base.chat === 1, 'Base: 2 mailboxes, 1 chat workspace');
    check(PLANS.business.files === true && PLANS.base.files === false, 'the file library is Business only');

    check(bucketOf(mailKey('a@b.com')) === 'mail', 'a new mailbox draws on the mail allowance');
    check(bucketOf('gmail_imap') === 'mail' && bucketOf('apple') === 'mail',
      'so do mailboxes linked before this change — upgrading must not drop them');
    check(bucketOf('slack') === 'chat', 'Slack draws on the chat allowance');

    const rows = [{ provider: mailKey('a@b.com') }, { provider: 'apple' }, { provider: 'slack' }];
    const used = countLinks(rows);
    check(used.mail === 2 && used.chat === 1, `counted separately: ${used.mail} mail, ${used.chat} chat`);
  }

  console.log('\n— what the user is told at the limit —');
  {
    check(refusal('base', 'mail', 1) === null, 'one mailbox on Base: the second is allowed');
    const stop = refusal('base', 'mail', 2);
    check(!!stop, 'the third is refused');
    check(/RATA Business/.test(stop) && /\d/.test(stop), `and the refusal names the way out: "${stop}"`);

    const noChat = refusal('free', 'chat', 0);
    check(/does not include/.test(noChat), `free preview, chat: "${noChat}"`);
    check(refusal('business', 'mail', 2) === null, 'Business has room for more');
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
