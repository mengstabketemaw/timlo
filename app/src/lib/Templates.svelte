<script lang="ts">
  import { api, errText, parseHm, timeInput, type Template } from './api'

  interface Row {
    key: number
    id: string | null
    start: string
    end: string
    title: string
    note: string
    icon: string
    colour: string
  }

  let templates = $state<Template[]>([])
  let selectedId = $state<string | null>(null)
  let rows = $state<Row[]>([])
  let name = $state('')
  let dirty = $state(false)
  let newName = $state('')
  let confirmDelete = $state(false)
  let error = $state('')
  let notice = $state('')
  let nextKey = 0

  const selected = $derived(templates.find((t) => t.id === selectedId) ?? null)

  async function load(select?: string | null) {
    try {
      templates = await api.templates()
      const id = select !== undefined ? select : selectedId
      pick(templates.find((t) => t.id === id)?.id ?? templates[0]?.id ?? null)
    } catch (e) {
      error = errText(e)
    }
  }

  function pick(id: string | null) {
    selectedId = id
    const t = templates.find((x) => x.id === id)
    name = t?.name ?? ''
    rows = (t?.blocks ?? []).map((b) => ({
      key: nextKey++,
      id: b.id,
      start: timeInput(b.start_min),
      end: timeInput(b.end_min),
      title: b.title,
      note: b.note ?? '',
      icon: b.icon ?? '',
      colour: b.colour ?? '',
    }))
    dirty = false
    confirmDelete = false
  }

  $effect(() => {
    load()
  })

  async function run(action: () => Promise<unknown>, select?: string | null, msg = '') {
    try {
      await action()
      error = ''
      notice = msg
      await load(select)
    } catch (e) {
      error = errText(e)
      notice = ''
    }
  }

  function select(id: string) {
    if (dirty && !window.confirm('Discard unsaved changes to this template?')) return
    error = ''
    notice = ''
    pick(id)
  }

  async function create() {
    const n = newName.trim()
    if (!n) return
    try {
      const t = await api.createTemplate(n)
      newName = ''
      error = ''
      await load(t.id)
    } catch (e) {
      error = errText(e)
    }
  }

  async function duplicate() {
    if (!selected) return
    let n = `${selected.name} copy`
    for (let i = 2; templates.some((t) => t.name.toLowerCase() === n.toLowerCase()); i++) n = `${selected.name} copy ${i}`
    try {
      const t = await api.duplicateTemplate(selected.id, n)
      await load(t.id)
    } catch (e) {
      error = errText(e)
    }
  }

  function rename() {
    if (!selected || name.trim() === selected.name) return
    run(() => api.renameTemplate(selected!.id, name), selected.id)
  }

  function addRow() {
    const last = rows.at(-1)
    const start = last ? Math.min(parseHm(last.end, true), 23 * 60) : 8 * 60
    rows.push({
      key: nextKey++,
      id: null,
      start: timeInput(start),
      end: timeInput(Math.min(start + 60, 24 * 60)),
      title: '',
      note: '',
      icon: '',
      colour: '',
    })
    dirty = true
  }

  function removeRow(key: number) {
    rows = rows.filter((r) => r.key !== key)
    dirty = true
  }

  function save() {
    if (!selected) return
    const blocks = rows
      .map((r) => ({
        id: r.id,
        start_min: parseHm(r.start),
        end_min: parseHm(r.end, true),
        title: r.title,
        note: r.note || null,
        icon: r.icon.trim() || null,
        colour: r.colour || null,
      }))
      .sort((a, b) => a.start_min - b.start_min)
    run(
      () => api.saveTemplateBlocks(selected!.id, blocks),
      selected.id,
      'Saved. Today (from now on) and upcoming days that follow this template were updated.',
    )
  }
</script>

