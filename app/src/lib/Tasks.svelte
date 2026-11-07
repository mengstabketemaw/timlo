<script lang="ts">
  import { ui } from './ui.svelte'
  import { flip } from 'svelte/animate'
  import { fade, slide } from 'svelte/transition'
  import { api, errText, type Task } from './api'

  let { refreshKey = 0 }: { refreshKey?: number } = $props()

  const SHOW_DONE_KEY = 'timlo.tasks.showDone'
  function readShowDone(): boolean {
    try {
      return localStorage.getItem(SHOW_DONE_KEY) === '1'
    } catch {
      return false
    }
  }

  let tasks = $state<Task[]>([])
  let doneCount = $state(0)
  let showDone = $state(readShowDone())
  let loaded = $state(false)
  let error = $state('')

  let newTitle = $state('')
  let newInput = $state<HTMLInputElement>()

  // Quick add (Waybar right-click, the Android widget): focus the box.
  $effect(() => {
    if (ui.quickAdd && newInput) {
      newInput.focus()
    }
  })
  let openId = $state<string | null>(null)
  let draft = $state({ title: '', notes: '' })
  let newStep = $state('')
  let confirmDelete = $state(false)
  /** Ticked just now: shown checked for a moment before it hides. */
  let leaving = $state(new Set<string>())

  async function load() {
    try {
      const r = await api.tasks(showDone)
      tasks = r.tasks
      doneCount = r.done_count
      loaded = true
      error = ''
      if (openId && !tasks.some((t) => t.id === openId)) openId = null
    } catch (e) {
      error = errText(e)
    }
  }

  $effect(() => {
    refreshKey
    showDone
    load()
  })

  async function run(action: () => Promise<unknown>) {
    try {
      await action()
      await load()
    } catch (e) {
      error = errText(e)
    }
  }

  function toggleShowDone() {
    showDone = !showDone
    try {
      localStorage.setItem(SHOW_DONE_KEY, showDone ? '1' : '0')
    } catch {}
  }

  async function add(e: SubmitEvent) {
    e.preventDefault()
    const title = newTitle.trim()
    if (!title) return
    newTitle = ''
    await run(() => api.addTask(title))
  }

  async function toggleDone(t: Task) {
    const done = !t.done
    try {
      await api.setTaskDone(t.id, done)
    } catch (e) {
      error = errText(e)
      return
    }
    if (done && !showDone) {
      // Let the tick show, then let it slide away.
      leaving = new Set(leaving).add(t.id)
      if (openId === t.id) openId = null
      setTimeout(async () => {
        await load()
        leaving.delete(t.id)
        leaving = new Set(leaving)
      }, 450)
    } else {
      await load()
    }
  }

  async function open(t: Task) {
    if (openId === t.id) {
      await saveDraft()
      openId = null
      return
    }
    await saveDraft()
    openId = t.id
    draft = { title: t.title, notes: t.notes ?? '' }
    newStep = ''
    confirmDelete = false
  }

  let saveTimer: ReturnType<typeof setTimeout> | undefined
  function queueSave() {
    clearTimeout(saveTimer)
    saveTimer = setTimeout(saveDraft, 700)
  }

  async function saveDraft() {
    clearTimeout(saveTimer)
    const t = tasks.find((x) => x.id === openId)
    if (!t || !draft.title.trim()) return
    if (draft.title.trim() === t.title && (draft.notes.trim() || null) === (t.notes?.trim() || null)) return
    try {
      await api.updateTask(t.id, draft.title, draft.notes || null)
      t.title = draft.title.trim()
      t.notes = draft.notes.trim() ? draft.notes : null
    } catch (e) {
      error = errText(e)
    }
  }

  async function addStep(e: SubmitEvent, t: Task) {
    e.preventDefault()
    const title = newStep.trim()
    if (!title) return
    newStep = ''
    await run(() => api.addTask(title, t.id))
  }

  function renameStep(s: Task, value: string) {
    if (value.trim() && value.trim() !== s.title) run(() => api.updateTask(s.id, value, null))
  }

  function autosize(el: HTMLTextAreaElement) {
    const fit = () => {
      el.style.height = 'auto'
      el.style.height = `${el.scrollHeight + 2}px`
    }
    fit()
    el.addEventListener('input', fit)
    return { destroy: () => el.removeEventListener('input', fit) }
  }

  function added(ms: number): string {
    return new Date(ms).toLocaleDateString(undefined, { day: 'numeric', month: 'short' })
  }

  const openCount = $derived(tasks.filter((t) => !t.done).length)
</script>

