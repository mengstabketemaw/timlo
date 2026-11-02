-- timlo sync schema. Run once in the Supabase dashboard: SQL Editor → New query.
--
-- The tables mirror the local SQLite tables, plus:
--   user_id    the owner; row-level security limits every row to its owner
--   synced_at  set by the server on every accepted write; devices pull rows
--              newer than the last synced_at they saw
-- Writes are last-write-wins by updated_at: an upsert carrying an older or
-- equal updated_at than the stored row is silently skipped.

create or replace function public.timlo_lww() returns trigger
language plpgsql as $$
begin
  if tg_op = 'UPDATE' and new.updated_at <= old.updated_at then
    return null;
  end if;
  new.synced_at := clock_timestamp();
  return new;
end
$$;

create table if not exists public.templates (
  user_id    uuid    not null default auth.uid() references auth.users on delete cascade,
  id         text    not null,
  name       text    not null,
  colour     text,
  updated_at bigint  not null,
  deleted    integer not null default 0,
  synced_at  timestamptz not null default clock_timestamp(),
  primary key (user_id, id)
);

create table if not exists public.template_blocks (
  user_id     uuid    not null default auth.uid() references auth.users on delete cascade,
  id          text    not null,
  template_id text    not null,
  start_min   integer not null,
  end_min     integer not null,
  title       text    not null,
  note        text,
  icon        text,
  colour      text,
  updated_at  bigint  not null,
  deleted     integer not null default 0,
  synced_at   timestamptz not null default clock_timestamp(),
  primary key (user_id, id)
);

create table if not exists public.week_assignment (
  user_id     uuid    not null default auth.uid() references auth.users on delete cascade,
  weekday     integer not null,
  template_id text,
  updated_at  bigint  not null,
  deleted     integer not null default 0,
  synced_at   timestamptz not null default clock_timestamp(),
  primary key (user_id, weekday)
);

create table if not exists public.days (
  user_id     uuid    not null default auth.uid() references auth.users on delete cascade,
  date        text    not null,
  template_id text,
  customised  integer not null default 0,
  overridden  integer not null default 0,
  updated_at  bigint  not null,
  deleted     integer not null default 0,
  synced_at   timestamptz not null default clock_timestamp(),
  primary key (user_id, date)
);

create table if not exists public.day_blocks (
  user_id           uuid    not null default auth.uid() references auth.users on delete cascade,
  id                text    not null,
  date              text    not null,
  template_block_id text,
  start_min         integer not null,
  end_min           integer not null,
  title             text    not null,
  note              text,
  status            text    not null default 'pending',
  status_at         bigint,
  updated_at        bigint  not null,
  deleted           integer not null default 0,
  synced_at         timestamptz not null default clock_timestamp(),
  primary key (user_id, id)
);

create table if not exists public.settings (
  user_id    uuid    not null default auth.uid() references auth.users on delete cascade,
  key        text    not null,
  value      text    not null,
  updated_at bigint  not null,
  deleted    integer not null default 0,
  synced_at  timestamptz not null default clock_timestamp(),
  primary key (user_id, key)
);

do $$
declare
  t text;
begin
  foreach t in array array['templates', 'template_blocks', 'week_assignment', 'days', 'day_blocks', 'settings']
  loop
    execute format('create index if not exists %I on public.%I (user_id, synced_at)', t || '_synced', t);
    execute format('alter table public.%I enable row level security', t);
    execute format('drop policy if exists "own rows" on public.%I', t);
    execute format(
      'create policy "own rows" on public.%I for all to authenticated
         using (user_id = auth.uid()) with check (user_id = auth.uid())', t);
    execute format('drop trigger if exists timlo_lww on public.%I', t);
    execute format(
      'create trigger timlo_lww before insert or update on public.%I
         for each row execute function public.timlo_lww()', t);
    execute format('grant select, insert, update on public.%I to authenticated', t);
    -- Realtime: devices hear about each other's changes at once.
    if not exists (
      select 1 from pg_publication_tables
      where pubname = 'supabase_realtime' and schemaname = 'public' and tablename = t
    ) then
      execute format('alter publication supabase_realtime add table public.%I', t);
    end if;
  end loop;
end
$$;