{#if error}<p class="error">{error}</p>{/if}

<div class="layout">
  <aside class="card">
    <h3>Templates</h3>
    <ul>
      {#each templates as t (t.id)}
        <li>
          <button class="item" class:active={t.id === selectedId} onclick={() => select(t.id)}>
            <span>{t.name}</span>
            <span class="muted">{t.blocks.length}</span>
          </button>
        </li>
      {/each}
    </ul>
    <form
      class="row"
      onsubmit={(e) => {
        e.preventDefault()
        create()
      }}
    >
      <input bind:value={newName} placeholder="New template" aria-label="New template name" />
      <button type="submit" disabled={!newName.trim()}>Add</button>
    </form>
  </aside>

  <section class="card editor">
    {#if selected}
      <div class="row">
        <input
          class="name"
          bind:value={name}
          onblur={rename}
          onkeydown={(e) => e.key === 'Enter' && rename()}
          aria-label="Template name"
        />
        <span class="spacer"></span>
        <button onclick={duplicate}>Duplicate</button>
        {#if confirmDelete}
          <button class="danger" onclick={() => run(() => api.deleteTemplate(selected!.id), null)}>Really delete</button>
          <button onclick={() => (confirmDelete = false)}>Cancel</button>
        {:else}
          <button class="danger" onclick={() => (confirmDelete = true)}>Delete</button>
        {/if}
      </div>

      <table>
        <thead>
          <tr><th>Start</th><th>End</th><th>Title</th><th>Note</th><th>Icon</th><th>Colour</th><th></th></tr>
        </thead>
        <tbody>
          {#each rows as r (r.key)}
            <tr>
              <td><input type="time" bind:value={r.start} oninput={() => (dirty = true)} aria-label="Start" /></td>
              <td><input type="time" bind:value={r.end} oninput={() => (dirty = true)} aria-label="End" /></td>
              <td><input bind:value={r.title} oninput={() => (dirty = true)} placeholder="Title" aria-label="Title" /></td>
              <td><input bind:value={r.note} oninput={() => (dirty = true)} placeholder="Optional" aria-label="Note" /></td>
              <td>
                <input
                  class="icon"
                  bind:value={r.icon}
                  oninput={() => (dirty = true)}
                  placeholder="📚"
                  maxlength="4"
                  title="An emoji shown before the title"
                  aria-label="Icon"
                />
              </td>
              <td>
                <input
                  type="color"
                  value={r.colour || '#2f5d8a'}
                  oninput={(e) => {
                    r.colour = e.currentTarget.value
                    dirty = true
                  }}
                  aria-label="Colour"
                />
              </td>
              <td><button class="ghost danger" onclick={() => removeRow(r.key)} aria-label="Remove block">✕</button></td>
            </tr>
          {/each}
        </tbody>
      </table>
      {#if rows.length === 0}<p class="muted">No blocks yet.</p>{/if}

      <div class="row">
        <button onclick={addRow}>+ Add block</button>
        <span class="spacer"></span>
        {#if notice && !dirty}<span class="muted notice">{notice}</span>{/if}
        <button class="primary" disabled={!dirty} onclick={save}>Save</button>
      </div>
      <p class="muted hint">
        Saving updates today (from now on) and upcoming days that use this template, except days you edited by hand.
        Past days keep their plan. An end time of 00:00 means midnight.
      </p>
    {:else}
      <p class="muted">No templates yet. Add one on the left.</p>
    {/if}
  </section>
</div>

<style>
  .icon {
    width: 3.2rem;
    text-align: center;
  }
  .layout {
    display: grid;
    grid-template-columns: minmax(180px, 240px) 1fr;
    gap: 1rem;
    align-items: start;
  }
  @media (max-width: 720px) {
    .layout {
      grid-template-columns: 1fr;
    }
  }
  aside ul {
    list-style: none;
    padding: 0;
    margin: 0.5rem 0 0.75rem;
  }
  .item {
    width: 100%;
    display: flex;
    justify-content: space-between;
    border-color: transparent;
    background: transparent;
    text-align: left;
  }
  .item.active {
    background: var(--surface-2);
    font-weight: 600;
  }
  aside form input {
    flex: 1;
  }
  .editor {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
    overflow-x: auto;
  }
  .name {
    font-size: 1.1rem;
    font-weight: 600;
    flex: 1;
    min-width: 10rem;
  }
  table {
    border-collapse: collapse;
    width: 100%;
  }
  th {
    text-align: left;
    font-weight: 500;
    font-size: 0.8rem;
    color: var(--muted);
    padding: 0 0.25rem 0.25rem;
  }
  td {
    padding: 0.15rem 0.25rem;
  }
  td input:not([type='time']):not([type='color']) {
    width: 100%;
  }
  input[type='color'] {
    padding: 0;
    width: 2.2rem;
    height: 1.9rem;
  }
  .notice,
  .hint {
    font-size: 0.85rem;
  }
  .hint {
    margin: 0;
  }
</style>
