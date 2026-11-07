// Focus player. Lives at module level so the sound keeps playing while you
// switch to other tabs.
//
// Everything is played from memory through Web Audio: music files are
// decoded whole (streaming them through an <audio> element on Linux,
// WebKitGTK, made playback skip ahead), and the coloured noises are
// generated here. Pausing suspends the AudioContext, which freezes the
// sound exactly in place.

import { api, errText, isoDate } from './api'

export type Sound = 'music' | 'brown' | 'pink' | 'white'

export const SOUNDS: [Sound, string][] = [
  ['music', 'Music'],
  ['brown', 'Brown'],
  ['pink', 'Pink'],
  ['white', 'White'],
]

export const focus = $state({
  dir: '',
  tracks: [] as string[],
  /** Why music can't play (shown under the controls). */
  problem: '' as string,
  playing: false,
  /** Music: the file playing. Noise: empty. */
  track: '' as string,
  sound: loadSound(),
  /** Session length in minutes; 0 = no limit. */
  length: loadLength(),
  /** Epoch ms when the current session ends (0 = none running). */
  endsAt: 0,
  /** Ms left when paused mid-session. */
  leftWhenPaused: 0,
  /** The last session ran to its end. */
  finished: false,
  /** Block this session is for, when started from one. */
  forBlock: null as string | null,
  /** Minutes logged in the running session. */
  sessionMin: 0,
})

const CANT_DECODE =
  "none of the files could be played. On Linux the app needs GStreamer's plugins for that: gst-plugins-base, gst-plugins-good and gst-libav"

let ctx: AudioContext | null = null
let source: AudioBufferSourceNode | null = null
let history: string[] = []
let endTimer: ReturnType<typeof setTimeout> | undefined

// ---- Settings -------------------------------------------------------------

function stored(key: string): string | null {
  try {
    return localStorage.getItem(key)
  } catch {
    return null
  }
}

function store(key: string, value: string) {
  try {
    localStorage.setItem(key, value)
  } catch {}
}

function loadLength(): number {
  const v = Number(stored('timlo.focus.length') ?? 50)
  return Number.isFinite(v) && v >= 0 ? v : 50
}

function loadSound(): Sound {
  const v = stored('timlo.focus.sound')
  return SOUNDS.some(([s]) => s === v) ? (v as Sound) : 'music'
}

export function setLength(min: number) {
  focus.length = min
  focus.forBlock = null
  store('timlo.focus.length', String(min))
  if (focus.playing) startClock(min * 60_000)
  else focus.leftWhenPaused = 0
}

export async function setSound(s: Sound) {
  if (s === focus.sound) return
  focus.sound = s
  store('timlo.focus.sound', s)
  if (focus.playing) await startSound()
  else stopSource()
}

export async function loadTracks() {
  try {
    const r = await api.focusTracks()
    focus.dir = r.dir
    focus.tracks = r.tracks
    focus.problem = r.problem ?? ''
  } catch (e) {
    focus.problem = errText(e)
  }
}

// ---- Sources --------------------------------------------------------------

function audioCtx(): AudioContext {
  ctx ??= new AudioContext()
  return ctx
}

/** A random track, avoiding the last few played when there's a choice. */
function pick(): string {
  const n = Math.min(5, focus.tracks.length - 1)
  const avoid = new Set(n > 0 ? history.slice(-n) : [])
  const pool = focus.tracks.filter((t) => !avoid.has(t))
  const list = pool.length ? pool : focus.tracks
  return list[Math.floor(Math.random() * list.length)]
}

function stopSource() {
  const s = source
  source = null
  if (s) {
    s.onended = null
    try {
      s.stop()
    } catch {}
  }
}

function startBuffer(buffer: AudioBuffer, loop: boolean) {
  const c = audioCtx()
  stopSource()
  const src = c.createBufferSource()
  src.buffer = buffer
  src.loop = loop
  // Noise buffers end with a SEAM-long tail that is blended into their start.
  if (loop) src.loopEnd = buffer.duration - SEAM
  src.connect(c.destination)
  src.onended = () => {
    // Only a track that played to its end moves on (not one we stopped).
    if (source === src) {
      source = null
      void nextTrack()
    }
  }
  source = src
  src.start()
}

