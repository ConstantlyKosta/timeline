import { create } from 'zustand';
import { cmd } from './api';
import type { MediaItem, NoteSummary, NoteRef, Stats, TagCount, TimelineEntry } from './types';

export type View = 'timeline' | 'notes' | 'graph' | 'search' | 'library';
export type Theme = 'light' | 'dark';

interface State {
  ready: boolean;
  error: string | null;
  view: View;
  theme: Theme;
  entries: TimelineEntry[];
  notes: NoteSummary[];
  titles: NoteRef[];
  tags: TagCount[];
  media: MediaItem[];
  stats: Stats | null;
  selectedNoteId: string | null;
  selectedMediaId: string | null;
  /** Seek position (seconds) requested together with openMedia, consumed by the player. */
  seekTo: number | null;
  /** Position in the timeline survives view switches. */
  tl: { ppd: number; centerMs: number } | null;
  switcherOpen: boolean;
  searchQuery: string;
  toast: string | null;
  // actions
  init(): Promise<void>;
  refresh(): Promise<void>;
  setView(v: View): void;
  setTheme(t: Theme): void;
  openNote(id: string | null): void;
  openMedia(id: string | null, seekTo?: number | null): void;
  newNote(body?: string, date?: number): Promise<string>;
  openByTitle(title: string): Promise<void>;
  setSwitcher(open: boolean): void;
  setSearchQuery(q: string): void;
  setTl(tl: { ppd: number; centerMs: number }): void;
  notify(msg: string): void;
  fail(e: unknown): void;
}

const savedTheme = (): Theme => {
  try {
    const t = localStorage.getItem('theme');
    if (t === 'light' || t === 'dark') return t;
  } catch { /* ignore */ }
  return window.matchMedia?.('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
};

let toastTimer: ReturnType<typeof setTimeout> | undefined;

export const useStore = create<State>((set, get) => ({
  ready: false, error: null, view: 'timeline', theme: savedTheme(),
  entries: [], notes: [], titles: [], tags: [], media: [], stats: null,
  selectedNoteId: null, selectedMediaId: null, seekTo: null, tl: null, switcherOpen: false, searchQuery: '', toast: null,

  async init() {
    try {
      await cmd.bootstrap();
      await get().refresh();
      set({ ready: true });
    } catch (e) { get().fail(e); }
  },
  async refresh() {
    try {
      const [entries, notes, titles, tags, media, stats] = await Promise.all([
        cmd.timelineEntries(), cmd.listNotes(), cmd.noteTitles(), cmd.listTags(), cmd.listMedia(), cmd.stats(),
      ]);
      set({ entries, notes, titles, tags, media, stats });
    } catch (e) { get().fail(e); }
  },
  setView: (view) => set({ view }),
  setTheme(theme) {
    try { localStorage.setItem('theme', theme); } catch { /* ignore */ }
    set({ theme });
  },
  openNote: (id) => set({ selectedNoteId: id, view: id ? 'notes' : get().view }),
  openMedia: (id, seekTo = null) => set({ selectedMediaId: id, seekTo, view: id ? 'library' : get().view }),
  async newNote(body = '', date) {
    const n = await cmd.createNote(body, date);
    await get().refresh();
    set({ selectedNoteId: n.id, view: 'notes' });
    return n.id;
  },
  async openByTitle(title) {
    const n = await cmd.openNoteByTitle(title);
    await get().refresh();
    set({ selectedNoteId: n.id, view: 'notes' });
  },
  setSwitcher: (switcherOpen) => set({ switcherOpen }),
  setSearchQuery: (searchQuery) => set({ searchQuery }),
  setTl: (tl) => set({ tl }),
  notify(msg) {
    set({ toast: msg });
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => set({ toast: null }), 3500);
  },
  fail(e) {
    const msg = e instanceof Error ? e.message : String(e);
    console.error(e);
    get().notify(msg);
    if (!get().ready) set({ error: msg });
  },
}));
