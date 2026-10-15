/* The public deletion page, /in-app/delete-account.

   Signs the person in with their mailrata.org email address and password,
   through the same Supabase client the website's sign-in page uses (its
   version in LIB is that page's, which the test fetches), then asks the account
   route to delete (DELETE /api/account, the route the app's Settings call
   too). Whatever the route refuses with is said in its own words; the rules
   are lib/account.js's, not this page's.

   Nothing is kept in the browser: the client holds its session in memory
   only, Cancel ends it at the sign-in service, and it goes with the page.
   Every word is drawn with textContent. */
(() => {
  'use strict';
  const LIB = '/vendor/supabase-js-2.112.4.js';
  const UNREACHABLE = 'Could not reach mailrata.org. Check your connection and try again.';
  const $ = id => document.getElementById(id);
  const cfg = window.RATA_CONFIG || {};
  const form = $('del-signin');
  const email = $('del-email');
  const password = $('del-password');
  const signIn = $('del-signin-go');
  const ask = $('del-confirm');
  const go = $('del-go');
  const cancel = $('del-cancel');
  const msg = $('del-msg');
  const done = $('del-done');

  if (!cfg.supabaseUrl || !cfg.supabaseKey) {
    $('del-off').hidden = false;
    for (const el of form.querySelectorAll('input, button')) el.disabled = true;
    return;
  }

  let sb = null;
  let session = null;
  let busy = false;

  function say(text) {
    msg.textContent = text || '';
    msg.hidden = !text;
  }

  function loadLib() {
    if (window.supabase) return Promise.resolve();
    return new Promise((resolve, reject) => {
      const s = document.createElement('script');
      s.src = LIB;
      s.onload = resolve;
      s.onerror = reject;
      document.head.appendChild(s);
    });
  }

  async function client() {
    if (!sb) {
      await loadLib();
      sb = window.supabase.createClient(cfg.supabaseUrl, cfg.supabaseKey, {
        auth: { persistSession: false, autoRefreshToken: false, detectSessionInUrl: false },
      });
    }
    return sb;
  }

  form.addEventListener('submit', async (e) => {
    e.preventDefault();
    if (busy) return;
    const address = email.value.trim();
    if (!address || !password.value) {
      say('Type your email address and password.');
      (address ? password : email).focus();
      return;
    }
    busy = true;
    signIn.disabled = true;
    try {
      const c = await client();
      const { data, error } = await c.auth.signInWithPassword({ email: address, password: password.value });
      if (error || !data || !data.session) {
        /* A sign-in service that could not be reached says so in its own
           terms ("Failed to fetch"); everything else is its own sentence. */
        say(error && error.status !== 0 && error.message ? error.message : UNREACHABLE);
        password.focus();
        return;
      }
      session = data.session;
      password.value = '';
      say('');
      $('del-who').textContent = String((data.user && data.user.email) || address).toLowerCase();
      form.hidden = true;
      ask.hidden = false;
      $('del-confirm-h').focus();
    } catch {
      say(UNREACHABLE);
      password.focus();
    } finally {
      busy = false;
      signIn.disabled = false;
    }
  });

  go.addEventListener('click', async () => {
    if (busy || !session) return;
    busy = true;
    go.disabled = true;
    cancel.disabled = true;
    say('');
    let answer = null;
    try {
      const r = await fetch('/api/account', {
        method: 'DELETE',
        headers: { Authorization: 'Bearer ' + session.access_token, 'Content-Type': 'application/json' },
        /* The route deletes only when told the account's own address,
           which the person has just typed to sign in. */
        body: JSON.stringify({ confirm: $('del-who').textContent }),
      });
      answer = await r.json().catch(() => null);
      if (r.ok && answer && answer.ok === true) {
        session = null;
        sb = null;
        ask.hidden = true;
        done.hidden = false;
        $('del-done-h').focus();
        return;
      }
    } catch { /* said below */ }
    say(answer && typeof answer.error === 'string' && answer.error ? answer.error : UNREACHABLE);
    go.disabled = false;
    cancel.disabled = false;
    go.focus();
    busy = false;
  });

  cancel.addEventListener('click', async () => {
    if (busy) return;
    busy = true;
    go.disabled = true;
    cancel.disabled = true;
    const c = sb;
    session = null;
    /* Ended at the sign-in service before the form comes back, so the next
       sign-in is never ended by this one; at most five seconds. */
    if (c) await Promise.race([c.auth.signOut().catch(() => {}), new Promise(r => setTimeout(r, 5000))]);
    say('');
    $('del-who').textContent = '';
    ask.hidden = true;
    form.hidden = false;
    go.disabled = false;
    cancel.disabled = false;
    busy = false;
    email.focus();
  });
})();
