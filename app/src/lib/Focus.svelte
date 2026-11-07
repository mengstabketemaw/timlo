<script lang="ts">
  import { api, type Day } from './api'
  import { focus, loadTracks, toggle, next, setLength, setSound, trackName, SOUNDS } from './focus.svelte'

  let canvas = $state<HTMLCanvasElement>()
  let now = $state(Date.now())
  let block = $state<string | null>(null)

  // The session length is typed into the ring: "45", "1:30", "90m", "1h", or 0/∞ for no limit.
  let editing = $state(false)
  let draft = $state('')

  function fmtLength(min: number): string {
    if (!min) return '∞'
    const h = Math.floor(min / 60)
    return h ? `${h}:${String(min % 60).padStart(2, '0')}` : String(min)
  }

  function parseLength(s: string): number | null {
    const t = s.trim().toLowerCase()
    if (t === '' || t === '∞' || t === 'inf' || t === '0') return 0
    let m = t.match(/^(\d{1,2}):(\d{1,2})$/)
    if (m) return +m[1] * 60 + +m[2]
    m = t.match(/^(?:(\d+)\s*h)?\s*(?:(\d+)\s*m?)?$/)
    if (m && (m[1] || m[2])) return (+(m[1] ?? 0)) * 60 + +(m[2] ?? 0)
    return null
  }

  function startEdit() {
    if (focus.playing) return
    draft = fmtLength(focus.length)
    editing = true
  }

  function commit() {
    if (!editing) return
    const v = parseLength(draft)
    if (v !== null) setLength(Math.min(v, 24 * 60))
    editing = false
  }

  function autofocus(el: HTMLInputElement) {
    el.focus()
    el.select()
  }

  $effect(() => {
    if (!focus.tracks.length && !focus.problem) loadTracks()
    api.day().then((d: Day) => {
      const b = d.now_min === null ? null : d.blocks.find((b) => b.start_min <= d.now_min! && d.now_min! < b.end_min)
      block = b?.title ?? null
    }, () => {})
    const t = setInterval(() => (now = Date.now()), 250)
    return () => clearInterval(t)
  })

  const left = $derived(
    focus.endsAt ? Math.max(0, focus.endsAt - now) : focus.leftWhenPaused || focus.length * 60_000,
  )
  const clock = $derived.by(() => {
    if (focus.length === 0) return '∞'
    const s = Math.ceil(left / 1000)
    const h = Math.floor(s / 3600)
    const m = Math.floor((s % 3600) / 60)
    const ss = String(s % 60).padStart(2, '0')
    return h ? `${h}:${String(m).padStart(2, '0')}:${ss}` : `${m}:${ss}`
  })
  const progress = $derived(focus.length ? 1 - left / (focus.length * 60_000) : 0)

  // ---- Visualizer: layered waves that drift on their own (not tied to the audio).
  $effect(() => {
    const c = canvas
    if (!c) return
    const g = c.getContext('2d')!
    let raf = 0
    let w = 0
    let h = 0
    const resize = () => {
      const dpr = window.devicePixelRatio || 1
      w = c.clientWidth
      h = c.clientHeight
      c.width = Math.round(w * dpr)
      c.height = Math.round(h * dpr)
      g.setTransform(dpr, 0, 0, dpr, 0, 0)
    }
    const ro = new ResizeObserver(resize)
    ro.observe(c)
    resize()

    // Smoothed energy per band (low, low-mid, mid, high).
    const energy = [0, 0, 0, 0]
    const layers = [
      { hue: 190, freq: 1.4, speed: 0.35, amp: 0.34, band: 0, alpha: 0.18 },
      { hue: 215, freq: 2.1, speed: -0.5, amp: 0.28, band: 1, alpha: 0.16 },
      { hue: 255, freq: 2.9, speed: 0.7, amp: 0.22, band: 2, alpha: 0.14 },
      { hue: 290, freq: 4.2, speed: -0.95, amp: 0.15, band: 3, alpha: 0.12 },
    ]
    let t = 0
    let last = performance.now()

    const frame = (ts: number) => {
      const dt = Math.min(0.05, (ts - last) / 1000)
      last = ts
      const live = focus.playing
      t += dt * (live ? 1 : 0.35)

      // Gentle, slow swell; a little livelier while playing.
      for (let b = 0; b < 4; b++) {
        const target = (live ? 0.3 : 0.16) + (live ? 0.08 * Math.sin(t * (0.5 + b * 0.23) + b) : 0)
        energy[b] += (target - energy[b]) * 0.04
      }

      g.clearRect(0, 0, w, h)
      // High up, so the ring and controls float on the water.
      const mid = h * 0.5
      for (const L of layers) {
        const swell = 0.15 + energy[L.band] * 1.7
        const amp = Math.min(h, 900) * L.amp * 0.45 * swell
        const ys: number[] = []
        const step = Math.max(4, w / 120)
        for (let x = 0; x <= w + step; x += step) {
          const u = x / w
          // Fade the ends so the waves come out of the edges softly.
          const env = Math.sin(Math.PI * Math.min(1, Math.max(0, u)))
          const y =
            Math.sin(u * Math.PI * L.freq + t * L.speed * 2) * 0.6 +
            Math.sin(u * Math.PI * L.freq * 2.3 - t * L.speed * 1.3 + 1.7) * 0.3 +
            Math.sin(u * Math.PI * L.freq * 0.5 + t * 0.4) * 0.25
          ys.push(mid + y * amp * (0.35 + 0.65 * env))
        }
        // Light at the surface, fading into the deep.
        const grad = g.createLinearGradient(0, mid - amp, 0, mid + h * 0.4)
        grad.addColorStop(0, `hsla(${L.hue}, 85%, 62%, ${L.alpha * 0.8 + energy[L.band] * 0.2})`)
        grad.addColorStop(0.35, `hsla(${L.hue}, 80%, 45%, ${L.alpha * 0.35})`)
        grad.addColorStop(1, `hsla(${L.hue}, 80%, 30%, 0)`)
        g.beginPath()
        g.moveTo(0, ys[0])
        ys.forEach((y, i) => g.lineTo(i * step, y))
        g.lineTo(w, h)
        g.lineTo(0, h)
        g.closePath()
        g.fillStyle = grad
        g.fill()
        g.beginPath()
        g.moveTo(0, ys[0])
        ys.forEach((y, i) => g.lineTo(i * step, y))
        g.strokeStyle = `hsla(${L.hue}, 90%, 72%, ${0.35 + energy[L.band] * 0.5})`
        g.lineWidth = 1.5
        g.stroke()
      }
      raf = requestAnimationFrame(frame)
    }
    raf = requestAnimationFrame(frame)
    return () => {
      cancelAnimationFrame(raf)
      ro.disconnect()
    }
  })