{#if error}<p class="error">{error}</p>{/if}

<header class="head">
  <div>
    <h2>Tasks</h2>
    <p class="muted">
      {openCount} open{doneCount ? ` · ${doneCount} done` : ''}
    </p>
  </div>
  <button class="toggle" class:on={showDone} onclick={toggleShowDone} aria-pressed={showDone}>
    {showDone ? 'Hide done' : 'Show done'}
  </button>
</header>

<form class="add" onsubmit={add}>
  <span class="plus" aria-hidden="true">+</span>
  <input bind:this={newInput} bind:value={newTitle} placeholder="Add a task…" aria-label="New task" enterkeyhint="done" />
</form>

{#if loaded && tasks.length === 0}
  <div class="empty muted">
    {#if doneCount && !showDone}
      All clear. {doneCount} done task{doneCount === 1 ? '' : 's'} hidden.
    {:else}
      Nothing here yet. Jot down whatever's on your mind.
    {/if}
  </div>
{/if}

<ul class="tasks">
  {#each tasks as t (t.id)}
    {@const isDone = t.done || leaving.has(t.id)}
    {@const stepsDone = t.subtasks.filter((s) => s.done).length}
    <li class="task" class:done={isDone} class:open={openId === t.id} animate:flip={{ duration: 200 }} out:slide={{ duration: 200 }}>
      <div class="line">
        <button class="check" class:on={isDone} onclick={() => toggleDone(t)} aria-label={isDone ? 'Mark not done' : 'Mark done'}>
          <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M3.5 8.5l3 3 6-7" /></svg>
        </button>
        <button class="summary" onclick={() => open(t)} aria-expanded={openId === t.id}>
          <span class="title">{t.title}</span>
          <span class="meta">
            {#if t.subtasks.length}<span class="pill" class:complete={stepsDone === t.subtasks.length}>{stepsDone}/{t.subtasks.length}</span>{/if}
            {#if t.notes}<span class="notes-mark" title="Has details">¶</span>{/if}
            <span class="chev" aria-hidden="true">›</span>
          </span>
        </button>
      </div>

      {#if openId === t.id}
        <div class="detail" transition:slide={{ duration: 160 }}>
          <input class="title-edit" bind:value={draft.title} oninput={queueSave} onblur={saveDraft} aria-label="Title" />
          <textarea
            use:autosize
            bind:value={draft.notes}
            oninput={queueSave}
            onblur={saveDraft}
            placeholder="Details, links, ideas…"
            rows="3"
            aria-label="Details"
          ></textarea>

          <div class="steps">
            {#each t.subtasks as s (s.id)}
              <div class="step" class:done={s.done} transition:fade={{ duration: 120 }}>
                <button class="check small" class:on={s.done} onclick={() => run(() => api.setTaskDone(s.id, !s.done))} aria-label={s.done ? 'Mark step not done' : 'Mark step done'}>
                  <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M3.5 8.5l3 3 6-7" /></svg>
                </button>
                <input class="step-title" value={s.title} onchange={(e) => renameStep(s, e.currentTarget.value)} aria-label="Step" />
                <button class="ghost remove" onclick={() => run(() => api.deleteTask(s.id))} aria-label="Remove step">✕</button>
              </div>
            {/each}
            <form class="step add-step" onsubmit={(e) => addStep(e, t)}>
              <span class="plus small" aria-hidden="true">+</span>
              <input class="step-title" bind:value={newStep} placeholder="Add a step" aria-label="New step" enterkeyhint="done" />
            </form>
          </div>

          <div class="foot">
            <span class="muted">Added {added(t.created_at)}{t.done && t.done_at ? ` · done ${added(t.done_at)}` : ''}</span>
            <span class="spacer"></span>
            {#if confirmDelete}
              <button class="danger" onclick={() => run(() => api.deleteTask(t.id))}>Delete task</button>
              <button onclick={() => (confirmDelete = false)}>Keep</button>
            {:else}
              <button class="ghost danger" onclick={() => (confirmDelete = true)}>Delete</button>
            {/if}
          </div>
        </div>
      {/if}
    </li>
  {/each}
</ul>

<style>
  .head {
    display: flex;
    justify-content: space-between;
    align-items: flex-end;
    gap: 1rem;
    margin-bottom: 0.75rem;
  }
  .head h2 {
    margin: 0;
  }
  .head p {
    margin: 0.1rem 0 0;
    font-size: 0.9rem;
  }
  .toggle {
    border-radius: 999px;
    font-size: 0.85rem;
  }
  .toggle.on {
    background: var(--surface-2);
    font-weight: 600;
  }

  .add {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 0.35rem 0.6rem;
    margin-bottom: 0.9rem;
  }
  .add:focus-within {
    border-color: var(--accent);
  }
  .add input {
    flex: 1;
    border: none;
    background: transparent;
    padding: 0.35rem 0;
    font-size: 1rem;
  }
  .add input:focus,
  .step-title:focus,
  .title-edit:focus,
  textarea:focus {
    outline: none;
  }
  .plus {
    color: var(--accent);
    font-size: 1.3rem;
    line-height: 1;
    width: 1.5rem;
    text-align: center;
  }
  .plus.small {
    font-size: 1.05rem;
    width: 1.25rem;
  }

  .empty {
    text-align: center;
    padding: 2rem 1rem;
  }

  .tasks {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: 0.45rem;
  }
  .task {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    transition: border-color 0.15s, box-shadow 0.15s;
  }
  .task.open {
    border-color: var(--accent);
    box-shadow: 0 2px 10px rgb(0 0 0 / 0.08);
  }
  .line {
    display: flex;
    align-items: center;
    gap: 0.25rem;
    padding: 0.2rem 0.4rem 0.2rem 0.55rem;
  }
  .summary {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 0.5rem;
    min-width: 0;
    border: none;
    background: transparent;
    padding: 0.55rem 0.25rem;
    text-align: left;
  }
  .summary:hover:not(:disabled) {
    background: transparent;
  }
  .title {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    transition: color 0.2s;
  }
  .task.done .title {
    color: var(--muted);
    text-decoration: line-through;
  }
  .meta {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    color: var(--muted);
    font-size: 0.8rem;
  }
  .pill {
    background: var(--surface-2);
    border-radius: 999px;
    padding: 0.05rem 0.45rem;
    font-variant-numeric: tabular-nums;
  }
  .pill.complete {
    background: var(--done-bg);
    color: var(--done);
  }
  .chev {
    font-size: 1.2rem;
    line-height: 1;
    transition: transform 0.15s;
  }
  .task.open .chev {
    transform: rotate(90deg);
  }

  .check {
    flex: none;
    width: 1.4rem;
    height: 1.4rem;
    padding: 0;
    border-radius: 50%;
    border: 2px solid var(--muted);
    background: transparent;
    display: grid;
    place-items: center;
    transition: background 0.15s, border-color 0.15s;
  }
  .check.small {
    width: 1.15rem;
    height: 1.15rem;
    border-radius: 5px;
  }
  .check svg {
    width: 70%;
    height: 70%;
    fill: none;
    stroke: var(--surface);
    stroke-width: 2.4;
    stroke-linecap: round;
    stroke-linejoin: round;
    stroke-dasharray: 16;
    stroke-dashoffset: 16;
    transition: stroke-dashoffset 0.25s ease-out;
  }
  .check:hover:not(:disabled) {
    border-color: var(--done);
    background: transparent;
  }
  .check.on,
  .check.on:hover:not(:disabled) {
    background: var(--done);
    border-color: var(--done);
  }
  .check.on svg {
    stroke-dashoffset: 0;
  }

  .detail {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    padding: 0 0.9rem 0.75rem 2.6rem;
  }
  .title-edit {
    border: none;
    border-bottom: 1px solid var(--border);
    border-radius: 0;
    background: transparent;
    padding: 0.3rem 0;
    font-size: 1.05rem;
    font-weight: 600;
  }
  .title-edit:focus {
    border-bottom-color: var(--accent);
  }
  textarea {
    font: inherit;
    color: inherit;
    resize: none;
    border: 1px solid transparent;
    border-radius: 6px;
    background: var(--surface-2);
    padding: 0.5rem 0.6rem;
    min-height: 4.5rem;
    line-height: 1.5;
  }
  textarea:focus {
    border-color: var(--border);
    background: var(--surface);
  }

  .steps {
    display: flex;
    flex-direction: column;
  }
  .step {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.1rem 0;
  }
  .step-title {
    flex: 1;
    border: none;
    background: transparent;
    padding: 0.3rem 0;
  }
  .step.done .step-title {
    color: var(--muted);
    text-decoration: line-through;
  }
  .remove {
    color: var(--muted);
    opacity: 0;
    padding: 0.1rem 0.4rem;
    transition: opacity 0.15s;
  }
  .step:hover .remove,
  .remove:focus-visible {
    opacity: 1;
  }
  @media (hover: none) {
    .remove {
      opacity: 0.6;
    }
  }
  .add-step {
    color: var(--muted);
  }

  .foot {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    font-size: 0.8rem;
    padding-top: 0.25rem;
    border-top: 1px solid var(--border);
  }
  .foot button {
    font-size: 0.85rem;
  }

  @media (max-width: 560px) {
    .detail {
      padding-left: 0.9rem;
    }
  }
</style>
