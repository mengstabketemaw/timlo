<script lang="ts">
  import {
    api,
    errText,
    fmtDur,
    hm,
    longDate,
    parseHm,
    STATUS_LABEL,
    timeInput,
    type Day,
    type DayBlock,
    type Status,
    type Template,
  } from './api'

  let {
    date = null,
    refreshKey = 0,
    onchange,
  }: { date?: string | null; refreshKey?: number; onchange?: () => void } = $props()

  let day = $state<Day | null>(null)
  let templates = $state<Template[]>([])
  let error = $state('')

  /** Block being edited: its id, 'new' for the add form, or null. */
  let editing = $state<string | null>(null)
  let form = $state({ start: '09:00', end: '10:00', title: '', note: '' })
  /** Template picked in the selector, waiting for confirmation. */
  let pendingSwitch = $state<string | null>(null)

  async function load() {
    try {
      const [d, t] = await Promise.all([api.day(date), api.templates()])
      day = d
      templates = t
      error = ''
    } catch (e) {
      error = errText(e)
    }
  }

  $effect(() => {
    void date
    void refreshKey
    load()
  })

  async function run(action: () => Promise<unknown>) {
    try {
      await action()
      error = ''
      await load()
      onchange?.()
      return true
    } catch (e) {
      error = errText(e)
      return false
    }
  }

  /** "📚 " for a block whose template block has an icon. */
  function iconOf(b: DayBlock): string {
    const icon = day?.looks[b.id]?.icon
    return icon ? `${icon} ` : ''
  }

  const current = $derived(
    day?.now_min != null
      ? day.blocks.find((b) => b.start_min <= day!.now_min! && day!.now_min! < b.end_min)
      : undefined,
  )
  const next = $derived.by(() => {
    if (!day || day.now_min == null) return undefined
    const after = current ? current.end_min : day.now_min + 1
    return day.blocks.find((b) => b.start_min >= after)
  })
  const editable = $derived(!!day && !day.is_past)
  const pendingName = $derived(templates.find((t) => t.id === pendingSwitch)?.name ?? 'no template')

  function progress(b: DayBlock): number {
    if (!day?.now_min) return 0
    return Math.round(((day.now_min - b.start_min) * 100) / (b.end_min - b.start_min))
  }

  function mark(b: DayBlock, status: Status) {
    // Tapping the active choice again clears it.
    const target: Status = b.status === status ? 'pending' : status
    run(() => api.setStatus(b.id, target))
  }

  function chooseTemplate(value: string) {
    if (!day) return
    const id = value || null
    if (id === day.template_id) return
    // Switching replaces planned blocks, so ask first when that loses something.
    if (day.is_today || day.customised) pendingSwitch = id ?? ''
    else run(() => api.switchDay(day!.date, id))
  }

  async function confirmSwitch() {
    if (!day || pendingSwitch === null) return
    const id = pendingSwitch || null
    pendingSwitch = null
    await run(() => api.switchDay(day!.date, id))
  }

  function startEdit(b: DayBlock) {
    form = { start: timeInput(b.start_min), end: timeInput(b.end_min), title: b.title, note: b.note ?? '' }
    editing = b.id
  }

  function startAdd() {
    const last = day?.blocks.at(-1)
    const start = last ? Math.min(last.end_min, 23 * 60) : 9 * 60
    form = { start: hm(start), end: timeInput(Math.min(start + 60, 24 * 60)), title: '', note: '' }
    editing = 'new'
  }

  async function saveEdit() {
    if (!day || !editing) return
    const start = parseHm(form.start)
    const end = parseHm(form.end, true)
    const ok =
      editing === 'new'
        ? await run(() =>
            api.addDayBlock(day!.date, { start_min: start, end_min: end, title: form.title, note: form.note }),
          )
        : await run(() => api.updateDayBlock(editing!, start, end, form.title, form.note || null))
    if (ok) editing = null
  }
</script>

