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
--    Holds the user's entire RATA workspace (messages, people,
--    documents, settings, audit chain) as JSON. Synced by the
--    app automatically ~1.5s after every change.
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
  plan text not null default 'base',        -- 'base' | future: 'automations' | 'crm'
  status text,                              -- active | trialing | past_due | canceled
  stripe_customer text,
  updated_at timestamptz not null default now()
);

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
-- 4. PROVIDER TOKENS — server-held credentials for live
--    Outlook, Slack, and iCloud sync. Written and read ONLY by
--    the RATA backend (service role). No client policies on
--    purpose. NOTE: values are stored in plaintext columns —
--    see the credential-storage note in BACKEND-SETUP.md.
-- ------------------------------------------------------------
create table if not exists public.provider_tokens (
  user_id uuid not null references auth.users (id) on delete cascade,
  provider text not null,               -- 'ms' | 'slack' | 'apple'
  label text,
  access text,
  refresh text,
  extra jsonb,
  expires_at timestamptz,
  updated_at timestamptz not null default now(),
  primary key (user_id, provider)
);
alter table public.provider_tokens enable row level security;

-- ------------------------------------------------------------
-- 5. LINK STATES — short-lived one-time states for OAuth flows
-- ------------------------------------------------------------
create extension if not exists pgcrypto;
create table if not exists public.link_states (
  id uuid primary key default gen_random_uuid(),
  user_id uuid not null references auth.users (id) on delete cascade,
  provider text not null,
  created_at timestamptz not null default now()
);
alter table public.link_states enable row level security;
