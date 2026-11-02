# timlo — day planner

A personal day planner that always shows what you should be doing right now.
Spec: [Day Planner — Spec](https://claude.ai/artifact/A7GszREJWzERksYrkiRSSr).

This repo has milestones 1–4:

| Part | What it is |
| --- | --- |
| `crates/planner-core` | Shared logic: data model, template → day rules, now/next, reminders, settings, SQLite storage, plan-file import. |
| `crates/plannerd` | Linux background service + CLI: Waybar module, desktop notifications, Done/Skip clicks. |
| `crates/planner-sync` | Supabase sync: login, push/pull, Realtime. Run by `plannerd`. |
| `app/` | Planner app (Tauri 2 + Svelte): Today, Templates, Week and Calendar screens. Same database as `plannerd`. |
| `supabase/schema.sql` | Tables, row-level security and Realtime setup for the Supabase project. |

Android and summaries come in later milestones.

## Install (Arch / Hyprland)

```sh
cargo install --path crates/plannerd          # → ~/.cargo/bin/plannerd
plannerd init                                 # writes ~/.config/timlo/plan.toml and imports it
install -Dm644 contrib/systemd/plannerd.service ~/.config/systemd/user/plannerd.service
systemctl --user daemon-reload
systemctl --user enable --now plannerd
```

If your Hyprland session doesn't start `graphical-session.target` (it does
under uwsm), change `WantedBy=` in the unit to `default.target`, or add
`exec-once = plannerd daemon` to `hyprland.conf` instead.

Then add the module from `contrib/waybar/config.jsonc` to your Waybar config
and, optionally, the classes in `contrib/waybar/style.css`.

### Planner app

```sh
cd app && npm install && npx tauri build --no-bundle
install -Dm755 ../target/release/timlo ~/.local/bin/timlo
```

Left-clicking the Waybar module runs `timlo`; a second launch focuses the
open window. To get a floating popup under Waybar instead of a tiled window,
add the rules in `contrib/hyprland.conf`. For development: `cd app && npx tauri dev`.

### Sync (Supabase)

1. Create a project at [supabase.com](https://supabase.com) (free tier).
2. SQL Editor → New query → paste `supabase/schema.sql` → Run.
3. Optional, for a single-user setup: Authentication → Sign In / Providers →
   Email → turn off "Confirm email" (otherwise click the link it sends).
4. Project Settings → API: copy the Project URL and the anon public key, then:

```sh
plannerd login --signup --url https://<project>.supabase.co --key <anon key>   # first device
plannerd login                                                               # later logins
```

The first login on a device sends all local plans up and pulls everything
down. After that, the running `plannerd` pushes each change as it happens,
hears about the other device's changes through Realtime, and does a catch-up
pull on start and every 15 minutes. Offline, everything keeps working from the
local database and queued changes go out when the connection is back.

Conflicts: last write wins per row. Copying a template into a new day and
marking blocks Missed don't count as writes, so they never override a
Done/Skip from the other device.

```text
plannerd sync-status   account, queued changes, last change received
plannerd sync          push and pull now
plannerd logout        stop syncing on this device
```

## Using it

Plan in the app: build templates, pick each weekday's default, and switch or
edit single days from Today or the Calendar.

The plan file `~/.config/timlo/plan.toml` still works: when you save it, the
daemon imports it within a minute (templates are matched by name). It is only
re-read when the file changes, so restarting the daemon never overwrites edits
made in the app; but saving the file again does overwrite the templates it
names.

| Waybar | Action |
| --- | --- |
| Left click | Open the planner app |
| Right click | Mark the current block Done |
| Middle click | Mark the current block Skipped |

In a gap, Done/Skip apply to the most recent block that ended without an answer.

```text
plannerd today [--date 2026-11-03]   print a day's plan
plannerd show                        today's plan as a notification
plannerd switch "Rest day" [--date]  run a day from another template
plannerd templates                   list templates and weekday defaults
plannerd done | skip                 same as the Waybar clicks
plannerd status                      print the Waybar JSON once
```

CSS classes: `active`, `ending-soon`, `free`, `day-over`, `done`, `skipped`,
`empty`, `offline`.

Files: database `~/.local/share/timlo/planner.db`, plan
`~/.config/timlo/plan.toml`, socket `$XDG_RUNTIME_DIR/timlo-plannerd.sock`
(override with `TIMLO_DB`, `TIMLO_PLAN`, `TIMLO_SOCKET`).

## Decisions on the spec's open questions (for now)

- Switching today's template mid-day replaces only the rest of the day; blocks
  that already ended keep their Done/Skip records.
- Blocks can't cross midnight; they must end by 24:00.
- No recurring items inside a template; use separate templates.
- A past date that was never opened gets an empty plan, so it doesn't count as
  a day of missed blocks.
- `days` has an extra `overridden` column so changing a weekday's default
  moves future days that follow it but not days you switched by hand.

## Development

```sh
cargo test                      # Rust: core + daemon
cd app && npm run check         # Svelte/TypeScript
cd app && npx tauri dev         # run the app with hot reload
```
