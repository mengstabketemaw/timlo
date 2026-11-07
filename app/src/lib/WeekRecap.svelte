<script lang="ts">
  import { api, errText, fmtDur, isoDate, parseIso, type WeekSummary } from './api'

  let { refreshKey = 0 }: { refreshKey?: number } = $props()

  /** 0 = this week, -1 = last week … */
  let offset = $state(0)
  let w = $state<WeekSummary | null>(null)
  let error = $state('')

  $effect(() => {
    void refreshKey
    const d = new Date()
    d.setDate(d.getDate() + offset * 7)
    api.weekSummary(isoDate(d)).then(
      (s) => {
        w = s
        error = ''
      },
      (e) => (error = errText(e)),
    )
  })

  const pct = $derived(w && w.planned ? Math.round((w.done / w.planned) * 100) : null)
  const delta = $derived(pct !== null && w?.prev_pct != null ? pct - w.prev_pct : null)
  const range = $derived(
    w
      ? `${parseIso(w.from).toLocaleDateString(undefined, { day: 'numeric', month: 'short' })} – ${parseIso(w.to).toLocaleDateString(undefined, { day: 'numeric', month: 'short' })}`
      : '',
  )
  const label = $derived(offset === 0 ? 'This week' : offset === -1 ? 'Last week' : range)
  const weekday = (d: string) => parseIso(d).toLocaleDateString(undefined, { weekday: 'long' })
</script>

<section class="card recap">
  <div class="row head">
    <button class="ghost" onclick={() => offset--} aria-label="Previous week">‹</button>
    <h3>{label}</h3>
    <button class="ghost" onclick={() => offset++} disabled={offset >= 0} aria-label="Next week">›</button>
    <span class="spacer"></span>
    <span class="muted range">{range}</span>
  </div>

  {#if error}
    <p class="error">{error}</p>
  {:else if w && (w.planned || w.focus_min)}
    <div class="figures">
      <div>
        <span class="big">{pct ?? '–'}{pct !== null ? '%' : ''}</span>
        <span class="muted">{w.done}/{w.planned} blocks done</span>
        {#if delta !== null}
          <span class="delta" class:up={delta >= 0} class:down={delta < 0}>
            {delta >= 0 ? '▲' : '▼'}
            {Math.abs(delta)} vs week before
          </span>
        {/if}
      </div>
      <div>
        <span class="big">{w.focus_min ? fmtDur(w.focus_min) : '0m'}</span>
        <span class="muted">focused</span>
      </div>
    </div>
    <ul class="notes">
      {#if w.best_day}<li><span class="k">Best day</span> {weekday(w.best_day)}</li>{/if}
      {#if w.top_done}<li><span class="k">Most done</span> {w.top_done.title} ×{w.top_done.count}</li>{/if}
      {#if w.most_dropped}<li><span class="k">Slipped most</span> {w.most_dropped.title} ×{w.most_dropped.count}</li>{/if}
      {#if w.skipped || w.missed}
        <li><span class="k">Not done</span> {w.skipped} skipped · {w.missed} missed</li>
      {/if}
    </ul>
  {:else}
    <p class="muted empty">Nothing planned or focused this week.</p>
  {/if}
</section>

<style>
  .recap {
    margin-bottom: 1rem;
  }
  .head h3 {
    min-width: 6.5rem;
    text-align: center;
  }
  .range {
    font-size: 0.8rem;
  }
  .figures {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0.75rem;
    margin: 0.6rem 0 0.4rem;
  }
  .figures > div {
    display: flex;
    flex-direction: column;
    font-size: 0.8rem;
  }
  .big {
    font-size: 1.6rem;
    font-weight: 600;
    line-height: 1.2;
    font-variant-numeric: tabular-nums;
  }
  .delta {
    font-size: 0.75rem;
  }
  .up {
    color: var(--done);
  }
  .down {
    color: var(--missed);
  }
  .notes {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
    font-size: 0.88rem;
  }
  .k {
    display: inline-block;
    min-width: 6.5rem;
    color: var(--muted);
  }
  .empty {
    margin: 0.5rem 0 0;
  }
</style>
