<script lang="ts">
  import { api, errText, isoDate, parseIso, type ActivityDay } from './api'

  let { refreshKey = 0 }: { refreshKey?: number } = $props()

  const WEEKS = 53
  let byDate = $state<Map<string, ActivityDay>>(new Map())
  let error = $state('')
  let hover = $state<string | null>(null)
  let range = $state<30 | 90>(30)
  let scroller = $state<HTMLDivElement>()
  let chartW = $state(600)

  const today = $derived.by(() => {
    void refreshKey
    return new Date()
  })
  const todayIso = $derived(isoDate(today))

  /** Monday of the first column. */
  const start = $derived.by(() => {
    const d = new Date(today.getFullYear(), today.getMonth(), today.getDate())
    d.setDate(d.getDate() - ((d.getDay() + 6) % 7) - (WEEKS - 1) * 7)
    return d
  })

  $effect(() => {
    const from = isoDate(start)
    void refreshKey
    api.activity(from, todayIso).then(
      (rows) => {
        byDate = new Map(rows.map((r) => [r.date, r]))
        error = ''
      },
      (e) => (error = errText(e)),
    )
  })

  // Show the latest weeks first on narrow screens.
  $effect(() => {
    void byDate
    if (scroller) scroller.scrollLeft = scroller.scrollWidth
  })

  function addDays(d: Date, n: number): Date {
    const x = new Date(d)
    x.setDate(x.getDate() + n)
    return x
  }

  const weeks = $derived(
    Array.from({ length: WEEKS }, (_, w) => Array.from({ length: 7 }, (_, d) => isoDate(addDays(start, w * 7 + d)))),
  )

  /** Month labels at the first column whose Monday is in a new month. */
  const monthLabels = $derived(
    weeks.map((col, i) => {
      const m = parseIso(col[0]).getMonth()
      const prev = i ? parseIso(weeks[i - 1][0]).getMonth() : -1
      return m !== prev && i < WEEKS - 1 ? parseIso(col[0]).toLocaleDateString(undefined, { month: 'short' }) : ''
    }),
  )

  function ratio(a: ActivityDay | undefined): number | null {
    return a && a.total ? a.done / a.total : null
  }

  function level(date: string): number {
    const r = ratio(byDate.get(date))
    if (r === null) return -1
    if (r === 0) return 0
    if (r < 0.25) return 1
    if (r < 0.5) return 2
    if (r < 0.75) return 3
    return 4
  }

  /** A day "counts" when at least half its blocks were done. */
  const good = (date: string) => (ratio(byDate.get(date)) ?? 0) >= 0.5

  const streaks = $derived.by(() => {
    let best = 0
    let run = 0
    for (let d = new Date(start); isoDate(d) <= todayIso; d = addDays(d, 1)) {
      run = good(isoDate(d)) ? run + 1 : 0
      best = Math.max(best, run)
    }
    // Today still in progress doesn't break the current streak.
    let cur = 0
    let d = good(todayIso) ? today : addDays(today, -1)
    while (good(isoDate(d))) {
      cur++
      d = addDays(d, -1)
    }
    return { cur, best }
  })

  function sumRange(fromDaysAgo: number, toDaysAgo: number) {
    let done = 0
    let total = 0
    for (let i = fromDaysAgo; i >= toDaysAgo; i--) {
      const a = byDate.get(isoDate(addDays(today, -i)))
      if (a) {
        done += a.done
        total += a.total
      }
    }
    return { done, total, pct: total ? Math.round((done / total) * 100) : null }
  }
  const thisWeek = $derived(sumRange(6, 0))
  const lastWeek = $derived(sumRange(13, 7))
  const yearDone = $derived([...byDate.values()].reduce((n, a) => n + a.done, 0))
  const yearFocusH = $derived(Math.round([...byDate.values()].reduce((n, a) => n + a.focus_min, 0) / 6) / 10)

  // ---- Progress chart -----------------------------------------------------
  const series = $derived.by(() => {
    const out: { date: string; pct: number | null; avg: number | null; a?: ActivityDay }[] = []
    for (let i = range - 1; i >= 0; i--) {
      const date = isoDate(addDays(today, -i))
      const a = byDate.get(date)
      const r = ratio(a)
      // 7-day completion, weighting each day by its number of blocks.
      let done = 0
      let total = 0
      for (let k = 0; k < 7; k++) {
        const b = byDate.get(isoDate(addDays(today, -i - k)))
        if (b) {
          done += b.done
          total += b.total
        }
      }
      out.push({ date, pct: r === null ? null : r * 100, avg: total ? (done / total) * 100 : null, a })
    }
    return out
  })

  const H = 150
  const PAD = { l: 34, r: 8, t: 10, b: 22 }
  const plotW = $derived(Math.max(100, chartW - PAD.l - PAD.r))
  const plotH = H - PAD.t - PAD.b
  const slot = $derived(plotW / range)
  const barW = $derived(Math.max(2, slot - 2))
  const y = (p: number) => PAD.t + plotH * (1 - p / 100)
  const xMid = (i: number) => PAD.l + slot * i + slot / 2

  const avgPath = $derived.by(() => {
    let d = ''
    let pen = false
    series.forEach((p, i) => {
      if (p.avg === null) {
        pen = false
        return
      }
      d += `${pen ? 'L' : 'M'}${xMid(i).toFixed(1)},${y(p.avg).toFixed(1)}`
      pen = true
    })
    return d
  })

  /** Rounded top, square bottom on the baseline. */
  function bar(x: number, top: number, w: number, bottom: number): string {
    const r = Math.min(4, w / 2, bottom - top)
    if (bottom - top < 0.5) return ''
    return `M${x},${bottom}V${top + r}Q${x},${top} ${x + r},${top}H${x + w - r}Q${x + w},${top} ${x + w},${top + r}V${bottom}Z`
  }

  let hoverI = $state<number | null>(null)

  function describe(date: string): string {
    const a = byDate.get(date)
    const day = parseIso(date).toLocaleDateString(undefined, { weekday: 'short', day: 'numeric', month: 'short' })
    const focus = a?.focus_min ? `${a.focus_min >= 60 ? `${Math.floor(a.focus_min / 60)}h ` : ''}${a.focus_min % 60}m focus` : ''
    if (!a || !a.total) return focus ? `${day} · ${focus}` : `${day} · nothing planned`
    const bits = [`${a.done}/${a.total} done`]
    if (a.skipped) bits.push(`${a.skipped} skipped`)
    if (a.missed) bits.push(`${a.missed} missed`)
    if (focus) bits.push(focus)
    return `${day} · ${bits.join(' · ')}`
  }

  const tickEvery = $derived(range === 30 ? 7 : 14)