async function playTrack(path: string): Promise<void> {
  const c = audioCtx()
  focus.track = path
  history.push(path)
  if (history.length > 50) history = history.slice(-50)
  let buffer: AudioBuffer
  try {
    buffer = await c.decodeAudioData(await (await readTrack(path)).arrayBuffer())
  } catch {
    // Skip files that can't be decoded, but don't spin if none can.
    focus.tracks = focus.tracks.filter((t) => t !== path)
    if (!focus.tracks.length) throw new Error(CANT_DECODE)
    return playTrack(pick())
  }
  if (focus.track !== path || focus.sound !== 'music') return // changed while loading
  startBuffer(buffer, false)
}

async function nextTrack() {
  if (focus.sound !== 'music') return
  if (!focus.tracks.length) await loadTracks()
  if (!focus.tracks.length) return
  try {
    await playTrack(pick())
  } catch (e) {
    focus.problem = errText(e)
  }
}

/** Skip button: another track (music only). */
export const next = nextTrack

const noiseCache = new Map<Sound, AudioBuffer>()
const SEAM = 0.05

/**
 * A few seconds of noise, looped. Brown (deep, like a waterfall) is white
 * noise integrated; pink (softer rain) uses Paul Kellet's filter. Both are
 * scaled to a comfortable level, since there's no volume control.
 */
function noise(kind: Sound): AudioBuffer {
  const cached = noiseCache.get(kind)
  if (cached) return cached
  const c = audioCtx()
  const seconds = 12
  const buf = c.createBuffer(2, c.sampleRate * seconds, c.sampleRate)
  for (let ch = 0; ch < 2; ch++) {
    const d = buf.getChannelData(ch)
    let b0 = 0, b1 = 0, b2 = 0, b3 = 0, b4 = 0, b5 = 0, b6 = 0
    let last = 0
    for (let i = 0; i < d.length; i++) {
      const w = Math.random() * 2 - 1
      if (kind === 'brown') {
        last = (last + 0.02 * w) / 1.02
        d[i] = last * 3.5
      } else if (kind === 'pink') {
        b0 = 0.99886 * b0 + w * 0.0555179
        b1 = 0.99332 * b1 + w * 0.0750759
        b2 = 0.969 * b2 + w * 0.153852
        b3 = 0.8665 * b3 + w * 0.3104856
        b4 = 0.55 * b4 + w * 0.5329522
        b5 = -0.7616 * b5 - w * 0.016898
        d[i] = (b0 + b1 + b2 + b3 + b4 + b5 + b6 + w * 0.5362) * 0.11
        b6 = w * 0.115926
      } else {
        d[i] = w * 0.25
      }
    }
    // Normalise, then blend the first 50 ms with the 50 ms after the loop
    // end (see SEAM in startBuffer), so the wrap-around is seamless.
    let peak = 0
    for (let i = 0; i < d.length; i++) peak = Math.max(peak, Math.abs(d[i]))
    const target = kind === 'white' ? 0.3 : 0.5
    const k = peak > 0 ? target / peak : 1
    const fade = Math.floor(c.sampleRate * SEAM)
    for (let i = 0; i < d.length; i++) d[i] *= k
    for (let i = 0; i < fade; i++) {
      const t = i / fade
      const j = d.length - fade + i
      d[i] = d[i] * t + d[j] * (1 - t)
    }
  }
  noiseCache.set(kind, buf)
  return buf
}

/** Start whatever `focus.sound` says, replacing what's playing. */
async function startSound() {
  if (focus.sound === 'music') {
    if (!focus.tracks.length) await loadTracks()
    if (!focus.tracks.length) {
      // No music: fall back to brown noise rather than silence.
      focus.track = ''
      startBuffer(noise('brown'), true)
      return
    }
    await playTrack(pick())
  } else {
    focus.track = ''
    startBuffer(noise(focus.sound), true)
  }
}

// ---- Session clock and log -------------------------------------------------

let session: { id: string; date: string; startedAt: number; playedMs: number; resumedAt: number; pausedAt: number } | null =
  null
let logTimer: ReturnType<typeof setInterval> | undefined

/** Pause this long and the next Play starts a new session. */
const NEW_SESSION_AFTER = 30 * 60_000

function playedMs(): number {
  if (!session) return 0
  return session.playedMs + (session.resumedAt ? Date.now() - session.resumedAt : 0)
}

