import { invoke } from '@tauri-apps/api/core'

export type Status = 'pending' | 'done' | 'skipped' | 'missed'

export interface DayBlock {
  id: string
  date: string
  template_block_id: string | null
  start_min: number
  end_min: number
  title: string
  note: string | null
  status: Status
  status_at: number | null
}

export interface Day {
  date: string
  template_id: string | null
  template_name: string | null
  default_template_id: string | null
  customised: boolean
  overridden: boolean
  is_today: boolean
  is_past: boolean
  now_min: number | null
  blocks: DayBlock[]
  /** Icon and colour per day block id, from its template block. */
  looks: Record<string, { icon: string | null; colour: string | null }>
}

export interface TemplateBlock {
  id: string
  template_id: string
  start_min: number
  end_min: number
  title: string
  note: string | null
  icon: string | null
  colour: string | null
}

export interface Template {
  id: string
  name: string
  colour: string | null
  blocks: TemplateBlock[]
}

/** A block as sent to the backend; nested fields stay snake_case. */
export interface BlockIn {
  id?: string | null
  start_min: number
  end_min: number
  title: string
  note?: string | null
  icon?: string | null
  colour?: string | null
}

export interface MonthDay {
  date: string
  template_name: string | null
  exists: boolean
  overridden: boolean
  customised: boolean
  total: number
  done: number
  skipped: number
  missed: number
}

export interface Settings {
  remind_start: boolean
  remind_before_start: number
  remind_before_end: number
  remind_unfinished: boolean
  ending_soon: number
  /** "23:00-07:00", or "" for none. */
  quiet_hours: string
  sound: boolean
  /** Desktop: hide the window when it loses focus (this device only). */
  hide_on_blur: boolean
}

export interface SyncInfo {
  url: string | null
  account: string | null
  pending: number
  last_received: string | null
  files: string
  default_url: string
  default_key: string
}

export interface Task {
  id: string
  parent_id: string | null
  title: string
  notes: string | null
  done: boolean
  done_at: number | null
  created_at: number
  subtasks: Task[]
}

export const api = {
  day: (date?: string | null) => invoke<Day>('get_day', { date: date ?? null }),
  setStatus: (id: string, status: Status) => invoke<DayBlock>('set_status', { id, status }),
  switchDay: (date: string, templateId: string | null) => invoke<void>('switch_day', { date, templateId }),
  addDayBlock: (date: string, block: BlockIn) => invoke<DayBlock>('add_day_block', { date, block }),
  updateDayBlock: (id: string, startMin: number, endMin: number, title: string, note: string | null) =>
    invoke<void>('update_day_block', { id, startMin, endMin, title, note }),
  deleteDayBlock: (id: string) => invoke<void>('delete_day_block', { id }),
  month: (year: number, month: number) => invoke<MonthDay[]>('get_month', { year, month }),
  templates: () => invoke<Template[]>('list_templates'),
  createTemplate: (name: string) => invoke<Template>('create_template', { name }),
  renameTemplate: (id: string, name: string) => invoke<void>('rename_template', { id, name }),
  duplicateTemplate: (id: string, name: string) => invoke<Template>('duplicate_template', { id, name }),
  deleteTemplate: (id: string) => invoke<void>('delete_template', { id }),
  saveTemplateBlocks: (id: string, blocks: BlockIn[]) => invoke<TemplateBlock[]>('save_template_blocks', { id, blocks }),
  week: () => invoke<(string | null)[]>('get_week'),
  setWeek: (weekday: number, templateId: string | null) => invoke<void>('set_week', { weekday, templateId }),
  tasks: (showDone: boolean) => invoke<{ tasks: Task[]; done_count: number }>('list_tasks', { showDone }),
  addTask: (title: string, parentId: string | null = null) => invoke<Task>('add_task', { title, parentId }),
  updateTask: (id: string, title: string, notes: string | null) => invoke<void>('update_task', { id, title, notes }),
  setTaskDone: (id: string, done: boolean) => invoke<void>('set_task_done', { id, done }),
  deleteTask: (id: string) => invoke<void>('delete_task', { id }),
  settings: () => invoke<Settings>('get_settings'),
  saveSettings: (settings: Settings) => invoke<void>('save_settings', { settings }),
  syncStatus: () => invoke<SyncInfo>('sync_status'),
  syncLogin: (url: string, key: string, email: string, password: string, signup: boolean) =>
    invoke<string>('sync_login', { url, key, email, password, signup }),
  syncLogout: () => invoke<void>('sync_logout'),
}

/** 420 → "07:00" */
export function hm(min: number): string {
  return `${String(Math.floor(min / 60)).padStart(2, '0')}:${String(min % 60).padStart(2, '0')}`
}

/** "07:00" → 420. An end time of "00:00" means midnight at the end of the day. */
export function parseHm(s: string, isEnd = false): number {
  const [h, m] = s.split(':').map(Number)
  const v = h * 60 + m
  return isEnd && v === 0 ? 24 * 60 : v
}

/** For <input type="time">, which can't show 24:00. */
export function timeInput(min: number): string {
  return min >= 24 * 60 ? '00:00' : hm(min)
}

export function fmtDur(min: number): string {
  const h = Math.floor(min / 60)
  const m = min % 60
  if (h === 0) return `${m}m`
  return m === 0 ? `${h}h` : `${h}h ${m}m`
}

/** Local date as YYYY-MM-DD. */
export function isoDate(d: Date): string {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`
}

export function parseIso(s: string): Date {
  const [y, m, d] = s.split('-').map(Number)
  return new Date(y, m - 1, d)
}

export function longDate(s: string): string {
  return parseIso(s).toLocaleDateString(undefined, { weekday: 'long', day: 'numeric', month: 'long' })
}

export const WEEKDAYS = ['Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday', 'Sunday']

export const STATUS_LABEL: Record<Status, string> = {
  pending: 'Pending',
  done: 'Done',
  skipped: 'Skipped',
  missed: 'Missed',
}

export function errText(e: unknown): string {
  return typeof e === 'string' ? e : e instanceof Error ? e.message : String(e)
}
