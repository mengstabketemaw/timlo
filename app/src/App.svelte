<script lang="ts">
  import DayView from './lib/DayView.svelte'
  import Templates from './lib/Templates.svelte'
  import Week from './lib/Week.svelte'
  import Calendar from './lib/Calendar.svelte'

  type Tab = 'today' | 'templates' | 'week' | 'calendar'
  const tabs: [Tab, string][] = [
    ['today', 'Today'],
    ['templates', 'Templates'],
    ['week', 'Week'],
    ['calendar', 'Calendar'],
  ]
  let tab = $state<Tab>('today')

  // Reload when the window gets focus and every 30 s, so Done/Skip from
  // Waybar or a notification and the moving "now" show up.
  let tick = $state(0)
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
    <button class="tab" class:active={tab === id} onclick={() => (tab = id)}>{label}</button>
  {/each}
</nav>

<main>
  {#if tab === 'today'}
    <DayView refreshKey={tick} />
  {:else if tab === 'templates'}
    <Templates />
  {:else if tab === 'week'}
    <Week />
  {:else}
    <Calendar refreshKey={tick} />
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
  main {
    max-width: 1100px;
    margin: 0 auto;
    padding: 1rem;
  }
</style>
