-- ============================================================
-- RATA — database schema (Supabase / Postgres)
-- Run this ONCE in your Supabase project:
--   supabase.com → your project → SQL Editor → paste → Run
-- Safe to re-run: uses IF NOT EXISTS / OR REPLACE throughout.
-- Accounts themselves live in Supabase Auth (auth.users) —
-- created automatically when people sign up on your site.
-- ============================================================

-- ------------------------------------------------------------
-- 1. WORKSPACES — one row per registered user.
--    Holds the user's preferences as JSON: settings, folders,
--    rules and the list of linked mailboxes (CLOUD_FIELDS in
--    app.html), minus per-device facts (CLOUD_STRIP: API keys,
--    signatures, sync state). Never mail, people, documents or
--    the audit chain: those stay on the device that read them.
--    Synced by the app automatically ~1.5s after a change; each
--    sync replaces the whole row, so what an older build uploaded
--    goes with the first save after an update.
-- ------------------------------------------------------------
create table if not exists public.workspaces (
  id uuid primary key references auth.users (id) on delete cascade,
  data jsonb,
  updated_at timestamptz not null default now()
);

alter table public.workspaces enable row level security;

drop policy if exists "own workspace" on public.workspaces;
create policy "own workspace" on public.workspaces
  for all
  using (auth.uid() = id)
  with check (auth.uid() = id);

-- keep updated_at honest on every write
create or replace function public.touch_updated_at()
returns trigger language plpgsql as $$
begin
  new.updated_at := now();
  return new;
end $$;

drop trigger if exists workspaces_touch on public.workspaces;
create trigger workspaces_touch
  before update on public.workspaces
  for each row execute function public.touch_updated_at();

-- ------------------------------------------------------------
-- 2. SUBSCRIPTIONS — billing entitlements (Stripe).
--    Written ONLY by the Stripe webhook Edge Function
--    (service role — see STRIPE-SETUP.md). Users can read
--    their own row; the app checks it at sign-in to set the
--    plan. Harmless to create now while the site is free —
--    it simply sits empty until Stripe goes live.
-- ------------------------------------------------------------
create table if not exists public.subscriptions (
  email text primary key,
  plan text not null default 'base',        -- 'base' | 'pro' | 'enterprise'
  status text,                              -- active | trialing | past_due | canceled
  stripe_customer text,
  -- Custom domains bought as an add-on ($1.50/month each). Pro includes none
  -- and buys them here; Enterprise includes them all and ignores this.
  domain_addons integer not null default 0,
  -- When Stripe says the event that last wrote this row happened. Deliveries
  -- are not ordered, so this is what stops an upgrade that arrived late from
  -- overwriting the downgrade that actually came after it.
  event_at timestamptz,
  updated_at timestamptz not null default now()
);

-- Safe to run against a table created before the add-on existed.
alter table public.subscriptions
  add column if not exists domain_addons integer not null default 0;
alter table public.subscriptions
  add column if not exists event_at timestamptz;

alter table public.subscriptions enable row level security;

drop policy if exists "read own sub" on public.subscriptions;
create policy "read own sub" on public.subscriptions
  for select
  using (lower(auth.jwt() ->> 'email') = email);

drop trigger if exists subscriptions_touch on public.subscriptions;
create trigger subscriptions_touch
  before update on public.subscriptions
  for each row execute function public.touch_updated_at();

-- ------------------------------------------------------------
-- 3. Helpful index for the billing lookup the app performs
-- ------------------------------------------------------------
create index if not exists subscriptions_status_idx
  on public.subscriptions (status);

-- Done. Verify: Table Editor should now show
--   public.workspaces  and  public.subscriptions
-- both with RLS enabled (shield icon).

-- ------------------------------------------------------------
-- 4. RETIRED — provider_tokens and link_states
--
--    RATA used to hold everybody's mailbox passwords, encrypted
--    with TOKEN_ENC_KEY, so a server could read their mail for
--    them. It does not any more: the app runs on the customer's
--    own machine and their passwords are in that machine's
--    keychain, where a breach here cannot reach them.
--
--    Nothing in the application reads these two tables now. They
--    are left standing rather than dropped in the same breath,
--    because this file is run against live databases and a
--    `drop table` in it would take real rows with it the next
--    time somebody applied the schema.
--
--    Drop them deliberately, when you are satisfied the desktop
--    app is working and nobody needs a second look at what was
--    there:
--
--      drop table if exists public.provider_tokens;
--      drop table if exists public.link_states;
--
--    Those rows are ciphertext and are worthless without
--    TOKEN_ENC_KEY, so the safest order is to drop the tables
--    first and destroy the key afterwards.
-- ------------------------------------------------------------

-- ------------------------------------------------------------
-- 5. AI_USAGE — what each customer's AI requests cost, per month.
--
--    The AI relay (app/api/ai) pays for summaries, translation
--    and task flags, and caps each customer at AI_MONTHLY_CAP_USD
--    a month. This is the running total it checks. It holds an
--    address, a month and a number — never any text: the mail
--    passes through the relay and is not written anywhere.
--
--    Only the server (service role) touches it, through the two
--    functions below; no policy lets a signed-in user read or
--    write it.
-- ------------------------------------------------------------
create table if not exists public.ai_usage (
  email text not null,
  month text not null,                    -- 'YYYY-MM', UTC
  micro_usd bigint not null default 0,    -- millionths of a dollar
  requests integer not null default 0,
  updated_at timestamptz not null default now(),
  primary key (email, month)
);

alter table public.ai_usage enable row level security;

create or replace function public.ai_spent(p_email text, p_month text)
returns bigint language sql stable security definer set search_path = public as $$
  select coalesce((select micro_usd from public.ai_usage where email = p_email and month = p_month), 0);
$$;

-- One statement, so two requests finishing together cannot lose a charge.
create or replace function public.ai_charge(p_email text, p_month text, p_micro bigint)
returns bigint language sql security definer set search_path = public as $$
  insert into public.ai_usage (email, month, micro_usd, requests)
  values (p_email, p_month, greatest(p_micro, 0), 1)
  on conflict (email, month) do update
    set micro_usd = public.ai_usage.micro_usd + greatest(excluded.micro_usd, 0),
        requests = public.ai_usage.requests + 1,
        updated_at = now()
  returning micro_usd;
$$;

revoke all on function public.ai_spent(text, text) from public, anon, authenticated;
revoke all on function public.ai_charge(text, text, bigint) from public, anon, authenticated;
grant execute on function public.ai_spent(text, text) to service_role;
grant execute on function public.ai_charge(text, text, bigint) to service_role;