</script>

<section class="focus" class:playing={focus.playing}>
  <div class="glow" aria-hidden="true"></div>

  <header>
    <span class="eyebrow">
      {focus.forBlock ? `Focus · ${focus.forBlock}` : block ? `Now · ${block}` : 'Deep focus'}
    </span>
  </header>

  <div class="centre">
    <div class="ring" style="--p: {progress}">
      {#if editing}
        <input
          class="clock edit"
          bind:value={draft}
          use:autofocus
          inputmode="numeric"
          aria-label="Session length in minutes, or hours:minutes"
          onkeydown={(e) => {
            if (e.key === 'Enter') commit()
            if (e.key === 'Escape') editing = false
          }}
          onblur={commit}
        />
      {:else}
        <button class="clock" onclick={startEdit} disabled={focus.playing} title={focus.playing ? '' : 'Set the session length'}>{clock}</button>
      {/if}
      <div class="sub">
        {#if focus.finished}
          Session complete
        {:else if focus.playing && focus.track}
          {trackName(focus.track)}
        {:else if focus.playing}
          {focus.sound === 'music' ? 'Brown noise (no music found)' : `${SOUNDS.find(([s]) => s === focus.sound)?.[1]} noise`}
        {:else if editing}
          minutes, or h:mm · 0 = no limit
        {:else}
          {focus.length ? 'Tap the time to change it' : 'No time limit · tap to set one'}
        {/if}
      </div>
    </div>

    <div class="controls">
      <button class="round play" onclick={toggle} aria-label={focus.playing ? 'Pause' : 'Play'}>
        {#if focus.playing}
          <svg viewBox="0 0 24 24" aria-hidden="true"><rect x="6" y="5" width="4" height="14" rx="1.5" /><rect x="14" y="5" width="4" height="14" rx="1.5" /></svg>
        {:else}
          <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M8 5.5v13a1 1 0 0 0 1.5.86l10.5-6.5a1 1 0 0 0 0-1.72L9.5 4.64A1 1 0 0 0 8 5.5z" /></svg>
        {/if}
      </button>
      <button class="round skip" onclick={next} aria-label="Next track" disabled={!focus.playing || focus.sound !== 'music' || !focus.tracks.length}>
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 6.2v11.6a.8.8 0 0 0 1.2.7L15 13v4.5a1 1 0 0 0 2 0v-11a1 1 0 0 0-2 0V11L6.2 5.5A.8.8 0 0 0 5 6.2z" /></svg>
      </button>
    </div>

    <div class="sounds" role="radiogroup" aria-label="Sound">
      {#each SOUNDS as [id, label]}
        <button role="radio" aria-checked={focus.sound === id} class:on={focus.sound === id} onclick={() => setSound(id)}>
          {#if id !== 'music'}<i class="dot {id}" aria-hidden="true"></i>{/if}{label}
        </button>
      {/each}
    </div>

    {#if focus.sound === 'music' && focus.problem}
      <p class="problem">
        No music in <code>{focus.dir}</code> ({focus.problem}), so Play uses brown noise.
        Add audio files there or pick a folder in Settings, then
        <button class="link" onclick={loadTracks}>look again</button>.
      </p>
    {:else if focus.sound === 'music' && focus.tracks.length}
      <p class="count">{focus.tracks.length} track{focus.tracks.length === 1 ? '' : 's'} · shuffled</p>
    {/if}
    {#if focus.sessionMin > 0}
      <p class="count">{focus.sessionMin} min focused this session</p>
    {/if}
  </div>

  <canvas bind:this={canvas} aria-hidden="true"></canvas>
</section>

<style>
  .focus {
    --ink: #eef0ff;
    --ink-2: rgba(226, 230, 255, 0.62);
    position: relative;
    overflow: hidden;
    isolation: isolate;
    min-height: calc(100dvh - 5.5rem);
    border-radius: 18px;
    color: var(--ink);
    background:
      radial-gradient(120% 80% at 50% 0%, #1c2350 0%, transparent 60%),
      linear-gradient(180deg, #0d1026 0%, #0a0c1c 55%, #07080f 100%);
    display: flex;
    flex-direction: column;
  }
  .glow {
    position: absolute;
    inset: auto -20% -30% -20%;
    height: 70%;
    z-index: -1;
    background: radial-gradient(50% 50% at 50% 50%, rgba(96, 110, 255, 0.22), transparent 70%);
    transition: opacity 1.2s;
    opacity: 0.4;
  }
  .playing .glow {
    opacity: 1;
  }
  header {
    position: relative;
    z-index: 1;
    padding: 1.1rem 1.25rem 0;
    text-align: center;
  }
  .eyebrow {
    display: inline-block;
    font-size: 0.75rem;
    letter-spacing: 0.14em;
    text-transform: uppercase;
    color: var(--ink-2);
    padding: 0.3rem 0.8rem;
    border: 1px solid rgba(255, 255, 255, 0.1);
    border-radius: 999px;
    background: rgba(255, 255, 255, 0.04);
  }
  .centre {
    position: relative;
    z-index: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 1.1rem;
    padding: 1.5rem 1rem 0;
  }
  .ring {
    --size: min(15rem, 62vw);
    width: var(--size);
    height: var(--size);
    border-radius: 50%;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 0.3rem;
    background:
      radial-gradient(closest-side, #0e1230 calc(100% - 3px), transparent calc(100% - 2px)),
      conic-gradient(#8ec5ff calc(var(--p) * 360deg), rgba(255, 255, 255, 0.08) 0);
  }
  .clock {
    border: none;
    background: none;
    color: inherit;
    padding: 0;
    width: 80%;
    text-align: center;
    font-family: inherit;
    font-size: clamp(2.4rem, 11vw, 3.6rem);
    font-weight: 300;
    letter-spacing: 0.02em;
    font-variant-numeric: tabular-nums;
    line-height: 1;
  }
  button.clock:not(:disabled) {
    cursor: text;
    border-radius: 10px;
  }
  button.clock:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.06);
  }
  button.clock:disabled {
    opacity: 1;
  }
  .clock.edit {
    border-bottom: 1px solid rgba(142, 197, 255, 0.6);
    border-radius: 0;
    outline: none;
  }
  .sub {
    max-width: 80%;
    font-size: 0.85rem;
    color: var(--ink-2);
    text-align: center;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .controls {
    display: flex;
    align-items: center;
    gap: 1rem;
  }
  .round {
    display: grid;
    place-items: center;
    border-radius: 50%;
    border: 1px solid rgba(255, 255, 255, 0.14);
    background: rgba(255, 255, 255, 0.06);
    color: var(--ink);
    padding: 0;
  }
  .round:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.12);
  }
  .round svg {
    fill: currentColor;
  }
  .play {
    width: 4.2rem;
    height: 4.2rem;
    background: var(--ink);
    color: #0d1026;
    border: none;
    box-shadow: 0 0 0 0 rgba(142, 197, 255, 0.5);
    transition: box-shadow 0.6s;
  }
  .play:hover:not(:disabled) {
    background: #fff;
  }
  .playing .play {
    box-shadow: 0 0 32px 4px rgba(142, 197, 255, 0.35);
  }
  .play svg {
    width: 1.8rem;
  }
  .skip {
    width: 2.8rem;
    height: 2.8rem;
  }
  .skip svg {
    width: 1.2rem;
  }
  .sounds {
    display: flex;
    gap: 0.25rem;
    padding: 0.2rem;
    border-radius: 999px;
    background: rgba(255, 255, 255, 0.05);
    border: 1px solid rgba(255, 255, 255, 0.08);
  }
  .sounds button {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    border: none;
    border-radius: 999px;
    background: transparent;
    color: var(--ink-2);
    padding: 0.3rem 0.8rem;
    font-size: 0.85rem;
  }
  .sounds button:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.08);
  }
  .sounds button.on {
    background: rgba(142, 197, 255, 0.18);
    color: var(--ink);
  }
  .dot {
    width: 0.55rem;
    height: 0.55rem;
    border-radius: 50%;
  }
  .dot.brown {
    background: #a0714f;
  }
  .dot.pink {
    background: #e8a0bf;
  }
  .dot.white {
    background: #e9ecf5;
  }
  .count,
  .problem {
    margin: 0;
    font-size: 0.8rem;
    color: var(--ink-2);
    text-align: center;
    max-width: 28rem;
  }
  .problem code {
    color: var(--ink);
    word-break: break-all;
  }
  .link {
    border: none;
    background: none;
    padding: 0;
    color: #8ec5ff;
    text-decoration: underline;
    font-size: inherit;
  }
  canvas {
    position: absolute;
    inset: 0;
    z-index: 0;
    width: 100%;
    height: 100%;
    display: block;
    pointer-events: none;
  }
  button:focus-visible {
    outline: 2px solid #8ec5ff;
    outline-offset: 2px;
  }
</style>
