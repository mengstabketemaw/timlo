<script lang="ts">
  import { api, errText, type Settings, type SyncInfo } from './api'
  import Templates from './Templates.svelte'
  import Week from './Week.svelte'
  import { loadTracks } from './focus.svelte'

  type Section = 'templates' | 'week' | 'general'
  let section = $state<Section>('templates')

  let s = $state<Settings | null>(null)
  let quietOn = $state(false)
  let quietFrom = $state('23:00')
  let quietTo = $state('07:00')
  let saved = $state('')
  let error = $state('')

  let sync = $state<SyncInfo | null>(null)
  let url = $state('')
  let key = $state('')
  let email = $state('')
  let password = $state('')
  let showProject = $state(false)
  let busy = $state(false)
  let syncMsg = $state('')
  let syncErr = $state('')

  async function load() {
    try {
      s = await api.settings()
      quietOn = s.quiet_hours !== ''
      if (quietOn) [quietFrom, quietTo] = s.quiet_hours.split('-')
      error = ''
    } catch (e) {
      error = errText(e)
    }
    await loadSync()
  }

  async function loadSync() {
    try {
      sync = await api.syncStatus()
      url ||= sync.default_url
      key ||= sync.default_key
      showProject = !url || !key
    } catch (e) {
      syncErr = errText(e)
    }
  }

  $effect(() => {
    load()
  })

  async function save() {
    if (!s) return
    try {
      await api.saveSettings({ ...s, quiet_hours: quietOn ? `${quietFrom}-${quietTo}` : '' })
      loadTracks()
      saved = 'Saved'
      error = ''
      setTimeout(() => (saved = ''), 1500)
    } catch (e) {
      error = errText(e)
    }
  }

  async function login(signup: boolean) {
    busy = true
    syncMsg = ''
    syncErr = ''
    try {
      syncMsg = await api.syncLogin(url, key, email, password, signup)
      password = ''
      await loadSync()
    } catch (e) {
      syncErr = errText(e)
    } finally {
      busy = false
    }
  }

  async function logout() {
    try {
      await api.syncLogout()
      syncMsg = 'Logged out. Your plans stay on this device.'
      await loadSync()
    } catch (e) {
      syncErr = errText(e)
    }
  }
</script>

<div class="sections" role="tablist">
  {#each [['templates', 'Templates'], ['week', 'Week'], ['general', 'General']] as [id, label]}
    <button role="tab" aria-selected={section === id} class:on={section === id} onclick={() => (section = id as Section)}>{label}</button>
  {/each}
</div>

{#if section === 'templates'}
  <Templates />
{:else if section === 'week'}
  <Week />
{:else}
{#if error}<p class="error">{error}</p>{/if}

<h2>Reminders</h2>
{#if s}
  <div class="card form">
    <label class="check"><input type="checkbox" bind:checked={s.remind_start} onchange={save} /> When a block starts</label>
    <label class="num">
      Before a block starts
      <span><input type="number" min="0" max="240" bind:value={s.remind_before_start} onchange={save} /> min</span>
    </label>
    <label class="num">
      Before a block ends
      <span><input type="number" min="0" max="240" bind:value={s.remind_before_end} onchange={save} /> min</span>
    </label>
    <label class="check">
      <input type="checkbox" bind:checked={s.remind_unfinished} onchange={save} /> “Did you finish …?” when a block ends with no answer
    </label>
    <label class="check"><input type="checkbox" bind:checked={s.sound} onchange={save} /> Sound</label>
    <div class="quiet">
      <label class="check"><input type="checkbox" bind:checked={quietOn} onchange={save} /> Quiet hours</label>
      {#if quietOn}
        <span class="row">
          <input type="time" bind:value={quietFrom} onchange={save} aria-label="Quiet from" />
          <span class="muted">to</span>
          <input type="time" bind:value={quietTo} onchange={save} aria-label="Quiet until" />
        </span>
      {/if}
    </div>
    <label class="num">
      Waybar “ending soon” colour
      <span><input type="number" min="0" max="240" bind:value={s.ending_soon} onchange={save} /> min left</span>
    </label>
    <p class="muted small">0 turns a timed reminder off. Settings are shared between your devices once synced. {saved}</p>
  </div>

  <h2>Window</h2>
  <div class="card form">
    <label class="check">
      <input type="checkbox" bind:checked={s.hide_on_blur} onchange={save} /> Hide the window when you click elsewhere
    </label>
    <p class="muted small">Clicking the Waybar module again also hides it. This device only.</p>
  </div>

  <h2>Focus</h2>
  <div class="card form">
    <label class="num">
      Music folder
      <input class="path" bind:value={s.focus_dir} onchange={save} spellcheck="false" />
    </label>
    <p class="muted small">Focus shuffles the audio files in this folder (and its subfolders). This device only.</p>
  </div>
{/if}

<h2>Sync</h2>
<div class="card form">
  {#if sync?.account}
    <p>Signed in as <strong>{sync.account}</strong></p>
    <p class="muted small">
      {sync.pending} local change{sync.pending === 1 ? '' : 's'} waiting to send · last change received
      {sync.last_received ? new Date(sync.last_received).toLocaleString() : 'never'}
    </p>
    <div class="row"><button onclick={logout}>Log out</button></div>
  {:else}
    <p class="muted small">Sign in with the same account on each device to keep plans and Done/Skip in step.</p>
    <input type="email" placeholder="Email" autocomplete="username" bind:value={email} />
    <input type="password" placeholder="Password" autocomplete="current-password" bind:value={password} />
    {#if showProject}
      <input placeholder="Project URL (https://….supabase.co)" bind:value={url} />
      <input placeholder="Publishable (anon) key" bind:value={key} />
    {:else}
      <button class="ghost small link" onclick={() => (showProject = true)}>Project: {url}</button>
    {/if}
    <div class="row">
      <button class="primary" disabled={busy || !email || !password} onclick={() => login(false)}>Log in</button>
      <button disabled={busy || !email || !password} onclick={() => login(true)}>Create account</button>
    </div>
  {/if}
  {#if syncMsg}<p class="small">{syncMsg}</p>{/if}
  {#if syncErr}<p class="error">{syncErr}</p>{/if}
</div>
{/if}

<style>
  .sections {
    display: flex;
    gap: 0.25rem;
    margin-bottom: 1rem;
    padding: 0.2rem;
    border-radius: 8px;
    background: var(--surface-2);
    width: fit-content;
    max-width: 100%;
  }
  .sections button {
    border: none;
    background: transparent;
    padding: 0.3rem 0.9rem;
  }
  .sections button.on {
    background: var(--surface);
    font-weight: 600;
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.12);
  }
  .path {
    flex: 1;
    max-width: 22rem;
  }
  h2 {
    margin: 1.25rem 0 0.5rem;
  }
  h2:first-of-type {
    margin-top: 0;
  }
  .form {
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
    max-width: 34rem;
  }
  .form p {
    margin: 0;
  }
  .check {
    display: flex;
    gap: 0.5rem;
    align-items: center;
  }
  .num {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 0.75rem;
  }
  .num input {
    width: 4.5rem;
  }
  .quiet {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem 1rem;
    align-items: center;
  }
  .small {
    font-size: 0.85rem;
  }
  .link {
    align-self: flex-start;
    padding-left: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 100%;
  }
</style>
