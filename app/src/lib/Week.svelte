<script lang="ts">
  import { api, errText, WEEKDAYS, type Template } from './api'

  let templates = $state<Template[]>([])
  let week = $state<(string | null)[]>([])
  let error = $state('')

  async function load() {
    try {
      ;[templates, week] = await Promise.all([api.templates(), api.week()])
      error = ''
    } catch (e) {
      error = errText(e)
    }
  }

  $effect(() => {
    load()
  })

  async function set(weekday: number, value: string) {
    try {
      await api.setWeek(weekday, value || null)
      await load()
    } catch (e) {
      error = errText(e)
    }
  }

  function summary(id: string | null): string {
    const t = templates.find((x) => x.id === id)
    if (!t) return ''
    return t.blocks.map((b) => b.title).join(' · ')
  }
</script>

{#if error}<p class="error">{error}</p>{/if}

<h2>Week</h2>
<p class="muted">The default template for each weekday. Changing one updates upcoming days that still follow the default.</p>

<div class="card">
  {#each WEEKDAYS as name, i}
    <div class="day">
      <span class="name">{name}</span>
      <select value={week[i] ?? ''} onchange={(e) => set(i + 1, e.currentTarget.value)} aria-label="{name} template">
        <option value="">None</option>
        {#each templates as t (t.id)}
          <option value={t.id}>{t.name}</option>
        {/each}
      </select>
      <span class="muted blocks">{summary(week[i] ?? null)}</span>
    </div>
  {/each}
</div>

<style>
  h2 {
    margin-bottom: 0.25rem;
  }
  p {
    margin: 0 0 1rem;
  }
  .day {
    display: grid;
    grid-template-columns: 7rem minmax(9rem, 14rem) 1fr;
    gap: 0.75rem;
    align-items: center;
    padding: 0.4rem 0;
    border-bottom: 1px solid var(--border);
  }
  .day:last-child {
    border-bottom: none;
  }
  .name {
    font-weight: 500;
  }
  .blocks {
    font-size: 0.85rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  @media (max-width: 560px) {
    .day {
      grid-template-columns: 6rem 1fr;
    }
    .blocks {
      grid-column: 1 / -1;
    }
  }
</style>
