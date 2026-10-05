/* RATA demo: the desktop app's Rust side, faked in the page.

   The interface and bridge.js are the real ones from the release. Only the
   seam below them, window.__TAURI__.core.invoke, is replaced, with sample
   mailboxes, mail and saved people. Every organisation and person here is
   invented, and no message holds patient information. Nothing is sent,
   nothing leaves this page, and no mailbox is opened. Reloading keeps what
   you did (in this browser only); "Reset demo" starts again. */
(function () {
  const DEMO_VERSION = '5';
  const UID = 'local_demo2';
  const ME = 'Riley Carter';
  const DAY = 86400000, HOUR = 3600000, MIN = 60000;
  const now = Date.now();

  const LARK = 'riley.carter@larkspurvalleyhealth.org';
  const SALT = 'rcarter@saltmarshregional.org';
  const HOME = 'riley.carter@gmail.com';
  const CPR = 'bookings@tidepoolcpr.com';
  const WRITE = 'riley@clearchartwriting.com';
  const MAILBOXES = [
    { email: LARK, host: 'outlook.office365.com', port: 993, label: 'Larkspur Valley Health' },
    { email: SALT, host: 'imap.saltmarshregional.org', port: 993, label: 'Saltmarsh Regional' },
    { email: HOME, host: 'imap.gmail.com', port: 993, label: 'Personal' },
    { email: CPR, host: 'imap.tidepoolcpr.com', port: 993, label: 'Tidepool CPR Training' },
    { email: WRITE, host: 'imap.clearchartwriting.com', port: 993, label: 'Clearchart Writing' },
  ];

  /* Which plan the demo shows, chosen in its label. Base holds two
     mailboxes (RATA refuses a third when linking), so Base shows the two
     hospital networks; Pro shows all five. */
  let PLAN = 'pro';
  try { PLAN = localStorage.getItem('rata_demo_plan') === 'base' ? 'base' : 'pro'; } catch (e) {}
  const PLANS = {
    base: { key: 'base', label: 'RATA Base', mail: 2, chat: 0, split: false, ai: false },
    pro: { key: 'pro', label: 'RATA Pro', mail: null, chat: 3, split: true, ai: true },
  };
  const ACTIVE = PLAN === 'base' ? MAILBOXES.slice(0, 2) : MAILBOXES;
  const active = (email) => ACTIVE.some((m) => m.email === email);

  /* People saved in People, so their mail threads under one name. */
  const CONTACTS = [
    { id: 'cd1', name: 'Dr. Anita Shah', nick: 'Anita', addr: 'anita.shah@larkspurvalleyhealth.org' },
    { id: 'cd2', name: 'Marcus Bell', nick: 'Marcus', addr: 'mbell@saltmarshregional.org' },
    { id: 'cd3', name: 'Medical Staff Office', nick: '', addr: 'medstaff@larkspurvalleyhealth.org' },
    { id: 'cd4', name: 'Elena Carter', nick: 'Mom', addr: 'elena.carter52@gmail.com' },
    { id: 'cd5', name: 'Jess Morgan', nick: 'Jess', addr: 'jess.morgan@gmail.com' },
    { id: 'cd6', name: 'Dana Whitfield', nick: 'Dana', addr: 'dana@littleoaksdaycare.com' },
    { id: 'cd7', name: 'Tom Becker', nick: 'Tom', addr: 'tom.becker@wellnessquarterly.com' },
    { id: 'cd9', name: 'Lena Ortiz', nick: 'Lena', addr: 'lena@ortizbookkeeping.com' },
    { id: 'cd10', name: 'Dr. Omar Haddad', nick: '', addr: 'ohaddad@saltmarshregional.org' },
  ];

  try {
    if (localStorage.getItem('rata_demo_v') !== DEMO_VERSION) {
      localStorage.clear();
      if (PLAN === 'base') localStorage.setItem('rata_demo_plan', 'base');
      try { indexedDB.deleteDatabase('rata-mail-local_demo'); indexedDB.deleteDatabase('rata-mail-' + UID); indexedDB.deleteDatabase('rata-files'); } catch (e) {}
      localStorage.setItem('rata_demo_v', DEMO_VERSION);
    }
    if (!localStorage.getItem('centra_session')) {
      localStorage.setItem('centra_session', JSON.stringify({ uid: UID, email: HOME, mode: 'local' }));
    }
    if (!localStorage.getItem('centra_ws_' + UID)) {
      localStorage.setItem('centra_ws_' + UID, JSON.stringify({
        v: 6, settings: { name: ME, profile: 'HIPAA', plan: null }, linked: [], rules: [],
        contacts: CONTACTS, messages: [], documents: [], folders: [],
        counters: { scans: 0, warned: 0, blocked: 0, conversions: 0 }, audit: [],
      }));
    }
    localStorage.setItem('rata_web_note_dismissed', '1');
  } catch (e) {}

  const label = (a) => (MAILBOXES.find((m) => m.email === a) || {}).label || a;
  const VALIDITY = 4242;

  /* One message in the shape refresh_mail answers with. UIDs are unique
     across the demo, so one number finds one message. */
  let uidNext = 900;
  function msg(o) {
    const acct = o.acct;
    const folder = o.folder || 'inbox';
    const uid = o.uid || uidNext++;
    const place = folder === 'inbox' ? '' : (typeof folder === 'object' ? 'f' + folder.named.toLowerCase() + '_' : folder + '_');
    const body = o.body.replace(/^\n/, '');
    const mine = folder === 'sent' || folder === 'drafts';
    return {
      id: acct + '_' + place + uid, folder, acct, acct_label: label(acct),
      from_name: mine ? ME : (o.fromName || ''), from_addr: mine ? acct : o.from,
      to_name: o.toName || '', to_addr: o.to || acct,
      to_all: o.toAll || [o.to || acct], cc: o.cc || [],
      subject: o.subject, preview: body.replace(/\s+/g, ' ').slice(0, 140), body,
      ts: now - o.ago, unread: !!o.unread, starred: !!o.starred,
      uid, uidvalidity: VALIDITY,
      message_id: o.mid || ('demo' + uid + '@' + acct.split('@')[1]),
      reply_to: o.replyTo || '', in_reply_to: o.inReplyTo || '', references_last: o.refs || o.inReplyTo || '',
      truncated: false, attachments: o.atts || [], html: !!o.html,
      ...(o.unsub ? { unsubscribe: o.unsub } : {}),
    };
  }

  const PIC_UID = 301;
  const MAIL = [
    /* Larkspur Valley Health */
    msg({ uid: 101, acct: LARK, from: 'anita.shah@larkspurvalleyhealth.org', fromName: 'Dr. Anita Shah', subject: 'November ED schedule is posted', ago: 25 * MIN, unread: true, starred: true,
      body: `
Hi all,

The November emergency department schedule is posted in the scheduling
portal. Riley, you have the 7p to 7a block on the 8th, 9th and 22nd.

Swap requests by Friday, please. After that they go through me.

Thanks,
Anita

Anita Shah, MD
Medical Director, Emergency Department
Larkspur Valley Health` }),
    msg({ uid: 102, acct: LARK, from: 'medstaff@larkspurvalleyhealth.org', fromName: 'Medical Staff Office', subject: 'Action needed: reappointment packet due 31 October', ago: 3 * HOUR, unread: true,
      body: `
Dear Dr. Carter,

Your two-year reappointment is coming up. Please return the attached
packet by 31 October with:

- a current CV
- proof of your BLS and ACLS certification
- your updated DEA registration

Questions? Reply to this message or call extension 4410.

Medical Staff Office
Larkspur Valley Health`, atts: [{ index: 1, name: 'Reappointment packet 2026.pdf', mime: 'application/pdf', size: 6100 }] }),
    msg({ uid: 103, acct: LARK, from: 'it-security@larkspurvalleyhealth.org', fromName: 'LVH IT Security', subject: 'Reminder: new sign-in prompt from Monday', ago: 1 * DAY + 2 * HOUR,
      body: `
From Monday, signing in to the clinical workstations will ask for your
badge tap and a code from the authenticator app.

IT will never ask for your password by email or phone.` }),
    msg({ uid: 104, acct: LARK, from: 'education@larkspurvalleyhealth.org', fromName: 'LVH Clinical Education', subject: 'Grand Rounds Thursday: sepsis bundle update', ago: 2 * DAY, html: true,
      body: `
Grand Rounds
Thursday 16 October, 12:00 to 13:00, Auditorium B and online

Sepsis bundle update: what changed this year
1.0 CME credit

Register: https://education.larkspurvalleyhealth.example/rounds`,
      unsub: { https: 'https://education.larkspurvalleyhealth.example/preferences' } }),

    /* Saltmarsh Regional */
    msg({ uid: 201, acct: SALT, from: 'mbell@saltmarshregional.org', fromName: 'Marcus Bell', subject: 'Shift swap on the 14th?', ago: 50 * MIN, unread: true,
      mid: 'swap1@saltmarshregional.org', body: `
Hey Riley,

Any chance you could take my day shift on Tuesday the 14th? I'd take your
night on the 20th in return. Staffing already said yes if we both agree.

Marcus` }),
    msg({ uid: 202, acct: SALT, folder: 'sent', to: 'mbell@saltmarshregional.org', toName: 'Marcus Bell', subject: 'Re: Shift swap on the 14th?', ago: 35 * MIN,
      inReplyTo: 'swap1@saltmarshregional.org', mid: 'swap2@saltmarshregional.org', body: `
Works for me. I'll take the 14th, you take the 20th. Can you put it in
the system so I can approve it?

Riley` }),
    msg({ uid: 203, acct: SALT, from: 'ohaddad@saltmarshregional.org', fromName: 'Dr. Omar Haddad', subject: 'Quality committee: minutes and next agenda', ago: 6 * HOUR,
      body: `
Minutes from Tuesday are attached. For next month I've put down:

1. Door-to-provider times on weekends
2. The new handoff checklist pilot
3. Your proposal on discharge instructions in plain language

Omar`, atts: [{ index: 1, name: 'Quality committee minutes Oct.docx', mime: 'application/vnd.openxmlformats-officedocument.wordprocessingml.document', size: 4800 }] }),
    msg({ uid: 204, acct: SALT, from: 'payroll@saltmarshregional.org', fromName: 'Saltmarsh Payroll', subject: 'Your pay statement is ready', ago: 3 * DAY,
      body: `
Your pay statement for the period ending 30 September is ready in the
employee portal. This email holds no pay details.` }),

    /* Personal */
    msg({ uid: PIC_UID, acct: HOME, from: 'jess.morgan@gmail.com', fromName: 'Jess Morgan', subject: 'Photos from Sunday', ago: 2 * HOUR, unread: true,
      body: `
Here are the best ones from the hike! The light at the lake was unreal.
The last one is for your wall.

Jess`, atts: [{ index: 1, name: 'lake-morning.jpg', mime: 'image/jpeg', size: 182000 },
        { index: 2, name: 'ridge.png', mime: 'image/png', size: 96000 },
        { index: 3, name: 'sunset.jpg', mime: 'image/jpeg', size: 154000 }] }),
    msg({ uid: 302, acct: HOME, from: 'elena.carter52@gmail.com', fromName: 'Elena Carter', subject: 'Sunday dinner', ago: 9 * HOUR, unread: true,
      body: `
Hi sweetheart,

Dinner Sunday at 6? Your brother is bringing the kids. Don't worry if
you're post-call, come whenever you wake up.

Love, Mom` }),
    msg({ uid: 303, acct: HOME, from: 'news@trailhead-weekly.example', fromName: 'Trailhead Weekly', subject: 'Five autumn hikes under two hours', ago: 1 * DAY + 5 * HOUR,
      body: `
TRAILHEAD WEEKLY

Five autumn hikes under two hours, a waterproof boot test, and where the
larches turn first this year.

Read online: https://trailhead-weekly.example/autumn`,
      unsub: { https: 'https://trailhead-weekly.example/unsubscribe', mailto: 'mailto:leave@trailhead-weekly.example?subject=unsubscribe' } }),
    msg({ uid: 304, acct: HOME, from: 'alerts@harborcu.example', fromName: 'Harbor Credit Union', subject: 'Your October statement is ready', ago: 4 * DAY,
      body: `
Your statement for the account ending 2291 is ready. Sign in to view it.` }),

    /* Tidepool CPR Training (side business) */
    msg({ uid: 401, acct: CPR, from: 'dana@littleoaksdaycare.com', fromName: 'Dana Whitfield', subject: 'Infant CPR class for 12 staff', ago: 4 * HOUR, unread: true,
      body: `
Hi Riley,

We'd like to book the infant and child CPR class for 12 of our staff.
Could you do a Saturday morning in November, here at the centre?

What would it cost, including the certification cards?

Thanks,
Dana Whitfield
Director, Little Oaks Daycare` }),
    msg({ uid: 402, acct: CPR, from: 'receipts@payably.example', fromName: 'Payably', subject: 'Payment received: $540.00 from Harbourside Gym', ago: 1 * DAY, html: true,
      body: `
Payably

You received $540.00 from Harbourside Gym.
Invoice TP-0187: BLS for fitness staff, 9 participants.
Paid out to your account ending 0934 on 7 October.` }),
    msg({ uid: 403, acct: CPR, folder: 'drafts', to: 'dana@littleoaksdaycare.com', toName: 'Dana Whitfield', subject: 'Re: Infant CPR class for 12 staff', ago: 2 * HOUR,
      body: `
Hi Dana,

Thanks for reaching out. For 12 staff at your centre, the infant and
child CPR class would be`, atts: [{ index: 1, name: 'Tidepool CPR quote, Little Oaks.pdf', mime: 'application/pdf', size: 5200 }] }),

    /* Clearchart Medical Writing (side business) */
    msg({ uid: 501, acct: WRITE, from: 'tom.becker@wellnessquarterly.com', fromName: 'Tom Becker', subject: 'Winter issue: 1,200 words on sleep and shift work?', ago: 7 * HOUR, unread: true,
      body: `
Hi Riley,

Readers loved your piece on hydration. Would you write 1,200 words on
sleep for shift workers for the winter issue? Deadline 15 November,
same rate as last time.

Tom Becker
Editor, Wellness Quarterly` }),
    msg({ uid: 502, acct: WRITE, from: 'lena@ortizbookkeeping.com', fromName: 'Lena Ortiz', subject: 'Q3 numbers for both businesses', ago: 2 * DAY,
      body: `
Hi Riley,

Q3 summary attached for Tidepool CPR and Clearchart. Both are in the
black. Please send me the mileage log by the 20th for the estimated tax
payment.

Lena`, atts: [{ index: 1, name: 'Q3 summary.xlsx', mime: 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet', size: 16000 }] }),
    msg({ uid: 503, acct: WRITE, folder: 'sent', to: 'tom.becker@wellnessquarterly.com', toName: 'Tom Becker', subject: 'Hydration piece, final', ago: 12 * DAY,
      body: `
Hi Tom, final draft attached, with the two sources you asked for.

Riley` }),

    /* Archive and spam */
    msg({ uid: 601, acct: LARK, folder: 'archive', from: 'medstaff@larkspurvalleyhealth.org', fromName: 'Medical Staff Office', subject: 'ACLS renewal recorded', ago: 40 * DAY,
      body: `
Thank you, Dr. Carter. Your ACLS renewal is recorded through 2028.` }),
    msg({ uid: 602, acct: HOME, folder: 'junk', from: 'winner@prize-centre.example', fromName: 'Prize Centre', subject: 'You have been selected!!!', ago: 1 * DAY,
      body: `
Claim your reward today. Click here.` }),
  ];

  const FOLDERS = {
    [LARK]: [{ name: 'Credentialing', label: 'Credentialing' }, { name: 'CME', label: 'CME' }],
    [SALT]: [{ name: 'Committees', label: 'Committees' }],
    [HOME]: [{ name: 'Travel', label: 'Travel' }],
    [CPR]: [{ name: 'Invoices', label: 'Invoices' }],
    [WRITE]: [{ name: 'Contracts', label: 'Contracts' }],
  };
  const IN_FOLDER = {
    CME: [msg({ uid: 701, acct: LARK, folder: { named: 'CME' }, from: 'education@larkspurvalleyhealth.org', fromName: 'LVH Clinical Education', subject: 'Your CME transcript', ago: 20 * DAY,
      body: 'Your transcript shows 31.5 of 50 credits for this cycle.' })],
    Credentialing: [msg({ uid: 702, acct: LARK, folder: { named: 'Credentialing' }, from: 'medstaff@larkspurvalleyhealth.org', fromName: 'Medical Staff Office', subject: 'Privileges approved', ago: 700 * DAY,
      body: 'Your emergency medicine privileges were approved by the board.' })],
    Committees: [msg({ uid: 703, acct: SALT, folder: { named: 'Committees' }, from: 'ohaddad@saltmarshregional.org', fromName: 'Dr. Omar Haddad', subject: 'Quality committee: September minutes', ago: 30 * DAY,
      body: 'September minutes attached. Next meeting in October.' })],
    Travel: [msg({ uid: 704, acct: HOME, folder: { named: 'Travel' }, from: 'trips@railway.example', fromName: 'Railway', subject: 'Your tickets to the coast', ago: 15 * DAY,
      body: 'Coach 6, seat 42. Departs 08:14 on 18 October.' })],
    Invoices: [msg({ uid: 705, acct: CPR, folder: { named: 'Invoices' }, from: 'receipts@payably.example', fromName: 'Payably', subject: 'Payment received: $360.00', ago: 25 * DAY,
      body: 'Invoice TP-0179: Heartsaver class, 6 participants.' })],
    Contracts: [msg({ uid: 706, acct: WRITE, folder: { named: 'Contracts' }, from: 'tom.becker@wellnessquarterly.com', fromName: 'Tom Becker', subject: 'Contributor agreement, signed', ago: 90 * DAY,
      body: 'Signed contributor agreement attached for your records.' })],
  };

  /* New mail that arrives while you look, so the demo shows it coming in. */
  let arrived = false;
  setTimeout(() => {
    arrived = true;
    MAIL.push(msg({ uid: 205, acct: SALT, from: 'mbell@saltmarshregional.org', fromName: 'Marcus Bell', subject: 'Re: Shift swap on the 14th?', ago: 0, unread: true,
      inReplyTo: 'swap2@saltmarshregional.org', mid: 'swap3@saltmarshregional.org', body: `
Done, it's in the system. You owe me nothing, I owe you a coffee.

Marcus` }));
    if (window.__rataMail) window.__rataMail({ email: SALT });
  }, 45000);

  /* Pictures drawn here, so the bytes are real PNG and JPEG, as the app's
     own check (core::sniff_image) requires. */
  function draw(type, w, h, paint) {
    const c = document.createElement('canvas');
    c.width = w; c.height = h;
    paint(c.getContext('2d'), w, h);
    return c.toDataURL(type, 0.86).split(',')[1];
  }
  const sky = (top, bottom) => (g, w, h) => { const s = g.createLinearGradient(0, 0, 0, h); s.addColorStop(0, top); s.addColorStop(1, bottom); g.fillStyle = s; g.fillRect(0, 0, w, h); };
  const hills = (color, base, amp, freq) => (g, w, h) => { g.fillStyle = color; g.beginPath(); g.moveTo(0, h); for (let x = 0; x <= w; x += 8) g.lineTo(x, h * base - Math.sin(x / w * Math.PI * freq) * h * amp - Math.sin(x / 37) * 6); g.lineTo(w, h); g.fill(); };
  const sun = (x, y, r, color) => (g, w, h) => { g.fillStyle = color; g.beginPath(); g.arc(w * x, h * y, r, 0, 7); g.fill(); };
  const scene = (...layers) => (g, w, h) => layers.forEach((l) => l(g, w, h));
  let PICS = null;
  const pictures = () => PICS || (PICS = {
    1: { name: 'lake-morning.jpg', mime: 'image/jpeg', data: draw('image/jpeg', 960, 640, scene(sky('#9cc3e6', '#f3dcc2'), sun(0.7, 0.42, 40, '#fff3d6'), hills('#5d7b8f', 0.62, 0.08, 2), hills('#2f4b5c', 0.78, 0.05, 3), (g, w, h) => { g.fillStyle = 'rgba(160,200,230,.55)'; g.fillRect(0, h * 0.8, w, h * 0.2); })) },
    2: { name: 'ridge.png', mime: 'image/png', data: draw('image/png', 800, 520, scene(sky('#c9d6e8', '#eef2f7'), hills('#8aa0b8', 0.55, 0.16, 1.5), hills('#4e6a5a', 0.75, 0.1, 2.5), hills('#24382c', 0.92, 0.05, 4))) },
    3: { name: 'sunset.jpg', mime: 'image/jpeg', data: draw('image/jpeg', 960, 640, scene(sky('#2b2d6e', '#f08a4b'), sun(0.5, 0.7, 70, '#ffcf7a'), hills('#3b2340', 0.8, 0.06, 2), (g, w, h) => { g.fillStyle = '#1d1426'; g.fillRect(0, h * 0.86, w, h * 0.14); })) },
  });

  const card = (accent, brand, inner) => '<div style="font-family:Helvetica,Arial,sans-serif;max-width:520px;margin:0 auto;color:#1d2433">'
    + '<div style="background:' + accent + ';color:#fff;padding:18px 22px;border-radius:10px 10px 0 0;font-size:20px;font-weight:bold">' + brand + '</div>'
    + '<div style="border:1px solid #e3e5ec;border-top:0;padding:22px;border-radius:0 0 10px 10px">' + inner + '</div></div>';
  const row = (a, b, last) => '<tr><td style="padding:8px 0;' + (last ? '' : 'border-bottom:1px solid #eee') + '">' + a + '</td><td style="text-align:right;' + (last ? '' : 'border-bottom:1px solid #eee') + '">' + b + '</td></tr>';
  const OPEN_HTML = {
    104: card('#2f6f62', 'Grand Rounds', '<p style="font-size:17px;margin-top:0"><b>Sepsis bundle update: what changed this year</b></p>'
      + '<table style="width:100%;border-collapse:collapse;margin:14px 0">' + row('When', 'Thu 16 Oct, 12:00 to 13:00') + row('Where', 'Auditorium B and online') + row('Credit', '1.0 CME', true) + '</table>'
      + '<p><a href="https://education.larkspurvalleyhealth.example/rounds" style="background:#2f6f62;color:#fff;padding:10px 16px;border-radius:8px;text-decoration:none">Register</a></p>'),
    402: card('#4b3fc6', 'Payably', '<p style="margin-top:0">You received <b>$540.00</b> from Harbourside Gym.</p>'
      + '<table style="width:100%;border-collapse:collapse;margin:14px 0">' + row('Invoice', 'TP-0187') + row('BLS for fitness staff', '9 participants') + row('<b>Paid out</b>', '<b>Account 0934, 7 Oct</b>', true) + '</table>'),
  };


  /* Real files behind the attachments, made by RATA's own writers (the same
     ones the Format Bridge converts with), so Convert reads them and Save
     hands them over. */
  const P = (text) => ({ t: 'p', runs: [{ text }] });
  const H = (text, level = 1) => ({ t: 'h', level, runs: [{ text }] });
  const LI = (text) => ({ t: 'li', runs: [{ text }] });
  const TABLE = (rows) => ({ t: 'table', rows });
  const textOf = (blocks) => blocks.map((b) => b.t === 'table' ? b.rows.map((r) => r.join('\t')).join('\n') : b.runs.map((r) => r.text).join('')).join('\n');
  const REAPPOINT = [
    H('Reappointment packet 2026'),
    P('Larkspur Valley Health, Medical Staff Office'),
    H('Return by 31 October', 2),
    LI('A current CV'), LI('Proof of BLS and ACLS certification'), LI('Your updated DEA registration'),
    H('Appointment details', 2),
    TABLE([['Field', 'On file'], ['Name', 'Riley Carter, MD'], ['Department', 'Emergency Medicine'], ['Category', 'Active staff'], ['Current term ends', '30 November 2026']]),
    P('Questions: Medical Staff Office, extension 4410.'),
  ];
  const MINUTES = [
    H('Quality Committee: minutes, October'),
    P('Saltmarsh Regional. Chair: Dr. Omar Haddad.'),
    H('Present', 2), P('O. Haddad, R. Carter, M. Bell, pharmacy and nursing quality leads.'),
    H('Discussed', 2),
    LI('Door-to-provider times on weekends are 9 minutes above target. Staffing pattern review in November.'),
    LI('Handoff checklist pilot: 3 units have started; feedback form goes out next week.'),
    LI('Plain-language discharge instructions: R. Carter to bring a draft template.'),
    H('Actions', 2),
    TABLE([['Action', 'Owner', 'Due'], ['Weekend staffing review', 'M. Bell', '14 Nov'], ['Discharge template draft', 'R. Carter', '11 Nov'], ['Pilot feedback summary', 'O. Haddad', '18 Nov']]),
  ];
  const QUOTE = [
    H('Tidepool CPR Training: quote'),
    P('For Little Oaks Daycare, infant and child CPR at your centre.'),
    TABLE([['Item', 'Qty', 'Price'], ['Infant and child CPR and AED class (4 hours)', '12', '$65.00'], ['Certification cards', '12', 'included'], ['Travel', '1', '$0.00'], ['Total', '', '$780.00']]),
    P('Saturday mornings in November: 8th, 15th or 22nd. Valid for 30 days.'),
  ];
  const FILES = {
    '102:1': { name: 'Reappointment packet 2026.pdf', mime: 'application/pdf', blocks: REAPPOINT, make: () => blocksToPdf(REAPPOINT) },
    '203:1': { name: 'Quality committee minutes Oct.docx', mime: 'application/vnd.openxmlformats-officedocument.wordprocessingml.document', blocks: MINUTES, make: async () => blocksToDocx(MINUTES, 'Quality Committee: minutes, October') },
    '502:1': { name: 'Q3 summary.xlsx', mime: 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet',
      rows: { 'Tidepool CPR': [['Month', 'Classes', 'Income', 'Costs'], ['July', 6, 2340, 410], ['August', 4, 1620, 290], ['September', 7, 2880, 515]],
        Clearchart: [['Month', 'Pieces', 'Income', 'Costs'], ['July', 2, 1400, 60], ['August', 1, 700, 25], ['September', 3, 2150, 90]] },
      make: async function () {
        const X = await brLib('xlsx', 'XLSX');
        const wb = X.utils.book_new();
        for (const [name, rows] of Object.entries(this.rows)) X.utils.book_append_sheet(wb, X.utils.aoa_to_sheet(rows), name);
        return new Blob([X.write(wb, { type: 'array', bookType: 'xlsx' })]);
      } },
    '403:1': { name: 'Tidepool CPR quote, Little Oaks.pdf', mime: 'application/pdf', blocks: QUOTE, make: () => blocksToPdf(QUOTE) },
  };
  const made = {};
  const fileOf = (uid, index) => {
    const f = FILES[uid + ':' + index];
    if (!f) return null;
    made[uid + ':' + index] = made[uid + ':' + index] || f.make();
    return made[uid + ':' + index].then((blob) => ({ f, blob }));
  };
  const b64 = (blob) => new Promise((ok, no) => { const r = new FileReader(); r.onload = () => ok(String(r.result).split(',')[1]); r.onerror = () => no(r.error); r.readAsDataURL(blob); });
  const fromB64 = (s) => Uint8Array.from(atob(s), (c) => c.charCodeAt(0));

  /* Saving goes through the page's downloads permission: the viewer is
     asked, and may say no. */
  async function hand(filename, data) {
    const dl = window.claude && window.claude.use ? await window.claude.use('downloads') : null;
    if (!dl) throw 'Saving files is not available in this view of the demo.';
    try { await dl.save({ filename, data }); }
    catch (e) {
      if (e && e.code === 'declined') throw 'Not saved: you chose not to keep ' + filename + '.';
      if (e && e.code === 'rejected_extension') throw 'The demo cannot hand over a file of that type.';
      throw 'It could not be saved here (' + ((e && e.code) || 'unknown') + ').';
    }
    return { path: 'your browser’s downloads', name: filename, size: data.size || data.length || 0 };
  }

  /* The Files section starts with documents in it: copies of the
     attachments above, kept the way the Format Bridge keeps a file. */
  async function seedFiles() {
    try { if (localStorage.getItem('rata_demo_files') === DEMO_VERSION) return; } catch (e) {}
    for (let i = 0; i < 80; i++) {
      if (typeof S !== 'undefined' && S && Array.isArray(S.documents) && typeof fvPut === 'function' && typeof blocksToPdf === 'function') break;
      await new Promise((r) => setTimeout(r, 250));
    }
    if (typeof S === 'undefined' || !S || !Array.isArray(S.documents)) return;
    let k = 0;
    for (const key of ['102:1', '203:1', '502:1']) {
      const [uid, index] = key.split(':').map(Number);
      const owner = find(uid);
      if (!owner || !active(owner.acct)) continue;
      const got = await fileOf(uid, index).catch(() => null);
      if (!got || S.documents.some((d) => d.name === got.f.name)) continue;
      const id = 'demo-doc-' + uid;
      try { await fvPut(id, got.blob); } catch (e) { continue; }
      const content = got.f.blocks ? textOf(got.f.blocks) : Object.entries(got.f.rows).map(([n, rows]) => n + '\n' + rows.map((r) => r.join('\t')).join('\n')).join('\n\n');
      S.documents.push({ id, name: got.f.name, fmt: got.f.name.split('.').pop().toUpperCase(), origin: 'email', prov: 'email',
        size: fmtSize(got.blob.size), bytes: got.blob.size, ts: now - (++k) * DAY, content, hasFile: true });
    }
    save();
    if (typeof renderDocs === 'function') renderDocs();
    try { localStorage.setItem('rata_demo_files', DEMO_VERSION); } catch (e) {}
  }
  if (document.readyState === 'complete') seedFiles(); else addEventListener('load', () => setTimeout(seedFiles, 1500));

  const all = () => MAIL;
  const find = (uid) => MAIL.concat(...Object.values(IN_FOLDER)).find((m) => m.uid === uid);
  let SENT = 0;

  window.__TAURI__ = { core: { invoke: async (cmd, args) => {
    await new Promise((r) => setTimeout(r, 120 + Math.random() * 180));
    switch (cmd) {
      case 'licence_status':
      case 'set_licence':
        return { licensed: true, message: 'Demo licence: ' + PLANS[PLAN].label + ' until 31 December 2026.', plan: PLANS[PLAN],
          used: ACTIVE.length, limit: PLANS[PLAN].mail, renewSoon: false, token: 'demo', reason: null, epoch: 0 };
      case 'list_mailboxes': return ACTIVE;
      case 'refresh_mail': {
        const only = args && args.only;
        return { messages: all().filter((m) => active(m.acct) && (!only || only.includes(m.acct))).map((m) => ({ ...m })), problems: [], skipped: [] };
      }
      case 'older_mail': return [];
      case 'reread_mail': return (args.uids || []).map(find).filter(Boolean);
      case 'open_message': {
        const m = find(args.uid) || {};
        return { text: m.body || '', truncated: false, attachments: m.attachments || [], html: OPEN_HTML[args.uid] || null, remote_images: false };
      }
      case 'read_pictures': {
        const p = pictures();
        return (args.indexes || []).map((i) => {
          const f = p[i];
          if (!f || args.uid !== PIC_UID) return { index: i, picture: null, data: null, reason: 'not-a-picture' };
          return { index: i, picture: f.mime === 'image/png' ? 'png' : 'jpeg', data: f.data, reason: null };
        });
      }
      case 'read_attachment': {
        const got = await (fileOf(args.uid, args.index) || Promise.resolve(null));
        if (!got) throw { email: args.email, kind: 'gone', error: 'That attachment is no longer in the message.' };
        return { name: got.f.name, mime: got.f.mime, data: await b64(got.blob), picture: null };
      }
      case 'save_attachment': {
        const got = await (fileOf(args.uid, args.index) || Promise.resolve(null));
        if (got) return hand(got.f.name, got.blob);
        const p = args.uid === PIC_UID ? pictures()[args.index] : null;
        if (p) return hand(p.name, new Blob([fromB64(p.data)]));
        throw { email: args.email, kind: 'gone', error: 'That attachment is no longer in the message.' };
      }
      case 'save_file': return hand(args.name, new Blob([fromB64(args.data)]));
      case 'list_folders': return FOLDERS[args.email] || [];
      case 'folder_mail': return (IN_FOLDER[args.folder && args.folder.named] || []).filter((m) => m.acct === args.email).map((m) => ({ ...m }));
      case 'change_messages': return { ok: true, done: args.uids, gone: [] };
      case 'send_mail':
        SENT++;
        return { via: 'nowhere (this is a demo, nothing was sent)', messageId: 'demo-sent-' + SENT + '@rata.demo' };
      case 'save_draft': return { outcome: 'no-place', error: 'This is a demo, so the draft stays in this browser.' };
      case 'search_mail': return { messages: [], matched: 0 };
      case 'watching': return arrived ? [] : ACTIVE.map((m) => m.email);
      case 'notify_mail': return null;
      case 'check_update': return { enabled: false, current: '0.1.46', releases: 'https://github.com/Noallrightokay/center-point-inbox/releases' };
      case 'diagnostics':
        return 'RATA diagnostics (demo)\nVersion: 0.1.46 (demo in a browser)\nLicence: ' + PLANS[PLAN].label + '\nMailboxes: ' + ACTIVE.length + '\n\n'
          + ACTIVE.map((m, i) => 'Mailbox ' + (i + 1) + ': ' + m.label + '\n  IMAP: ' + m.host + ':993 (TLS from the start)\n  Last error: none since RATA started').join('\n\n');
      case 'open_link': window.open(args.url, '_blank', 'noopener'); return null;
      case 'link_mailbox':
        /* As core::may_link: Base holds two mailboxes. */
        if (PLANS[PLAN].mail !== null && ACTIVE.length >= PLANS[PLAN].mail) throw 'RATA Base includes 2 mailboxes. Upgrade at mailrata.org to add another.';
        return { outcome: 'failed', error: 'This demo cannot link a real mailbox. Download RATA to add yours.' };
      case 'discover_mailbox': return { label: 'Your provider', microsoft: false, configured: false };
      case 'microsoft_ready': return false;
      case 'link_microsoft': return { outcome: 'microsoft', configured: false, error: 'This demo cannot sign in to Microsoft.' };
      case 'cancel_microsoft': return false;
      case 'unlink_mailbox': return null;
      case 'forget_everything': return { mailboxes: 0 };
      case 'install_update': return null;
      default: throw 'This demo does not do that (' + cmd + ').';
    }
  } } };

  /* A small label so nobody mistakes the demo for their own mail. */
  function badge() {
    const b = document.createElement('div');
    b.id = 'demo-badge';
    b.setAttribute('role', 'note');
    b.innerHTML = '<b>Demo</b><span class="demo-words">Sample mail, nothing is sent</span>'
      + '<span class="demo-plan" role="group" aria-label="Plan"><button type="button" data-plan="base">Base</button><button type="button" data-plan="pro">Pro</button></span>'
      + '<button type="button" id="demo-reset">Reset</button>';
    document.body.appendChild(b);
    b.querySelectorAll('[data-plan]').forEach((x) => {
      x.setAttribute('aria-pressed', String(x.dataset.plan === PLAN));
      x.onclick = () => { if (x.dataset.plan !== PLAN) restart(x.dataset.plan); };
    });
    document.getElementById('demo-reset').onclick = () => restart(PLAN);
  }
  /* Start the demo again on a plan: a Base customer never had five
     mailboxes, so the workspace starts fresh rather than keeping theirs. */
  function restart(plan) {
    try {
      localStorage.clear();
      if (plan === 'base') localStorage.setItem('rata_demo_plan', 'base');
      indexedDB.deleteDatabase('rata-mail-' + UID);
      indexedDB.deleteDatabase('rata-files');
    } catch (e) {}
    setTimeout(() => location.reload(), 300);
  }
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', badge); else badge();
})();
