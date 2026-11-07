// App-wide UI state: which tab is open, and one-shot requests between tabs.

export type Tab = 'today' | 'tasks' | 'focus' | 'calendar' | 'settings'

export const ui = $state({
  tab: 'today' as Tab,
  /** Bumped to ask the Tasks tab to focus its "Add a task…" box. */
  quickAdd: 0,
})