{#if error}<p class="error">{error}</p>{/if}

{#if day}
  <header class="head">
    <div>
      <h2>{day.is_today ? 'Today' : longDate(day.date)}</h2>
      <p class="muted">
        {day.is_today ? longDate(day.date) : day.is_past ? 'Past day' : 'Upcoming'}
        {#if day.overridden}· switched from the weekly default{/if}
        {#if day.customised}· edited by hand{/if}
      </p>
    </div>
    <label class="tpl">
      <span class="muted">Template</span>
      <select
        value={day.template_id ?? ''}
        disabled={day.is_past}
        onchange={(e) => chooseTemplate(e.currentTarget.value)}
      >
        <option value="">None</option>
        {#each templates as t (t.id)}
          <option value={t.id}>{t.name}{t.id === day.default_template_id ? ' (default)' : ''}</option>
        {/each}
      </select>
    </label>
  </header>

  {#if pendingSwitch !== null}
    <div class="confirm card row">
      <span>
        {#if day.is_today}
          Switch to {pendingName}? Blocks that already ended keep their Done/Skip; the rest of today is replaced.
        {:else}
          Switch to {pendingName}? Your hand edits to this day are replaced.
        {/if}
      </span>
      <span class="spacer"></span>
      <button class="primary" onclick={confirmSwitch}>Switch</button>
      <button onclick={() => (pendingSwitch = null)}>Cancel</button>
    </div>
  {/if}

  {#if day.is_today}
    <section class="now card" class:free={!current}>
      {#if current}
        <div class="label">Now · {hm(current.start_min)}–{hm(current.end_min)}</div>
        <div class="now-title">{iconOf(current)}{current.title}</div>
        <div class="bar"><span style:width="{progress(current)}%"></span></div>
        <div class="row">
          <span class="muted">{fmtDur(current.end_min - day.now_min!)} left{next ? ` · next: ${next.title} at ${hm(next.start_min)}` : ''}</span>
          <span class="spacer"></span>
          <button class:primary={current.status === 'done'} onclick={() => mark(current!, 'done')}>Done</button>
          <button class:primary={current.status === 'skipped'} onclick={() => mark(current!, 'skipped')}>Skip</button>
        </div>
      {:else if next}
        <div class="label">Free</div>
        <div class="now-title">Free until {hm(next.start_min)} → {next.title}</div>
      {:else}
        <div class="label">Free</div>
        <div class="now-title">{day.blocks.length ? 'Nothing else planned today' : 'No plan today'}</div>
      {/if}
    </section>
  {/if}

  {#if day.blocks.length === 0 && editing !== 'new'}
    <p class="muted empty">
      {day.is_past ? 'Nothing was planned for this day.' : 'Nothing planned. Pick a template or add a block.'}
    </p>
  {/if}

  <ol class="timeline">
    {#each day.blocks as b (b.id)}
      <li class="block {b.status}" class:current={b === current} style:--block-colour={day.looks[b.id]?.colour}>
        {#if editing === b.id}
          {@render editor()}
        {:else}
          <div class="time">{hm(b.start_min)}<br /><span class="muted">{hm(b.end_min)}</span></div>
          <div class="main">
            <div class="title">{iconOf(b)}{b.title}</div>
            {#if b.note}<div class="note muted">{b.note}</div>{/if}
          </div>
          <span class="badge {b.status}">{STATUS_LABEL[b.status]}</span>
          <div class="actions">
            <button title="Done" class:primary={b.status === 'done'} onclick={() => mark(b, 'done')}>✓</button>
            <button title="Skip" class:primary={b.status === 'skipped'} onclick={() => mark(b, 'skipped')}>✗</button>
            {#if editable}
              <button class="ghost" title="Edit" onclick={() => startEdit(b)}>Edit</button>
              <button class="ghost danger" title="Delete" onclick={() => run(() => api.deleteDayBlock(b.id))}>Delete</button>
            {/if}
          </div>
        {/if}
      </li>
    {/each}
    {#if editing === 'new'}
      <li class="block">{@render editor()}</li>
    {/if}
  </ol>

  {#if editable && editing === null}
    <button onclick={startAdd}>+ Add block to this day</button>
    <p class="muted hint">Adding, moving or deleting blocks here changes this day only, not the template.</p>
  {/if}
{/if}

{#snippet editor()}
  <form
    class="editor"
    onsubmit={(e) => {
      e.preventDefault()
      saveEdit()
    }}
  >
    <input type="time" bind:value={form.start} required aria-label="Start" />
    <input type="time" bind:value={form.end} required aria-label="End" />
    <input class="grow" bind:value={form.title} placeholder="Title" required aria-label="Title" />
    <input class="grow" bind:value={form.note} placeholder="Note (optional)" aria-label="Note" />
    <button class="primary" type="submit">Save</button>
    <button type="button" onclick={() => (editing = null)}>Cancel</button>
  </form>
{/snippet}

<style>
  .head {
    display: flex;
    justify-content: space-between;
    align-items: flex-end;
    gap: 1rem;
    flex-wrap: wrap;
    margin-bottom: 1rem;
  }
  .head p {
    margin: 0.15rem 0 0;
  }
  .tpl {
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
    font-size: 0.85rem;
  }
  .confirm {
    margin-bottom: 1rem;
    border-color: var(--now);
  }
  .now {
    margin-bottom: 1rem;
    border-left: 4px solid var(--now);
  }
  .now.free {
    border-left-color: var(--accent);
  }
  .label {
    font-size: 0.8rem;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--muted);
  }
  .now-title {
    font-size: 1.4rem;
    font-weight: 600;
    margin: 0.15rem 0 0.5rem;
  }
  .bar {
    height: 6px;
    background: var(--surface-2);
    border-radius: 3px;
    overflow: hidden;
    margin-bottom: 0.6rem;
  }
  .bar span {
    display: block;
    height: 100%;
    background: var(--now);
  }
  .empty {
    margin: 1rem 0;
  }
  .timeline {
    list-style: none;
    padding: 0;
    margin: 0 0 1rem;
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }
  .block {
    border-left: 4px solid var(--block-colour, var(--border)) !important;
    display: flex;
    align-items: center;
    gap: 0.75rem;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 0.5rem 0.75rem;
    flex-wrap: wrap;
  }
  .block.current {
    border-color: var(--now);
    background: var(--now-bg);
  }
  .block.done .title,
  .block.skipped .title {
    color: var(--muted);
  }
  .block.skipped .title {
    text-decoration: line-through;
  }
  .time {
    font-variant-numeric: tabular-nums;
    font-size: 0.9rem;
    width: 3.2rem;
    line-height: 1.25;
  }
  .main {
    flex: 1;
    min-width: 8rem;
  }
  .title {
    font-weight: 500;
  }
  .note {
    font-size: 0.85rem;
  }
  .badge {
    font-size: 0.75rem;
    padding: 0.1rem 0.5rem;
    border-radius: 999px;
    background: var(--surface-2);
    color: var(--muted);
  }
  .badge.done {
    background: var(--done-bg);
    color: var(--done);
  }
  .badge.skipped {
    background: var(--skipped-bg);
    color: var(--skipped);
  }
  .badge.missed {
    background: var(--missed-bg);
    color: var(--missed);
  }
  .actions {
    display: flex;
    gap: 0.25rem;
  }
  .editor {
    display: flex;
    gap: 0.4rem;
    flex-wrap: wrap;
    width: 100%;
  }
  .editor .grow {
    flex: 1;
    min-width: 8rem;
  }
  .hint {
    font-size: 0.85rem;
  }
</style>
