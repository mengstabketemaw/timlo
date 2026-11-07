<script lang="ts">
  import DayView from './lib/DayView.svelte'
  import Focus from './lib/Focus.svelte'
  import Calendar from './lib/Calendar.svelte'
  import SettingsView from './lib/Settings.svelte'
  import Tasks from './lib/Tasks.svelte'

  import { listen } from '@tauri-apps/api/event'
  import { api } from './lib/api'
  import { ui, type Tab } from './lib/ui.svelte'
  import { focus, startFor } from './lib/focus.svelte'

  const tabs: [Tab, string][] = [
    ['today', 'Today'],
    ['tasks', 'Tasks'],
    ['focus', 'Focus'],
    ['calendar', 'Calendar'],
    ['settings', 'Settings'],
  ]

  // Reload when the window gets focus and every 30 s, so Done/Skip from
  // Waybar or a notification and the moving "now" show up.
  let tick = $state(0)

  /** Focus on the block running now, until it ends (or a plain session if none). */
  async function focusNow() {
    ui.tab = 'focus'
    try {
      const d = await api.day()
      const m = d.now_min
      const b = m === null ? undefined : d.blocks.find((b) => b.start_min <= m && m < b.end_min)
      if (b && m !== null) await startFor(b.title, b.end_min - m)
      else if (!focus.playing) await startFor('', focus.length || 50)
    } catch {}
  }

  // `timlo --add-task` / `--focus`, the Android widget and notification.
  async function takeLaunchAction() {
    const action = await api.takeLaunchAction().catch(() => null)
    if (action === 'add-task') {
      ui.tab = 'tasks'
      ui.quickAdd++
    } else if (action === 'focus') {
      await focusNow()
    }
  }

  $effect(() => {
    takeLaunchAction()
    const unlisten = listen('launch-action', takeLaunchAction)
    const onVisible = () => document.visibilityState === 'visible' && takeLaunchAction()
    document.addEventListener('visibilitychange', onVisible)
    window.addEventListener('focus', takeLaunchAction)
    return () => {
      unlisten.then((f) => f())
      document.removeEventListener('visibilitychange', onVisible)
      window.removeEventListener('focus', takeLaunchAction)
    }
  })

  $effect(() => {
    const bump = () => tick++
    window.addEventListener('focus', bump)
    const timer = setInterval(bump, 30_000)
    return () => {
      window.removeEventListener('focus', bump)
      clearInterval(timer)
    }
  })
</script>

<nav>
  <span class="brand">timlo</span>
  {#each tabs as [id, label]}
    <button class="tab" class:active={ui.tab === id} class:gear={id === 'settings'} onclick={() => (ui.tab = id)} aria-label={label}>
      {#if id === 'settings'}<span class="wide">{label}</span><span class="narrow" aria-hidden="true">⚙</span>{:else}{label}{/if}
    </button>
  {/each}
</nav>

<main>
  {#if ui.tab === 'today'}
    <DayView refreshKey={tick} />
  {:else if ui.tab === 'tasks'}
    <Tasks refreshKey={tick} />
  {:else if ui.tab === 'focus'}
    <Focus />
  {:else if ui.tab === 'calendar'}
    <Calendar refreshKey={tick} />
  {:else}
    <SettingsView />
  {/if}
</main>

<style>
  nav {
    position: sticky;
    top: 0;
    z-index: 1;
    display: flex;
    gap: 0.25rem;
    align-items: center;
    padding: 0.5rem 1rem;
    background: var(--surface);
    border-bottom: 1px solid var(--border);
    overflow-x: auto;
  }
  .brand {
    font-weight: 700;
    margin-right: 0.75rem;
    color: var(--accent);
  }
  .tab {
    border-color: transparent;
    background: transparent;
  }
  .tab.active {
    background: var(--surface-2);
    font-weight: 600;
  }
  .narrow {
    display: none;
  }
  /* Phones and the narrow Waybar popup: every tab fits without scrolling; Settings becomes ⚙. */
  @media (max-width: 560px) {
    nav {
      gap: 0;
      padding: 0.4rem 0.25rem;
      overflow-x: visible;
    }
    .brand {
      display: none;
    }
    .tab {
      flex: 1 1 auto;
      min-width: 0;
      padding: 0.45rem 0.1rem;
      font-size: 0.82rem;
      white-space: nowrap;
    }
    main {
      padding: 0.75rem;
    }
    .tab.gear {
      flex: 0 0 2rem;
      font-size: 1.05rem;
    }
    .wide {
      display: none;
    }
    .narrow {
      display: inline;
    }
  }
  main {
    max-width: 1100px;
    margin: 0 auto;
    padding: 1rem;
  }
</style>
