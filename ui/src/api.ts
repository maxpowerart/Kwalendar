import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

export const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

export interface Account {
  id: string;
  email: string;
  name: string | null;
  color: string;
  status: 'ok' | 'syncing' | 'reauth' | 'offline' | 'error';
  error: string | null;
  lastSync: number | null;
}

export interface Calendar {
  accountId: string;
  id: string;
  summary: string;
  color: string;
  selected: boolean;
  accessRole: string;
  primary: boolean;
  timeZone: string | null;
  defaultReminders: number[];
}

export interface Attendee {
  email: string;
  name: string | null;
  response: 'needsAction' | 'declined' | 'tentative' | 'accepted';
  isSelf: boolean;
  organizer: boolean;
  optional: boolean;
}

export interface Instance {
  key: string;
  accountId: string;
  calendarId: string;
  eventId: string;
  masterId: string | null;
  title: string;
  start: number;
  end: number;
  allDay: boolean;
  startDate: string | null;
  endDate: string | null;
  location: string | null;
  description: string | null;
  meetLink: string | null;
  htmlLink: string | null;
  color: string;
  attendees: Attendee[];
  organizer: string | null;
  organizerSelf: boolean;
  selfResponse: string | null;
  recurring: boolean;
  canEdit: boolean;
  pending: boolean;
  reminders: number[];
  remindersDefault: boolean;
}

export interface Config {
  googleClientId: string;
  googleClientSecret: string;
  remindersEnabled: boolean;
  notifyChanges: boolean;
  pollFocusedSecs: number;
  pollBackgroundSecs: number;
  view: string;
  sidebar: boolean;
  dayStartHour: number;
  theme: 'system' | 'dark' | 'light';
  /** 'system' | 'ru' | 'en' */
  language: string;
  checkUpdates: boolean;
  skippedVersion: string;
  relayUrl: string;
  relayKey: string;
}

export interface SyncStatus {
  state: 'idle' | 'syncing' | 'offline' | 'error';
  lastSync: number | null;
  pending: number;
  message: string | null;
}

export interface Snapshot {
  accounts: Account[];
  calendars: Calendar[];
  config: Config;
  sync: SyncStatus;
  mutedUntil: number;
  pushActive: boolean;
  builtinClient: boolean;
  /** Windows display language resolved by the core: 'ru' | 'en'. */
  systemLang: string;
}

export interface EventInput {
  accountId: string;
  calendarId: string;
  title: string;
  allDay: boolean;
  start?: number | null;
  end?: number | null;
  startDate?: string | null;
  endDate?: string | null;
  timeZone?: string | null;
  location?: string | null;
  description?: string | null;
  reminders?: number[] | null;
  attendees?: string[];
  addMeet?: boolean;
}

export interface EditTarget {
  accountId: string;
  calendarId: string;
  eventId: string;
  masterId: string | null;
  start: number | null;
  startDate: string | null;
}

export type Notice =
  | { kind: 'moved'; title: string; oldStart: number; newStart: number; allDay: boolean }
  | { kind: 'invite'; title: string; start: number; organizer: string | null }
  | { kind: 'conflict'; title: string }
  | { kind: 'reauth'; email: string }
  | { kind: 'error'; message: string };

export function targetOf(i: Instance): EditTarget {
  return {
    accountId: i.accountId,
    calendarId: i.calendarId,
    eventId: i.eventId,
    masterId: i.masterId,
    start: i.allDay ? null : i.start,
    startDate: i.startDate,
  };
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (isTauri) return invoke<T>(cmd, args);
  const { mockInvoke } = await import('./mock');
  return mockInvoke(cmd, args) as Promise<T>;
}

export async function on<T>(event: string, cb: (payload: T) => void): Promise<UnlistenFn> {
  if (isTauri) return listen<T>(event, (e) => cb(e.payload));
  const { mockListen } = await import('./mock');
  return mockListen(event, cb as (p: unknown) => void);
}

export const api = {
  snapshot: () => call<Snapshot>('snapshot'),
  instances: (from: number, to: number) => call<Instance[]>('instances', { from, to }),
  search: (q: string) => call<Instance[]>('search', { q }),
  setCalendarSelected: (accountId: string, calendarId: string, selected: boolean) =>
    call<void>('set_calendar_selected', { accountId, calendarId, selected }),
  addAccount: (hint?: string) => call<Account>('add_account', { hint: hint ?? null }),
  removeAccount: (accountId: string) => call<void>('remove_account', { accountId }),
  syncNow: () => call<void>('sync_now'),
  createEvent: (input: EventInput) => call<string>('create_event', { input }),
  updateEvent: (target: EditTarget, input: EventInput) => call<void>('update_event', { target, input }),
  updateSeries: (target: EditTarget, input: EventInput) => call<void>('update_series', { target, input }),
  moveEvent: (target: EditTarget, start: number, end: number) => call<void>('move_event', { target, start, end }),
  deleteEvent: (target: EditTarget, scope: 'this' | 'series') => call<void>('delete_event', { target, scope }),
  respond: (target: EditTarget, response: string) => call<void>('respond', { target, response }),
  setConfig: (config: Config) => call<void>('set_config', { config }),
  mute: (minutes: number) => call<void>('mute', { minutes }),
  openUrl: (url: string) => call<void>('open_url', { url }),
  appReady: () => call<void>('app_ready'),
  autostartGet: () => call<boolean>('autostart_get'),
  autostartSet: (enabled: boolean) => call<void>('autostart_set', { enabled }),
  testNotification: () => call<void>('test_notification'),
};

// ---------- updates ----------

export interface Release {
  version: string;
  notes: string;
  url: string;
  sha256: string;
  published: string | null;
  manifestUrl: string;
}

export interface Listed {
  version: string;
  published: string;
  prerelease: boolean;
  withdrawn: boolean;
  reason: string;
  manifestUrl: string;
}

export interface UpdateInfo {
  current: string;
  available: Release | null;
  withdrawn: { reason: string; target: Listed | null } | null;
  checkedAt: number | null;
  error: string | null;
}

export const updates = {
  info: () => call<UpdateInfo>('update_info'),
  check: () => call<UpdateInfo>('update_check'),
  list: () => call<Listed[]>('update_list'),
  manifest: (url: string) => call<Release>('update_manifest', { url }),
  skip: (version: string) => call<void>('update_skip', { version }),
  install: (release: Release) => call<void>('update_install', { release }),
};

/** semver-ish compare of "x.y.z" (pre-release tags ignored). */
export function cmpVersion(a: string, b: string): number {
  const p = (v: string) => v.replace(/^v/i, '').split(/[.-]/).slice(0, 3).map((x) => parseInt(x, 10) || 0);
  const [x, y] = [p(a), p(b)];
  for (let i = 0; i < 3; i++) if (x[i] !== y[i]) return x[i] - y[i];
  return 0;
}