async function saveSession() {
  if (!session) return
  const minutes = Math.floor(playedMs() / 60_000)
  focus.sessionMin = minutes
  if (minutes < 1) return
  try {
    await api.saveFocusSession({
      id: session.id,
      date: session.date,
      started_at: session.startedAt,
      minutes,
      block_title: focus.forBlock,
    })
  } catch {
    // Logged again on the next save.
  }
}

function sessionResume() {
  const now = Date.now()
  if (!session || focus.finished || (session.pausedAt && now - session.pausedAt > NEW_SESSION_AFTER)) {
    session = { id: crypto.randomUUID(), date: isoDate(new Date()), startedAt: now, playedMs: 0, resumedAt: 0, pausedAt: 0 }
    focus.sessionMin = 0
  }
  session.resumedAt = now
  clearInterval(logTimer)
  logTimer = setInterval(saveSession, 60_000)
}

function sessionPause() {
  clearInterval(logTimer)
  if (!session) return
  session.playedMs = playedMs()
  session.resumedAt = 0
  session.pausedAt = Date.now()
  void saveSession()
}

function startClock(ms: number) {
  clearTimeout(endTimer)
  if (focus.length === 0) {
    focus.endsAt = 0
    return
  }
  focus.endsAt = Date.now() + ms
  endTimer = setTimeout(() => {
    stopPlaying()
    focus.finished = true
    focus.leftWhenPaused = 0
    session = null
  }, ms)
}

// ---- Controls ---------------------------------------------------------------

export async function play() {
  const c = audioCtx()
  sessionResume()
  focus.finished = false
  focus.playing = true
  startClock(focus.leftWhenPaused || focus.length * 60_000)
  focus.leftWhenPaused = 0
  try {
    // Paused mid-sound: carry on exactly where it stopped.
    if (!source) await startSound()
    await c.resume()
  } catch (e) {
    focus.problem = errText(e)
    stopPlaying()
  }
}

function stopPlaying() {
  clearTimeout(endTimer)
  if (focus.playing) sessionPause()
  focus.playing = false
  focus.endsAt = 0
  void ctx?.suspend()
}

export function pause() {
  focus.leftWhenPaused = focus.endsAt ? Math.max(0, focus.endsAt - Date.now()) : 0
  stopPlaying()
}

export function toggle() {
  if (focus.playing) pause()
  else void play()
}

/**
 * Start a session for a plan block: runs until the block ends. Doesn't
 * change the remembered default length.
 */
export async function startFor(title: string, minutes: number) {
  if (focus.playing) pause()
  session = null
  focus.length = Math.max(1, minutes)
  focus.leftWhenPaused = 0
  focus.forBlock = title || null
  await play()
}

// ---- Reading music files -----------------------------------------------------

const CHUNK = 4 << 20

function fromBase64(b64: string): Uint8Array<ArrayBuffer> {
  const native = (Uint8Array as unknown as { fromBase64?: (s: string) => Uint8Array<ArrayBuffer> }).fromBase64
  if (native) return native(b64)
  const bin = atob(b64)
  const out = new Uint8Array(bin.length)
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i)
  return out
}

/** The whole file, read from the app in chunks. */
async function readTrack(path: string): Promise<Blob> {
  const parts: Uint8Array<ArrayBuffer>[] = []
  let offset = 0
  for (;;) {
    const { data, size } = await api.focusRead(path, offset, CHUNK)
    const part = fromBase64(data)
    parts.push(part)
    offset += part.length
    if (!part.length || offset >= size) break
  }
  return new Blob(parts, { type: mimeOf(path) })
}

function mimeOf(path: string): string {
  const ext = path.split('.').pop()?.toLowerCase() ?? ''
  const types: Record<string, string> = {
    mp3: 'audio/mpeg',
    ogg: 'audio/ogg',
    oga: 'audio/ogg',
    opus: 'audio/ogg',
    flac: 'audio/flac',
    wav: 'audio/wav',
    m4a: 'audio/mp4',
    aac: 'audio/aac',
    webm: 'audio/webm',
  }
  return types[ext] ?? 'audio/mpeg'
}

/** "…/Music/focus/Rain Loop.mp3" → "Rain Loop" */
export function trackName(path: string): string {
  const file = path.split(/[\\/]/).pop() ?? path
  return file.replace(/\.[^.]+$/, '').replace(/[_]+/g, ' ')
}