</script>

{#if error}<p class="error">{error}</p>{/if}

<section class="card activity">
  <div class="stats">
    <div class="stat">
      <span class="big">{streaks.cur}</span>
      <span class="muted">day streak</span>
    </div>
    <div class="stat">
      <span class="big">{streaks.best}</span>
      <span class="muted">best streak</span>
    </div>
    <div class="stat">
      <span class="big">{thisWeek.pct ?? '–'}{thisWeek.pct !== null ? '%' : ''}</span>
      <span class="muted">
        this week
        {#if thisWeek.pct !== null && lastWeek.pct !== null}
          <span class:up={thisWeek.pct >= lastWeek.pct} class:down={thisWeek.pct < lastWeek.pct}>
            {thisWeek.pct >= lastWeek.pct ? '▲' : '▼'}{Math.abs(thisWeek.pct - lastWeek.pct)}
          </span>
        {/if}
      </span>
    </div>
    <div class="stat">
      <span class="big">{yearDone}</span>
      <span class="muted">blocks done{yearFocusH ? ` · ${yearFocusH}h focus` : ''}, past year</span>
    </div>
  </div>

  <div class="heat-wrap" bind:this={scroller}>
    <div class="heat" style="--weeks: {WEEKS}">
      <div class="months">
        {#each monthLabels as m, i (i)}<span>{m}</span>{/each}
      </div>
      <div class="days muted" aria-hidden="true">
        <span>Mon</span><span></span><span>Wed</span><span></span><span>Fri</span><span></span><span></span>
      </div>
      <div class="cells" role="grid" aria-label="Activity, past year">
        {#each weeks as col, w (w)}
          {#each col as date, d (date)}
            {#if date <= todayIso}
              <button
                class="cell l{level(date)}"
                class:today={date === todayIso}
                style="grid-column: {w + 1}; grid-row: {d + 1}"
                title={describe(date)}
                aria-label={describe(date)}
                onmouseenter={() => (hover = date)}
                onfocus={() => (hover = date)}
                onclick={() => (hover = date)}
              ></button>
            {/if}
          {/each}
        {/each}
      </div>
    </div>
  </div>

  <div class="heat-foot">
    <span class="readout">{hover ? describe(hover) : 'Each square is a day: the darker, the more blocks you finished.'}</span>
    <span class="scale muted" aria-hidden="true">
      Less <i class="cell l0"></i><i class="cell l1"></i><i class="cell l2"></i><i class="cell l3"></i><i class="cell l4"></i> More
    </span>
  </div>
</section>

<section class="card chart-card">
  <div class="row">
    <h3>Progress</h3>
    <span class="spacer"></span>
    <div class="seg" role="radiogroup" aria-label="Range">
      <button role="radio" aria-checked={range === 30} class:on={range === 30} onclick={() => (range = 30)}>30 days</button>
      <button role="radio" aria-checked={range === 90} class:on={range === 90} onclick={() => (range = 90)}>90 days</button>
    </div>
  </div>
  <div class="legend muted">
    <span><i class="key bar-key"></i>Blocks done that day</span>
    <span><i class="key line-key"></i>7-day average</span>
  </div>

  <div class="chart" bind:clientWidth={chartW}>
    <svg width={chartW} height={H} role="img" aria-label="Share of blocks done per day, last {range} days" onmouseleave={() => (hoverI = null)}>
      {#each [0, 50, 100] as g}
        <line class="grid" x1={PAD.l} x2={PAD.l + plotW} y1={y(g)} y2={y(g)} />
        <text class="tick" x={PAD.l - 6} y={y(g) + 4} text-anchor="end">{g}%</text>
      {/each}
      {#each series as p, i (p.date)}
        {#if p.pct !== null}
          <path class="bar" class:dim={hoverI !== null && hoverI !== i} d={bar(PAD.l + slot * i + (slot - barW) / 2, y(p.pct), barW, y(0))} />
        {/if}
        {#if i % tickEvery === (range - 1) % tickEvery}
          <text class="tick" x={xMid(i)} y={H - 6} text-anchor="middle">
            {parseIso(p.date).toLocaleDateString(undefined, { day: 'numeric', month: 'short' })}
          </text>
        {/if}
      {/each}
      <path class="avg" d={avgPath} />
      {#if hoverI !== null}
        <line class="cross" x1={xMid(hoverI)} x2={xMid(hoverI)} y1={PAD.t} y2={y(0)} />
        {#if series[hoverI].avg !== null}
          <circle class="dot" cx={xMid(hoverI)} cy={y(series[hoverI].avg!)} r="4" />
        {/if}
      {/if}
      {#each series as _, i (i)}
        <rect
          class="hit"
          x={PAD.l + slot * i}
          y={PAD.t}
          width={slot}
          height={plotH}
          role="presentation"
          onmouseenter={() => (hoverI = i)}
          onclick={() => (hoverI = i)}
        />
      {/each}
    </svg>
  </div>
  <p class="readout">
    {#if hoverI !== null}
      {describe(series[hoverI].date)}{series[hoverI].avg !== null ? ` · 7-day ${Math.round(series[hoverI].avg!)}%` : ''}
    {:else}
      Hover or tap a day for details.
    {/if}
  </p>
</section>

<style>
  .activity,
  .chart-card {
    margin-bottom: 1rem;
    --l0: color-mix(in srgb, var(--border) 70%, var(--surface));
    --l1: color-mix(in srgb, var(--done) 30%, var(--surface));
    --l2: color-mix(in srgb, var(--done) 52%, var(--surface));
    --l3: color-mix(in srgb, var(--done) 76%, var(--surface));
    --l4: var(--done);
  }
  .stats {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 0.5rem;
    margin-bottom: 0.9rem;
  }
  @media (max-width: 560px) {
    .stats {
      grid-template-columns: repeat(2, 1fr);
    }
  }
  .stat {
    display: flex;
    flex-direction: column;
    font-size: 0.78rem;
  }
  .big {
    font-size: 1.5rem;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    line-height: 1.2;
  }
  .up {
    color: var(--done);
  }
  .down {
    color: var(--missed);
  }
  .heat-wrap {
    overflow-x: auto;
    padding-bottom: 0.25rem;
  }
  .heat {
    --c: 11px;
    --g: 3px;
    display: grid;
    grid-template-columns: auto auto;
    grid-template-areas: '. months' 'days cells';
    gap: 4px 6px;
    width: max-content;
  }
  .months {
    grid-area: months;
    display: grid;
    grid-template-columns: repeat(var(--weeks), var(--c));
    column-gap: var(--g);
    font-size: 0.68rem;
    color: var(--muted);
    white-space: nowrap;
  }
  .days {
    grid-area: days;
    display: grid;
    grid-template-rows: repeat(7, var(--c));
    row-gap: var(--g);
    font-size: 0.64rem;
    line-height: var(--c);
  }
  .cells {
    grid-area: cells;
    display: grid;
    grid-template-columns: repeat(var(--weeks), var(--c));
    grid-template-rows: repeat(7, var(--c));
    gap: var(--g);
  }
  .cell {
    display: inline-block;
    width: var(--c, 11px);
    height: var(--c, 11px);
    padding: 0;
    border: none;
    border-radius: 2px;
    background: transparent;
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--border) 60%, transparent);
  }
  .cell:hover {
    outline: 1px solid var(--text);
  }
  .cell.l0 {
    background: var(--l0);
    box-shadow: none;
  }
  .cell.l1 {
    background: var(--l1);
    box-shadow: none;
  }
  .cell.l2 {
    background: var(--l2);
    box-shadow: none;
  }
  .cell.l3 {
    background: var(--l3);
    box-shadow: none;
  }
  .cell.l4 {
    background: var(--l4);
    box-shadow: none;
  }
  .cell.today {
    outline: 1.5px solid var(--now);
    outline-offset: 1px;
  }
  .heat-foot {
    display: flex;
    flex-wrap: wrap;
    justify-content: space-between;
    gap: 0.4rem 1rem;
    margin-top: 0.5rem;
    font-size: 0.78rem;
  }
  .scale {
    display: inline-flex;
    align-items: center;
    gap: 3px;
  }
  .scale .cell {
    --c: 10px;
  }
  .readout {
    font-size: 0.8rem;
    margin: 0.25rem 0 0;
    min-height: 1.2em;
  }
  .seg {
    display: flex;
    padding: 0.15rem;
    border-radius: 7px;
    background: var(--surface-2);
  }
  .seg button {
    border: none;
    background: transparent;
    font-size: 0.8rem;
    padding: 0.2rem 0.6rem;
  }
  .seg button.on {
    background: var(--surface);
    font-weight: 600;
  }
  .legend {
    display: flex;
    flex-wrap: wrap;
    gap: 0.3rem 1rem;
    font-size: 0.78rem;
    margin: 0.4rem 0 0.25rem;
  }
  .key {
    display: inline-block;
    vertical-align: middle;
    margin-right: 0.35rem;
  }
  .bar-key {
    width: 10px;
    height: 10px;
    border-radius: 2px;
    background: var(--l2);
  }
  .line-key {
    width: 14px;
    height: 2px;
    background: var(--accent);
  }
  .chart {
    width: 100%;
  }
  svg {
    display: block;
    overflow: visible;
  }
  .grid {
    stroke: var(--border);
    stroke-width: 1;
    stroke-dasharray: 2 3;
  }
  .tick {
    fill: var(--muted);
    font-size: 10px;
    font-variant-numeric: tabular-nums;
  }
  .bar {
    fill: var(--l2);
    transition: opacity 0.15s;
  }
  .bar.dim {
    opacity: 0.45;
  }
  .avg {
    fill: none;
    stroke: var(--accent);
    stroke-width: 2;
    stroke-linejoin: round;
    stroke-linecap: round;
  }
  .cross {
    stroke: var(--muted);
    stroke-width: 1;
  }
  .dot {
    fill: var(--accent);
    stroke: var(--surface);
    stroke-width: 2;
  }
  .hit {
    fill: transparent;
    cursor: crosshair;
  }
</style>
