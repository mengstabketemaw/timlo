<script lang="ts">
  import DayView from './DayView.svelte'
  import { api, errText, isoDate, parseIso, type MonthDay } from './api'

  let { refreshKey = 0 }: { refreshKey?: number } = $props()

  const today = isoDate(new Date())
  let year = $state(new Date().getFullYear())
  let month = $state(new Date().getMonth() + 1) // 1–12
  let days = $state<MonthDay[]>([])
  let selected = $state(today)
  let error = $state('')

  async function load() {
    try {
      days = await api.month(year, month)
      error = ''
    } catch (e) {
      error = errText(e)
    }
  }

  $effect(() => {
    void year
    void month
    void refreshKey
    load()
  })

  function shift(delta: number) {
    const d = new Date(year, month - 1 + delta, 1)
    year = d.getFullYear()
    month = d.getMonth() + 1
  }

  function goToday() {
    const d = new Date()
    year = d.getFullYear()
    month = d.getMonth() + 1
    selected = today
  }

  /** Empty cells before the 1st so weeks start on Monday. */
  const lead = $derived((new Date(year, month - 1, 1).getDay() + 6) % 7)
  const title = $derived(new Date(year, month - 1, 1).toLocaleDateString(undefined, { month: 'long', year: 'numeric' }))
</script>

{#if error}<p class="error">{error}</p>{/if}

<div class="layout">
  <section class="card cal">
    <div class="row">
      <button onclick={() => shift(-1)} aria-label="Previous month">‹</button>
      <h3 class="title">{title}</h3>
      <button onclick={() => shift(1)} aria-label="Next month">›</button>
      <span class="spacer"></span>
      <button onclick={goToday}>Today</button>
    </div>
    <div class="grid">
      {#each ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'] as w}
        <div class="wd muted">{w}</div>
      {/each}
      {#each Array(lead) as _, i (i)}
        <div></div>
      {/each}
      {#each days as d (d.date)}
        <button
          class="cell"
          class:today={d.date === today}
          class:selected={d.date === selected}
          class:past={d.date < today}
          onclick={() => (selected = d.date)}
        >
          <span class="num">{parseIso(d.date).getDate()}</span>
          <span class="tname" class:override={d.overridden}>{d.template_name ?? ''}</span>
          {#if d.exists && d.total > 0 && d.date <= today}
            <span class="score" title="{d.done} done, {d.skipped} skipped, {d.missed} missed">
              {d.done}/{d.total}
            </span>
          {/if}
        </button>
      {/each}
    </div>
    <p class="muted legend"><span class="override">Italic</span> = switched from the weekly default.</p>
  </section>

  <section class="day">
    {#key selected}
      <DayView date={selected} {refreshKey} onchange={load} />
    {/key}
  </section>
</div>

<style>
  .layout {
    display: grid;
    grid-template-columns: minmax(300px, 420px) 1fr;
    gap: 1rem;
    align-items: start;
  }
  @media (max-width: 820px) {
    .layout {
      grid-template-columns: 1fr;
    }
  }
  .title {
    min-width: 9rem;
    text-align: center;
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(7, 1fr);
    gap: 3px;
    margin-top: 0.75rem;
  }
  .wd {
    font-size: 0.75rem;
    text-align: center;
  }
  .cell {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 0.1rem;
    min-height: 3.6rem;
    padding: 0.25rem 0.3rem;
    border-radius: 6px;
    overflow: hidden;
    text-align: left;
  }
  .cell.past {
    background: var(--surface-2);
  }
  .cell.today {
    border-color: var(--now);
  }
  .cell.selected {
    outline: 2px solid var(--accent);
    outline-offset: -1px;
  }
  .num {
    font-weight: 600;
    font-size: 0.9rem;
  }
  .tname {
    font-size: 0.68rem;
    color: var(--muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 100%;
  }
  .override {
    font-style: italic;
  }
  .score {
    font-size: 0.68rem;
    color: var(--done);
    font-variant-numeric: tabular-nums;
  }
  .legend {
    font-size: 0.8rem;
    margin: 0.5rem 0 0;
  }
</style>
