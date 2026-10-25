// One command surface, two transports: Tauri IPC (desktop/mobile app) and HTTP (dev bridge for the browser / Playwright).
import type { AsrSettings, AsrStatus, ExportResult, ImportSummary, JobInfo, Graph, GraphRaw, GraphViewRow, NodePos, ImportOutcome, MediaItem, Note, NoteContext, NoteRef, NoteSummary, SearchHit, Stats, TagCount, TimelineEntry, Transcript } from './types';

export interface Api {
  /** Raw command dispatch (name + camelCase JSON args). */
  call<T>(cmd: string, args?: Record<string, unknown>): Promise<T>;
  importFile(name: string, bytes: Uint8Array, recordedAt?: number): Promise<ImportOutcome>;
  mediaUrl(id: string): string;
  readonly kind: 'tauri' | 'http';
}

export const isTauri = () => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

class HttpApi implements Api {
  readonly kind = 'http' as const;
  constructor(private base: string) {}
  async call<T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
    const r = await fetch(`${this.base}/api/${cmd}`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(args) });
    const j = await r.json();
    if (!r.ok) throw new Error(j.error ?? `HTTP ${r.status}`);
    return j as T;
  }
  async importFile(name: string, bytes: Uint8Array, recordedAt?: number): Promise<ImportOutcome> {
    const q = new URLSearchParams({ name });
    if (recordedAt) q.set('recordedAt', String(Math.round(recordedAt)));
    const r = await fetch(`${this.base}/import?${q}`, { method: 'POST', body: bytes as unknown as BodyInit });
    const j = await r.json();
    if (!r.ok) throw new Error(j.error ?? `HTTP ${r.status}`);
    return j;
  }
  mediaUrl(id: string) { return `${this.base}/media/${id}`; }
}

class TauriApi implements Api {
  readonly kind = 'tauri' as const;
  async call<T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
    const { invoke } = await import('@tauri-apps/api/core');
    try {
      return await invoke<T>('dispatch', { name: cmd, args });
    } catch (e) {
      throw new Error(typeof e === 'string' ? e : (e as Error).message ?? String(e));
    }
  }
  async importFile(name: string, bytes: Uint8Array, recordedAt?: number): Promise<ImportOutcome> {
    const { invoke } = await import('@tauri-apps/api/core');
    const headers: Record<string, string> = { 'x-file-name': encodeURIComponent(name) };
    if (recordedAt) headers['x-recorded-at'] = String(Math.round(recordedAt));
    try {
      return await invoke<ImportOutcome>('import_media', bytes, { headers });
    } catch (e) {
      throw new Error(typeof e === 'string' ? e : (e as Error).message ?? String(e));
    }
  }
  mediaUrl(id: string) {
    // Custom URI scheme served by src-tauri (Range-capable). Windows/Android use http://tlmedia.localhost/.
    const ua = navigator.userAgent;
    return /Windows|Android/i.test(ua) ? `http://tlmedia.localhost/${id}` : `tlmedia://localhost/${id}`;
  }
}

function pickBase(): string {
  if (typeof location === 'undefined') return '';
  const q = new URLSearchParams(location.search).get('bridge');
  if (q) return q;
  return location.port === '5173' ? 'http://127.0.0.1:8787' : '';
}

export const api: Api = isTauri() ? new TauriApi() : new HttpApi(pickBase());

// Typed helpers --------------------------------------------------------------
export const cmd = {
  bootstrap: () => api.call<{ seeded: boolean; stats: Stats }>('bootstrap', { tzOffsetMinutes: -new Date().getTimezoneOffset() }),
  loadSample: () => api.call<Stats>('load_sample_data', { tzOffsetMinutes: -new Date().getTimezoneOffset() }),
  stats: () => api.call<Stats>('stats'),
  timelineEntries: () => api.call<TimelineEntry[]>('timeline_entries'),
  listNotes: () => api.call<NoteSummary[]>('list_notes'),
  getNote: (id: string) => api.call<Note>('get_note', { id }),
  noteTitles: () => api.call<NoteRef[]>('note_titles'),
  listTags: () => api.call<TagCount[]>('list_tags'),
  createNote: (body = '', date?: number) => api.call<Note>('create_note', { body, date }),
  updateNote: (id: string, patch: { body?: string; date?: number }) => api.call<Note>('update_note', { id, ...patch }),
  deleteNote: (id: string) => api.call<null>('delete_note', { id }),
  toggleTask: (id: string, line: number) => api.call<Note>('toggle_task', { id, line }),
  openNoteByTitle: (title: string, date?: number) => api.call<Note>('open_note_by_title', { title, date }),
  noteContext: (id: string) => api.call<NoteContext>('note_context', { id }),
  graph: () => api.call<Graph>('graph'),
  graphData: () => api.call<GraphRaw>('graph_data'),
  graphViews: () => api.call<GraphViewRow[]>('graph_views'),
  saveGraphView: (name: string, json: unknown) => api.call<null>('save_graph_view', { name, json }),
  deleteGraphView: (name: string) => api.call<null>('delete_graph_view', { name }),
  graphPositions: () => api.call<NodePos[]>('graph_positions'),
  saveGraphPositions: (positions: NodePos[]) => api.call<null>('save_graph_positions', { positions }),
  search: (query: string, limit = 30) => api.call<SearchHit[]>('search', { query, limit }),
  listMedia: () => api.call<MediaItem[]>('list_media'),
  getMedia: (id: string) => api.call<MediaItem>('get_media', { id }),
  updateMedia: (id: string, patch: { title?: string; recordedAt?: number }) => api.call<MediaItem>('update_media', { id, ...patch }),
  deleteMedia: (id: string) => api.call<null>('delete_media', { id }),
  attachTranscript: (mediaId: string, fileName: string, text: string) => api.call<Transcript>('attach_transcript', { mediaId, fileName, text }),
  getTranscript: (mediaId: string) => api.call<Transcript | null>('get_transcript', { mediaId }),
  removeTranscript: (mediaId: string) => api.call<null>('remove_transcript', { mediaId }),
  exportTranscript: (mediaId: string) => api.call<string>('export_transcript', { mediaId }),
  importStart: (paths: string[]) => api.call<{ jobId: string }>('import_start', { paths, tzOffsetMinutes: -new Date().getTimezoneOffset() }),
  importInline: (files: { path: string; text: string; modifiedMs?: number }[], vaults: string[] = []) =>
    api.call<ImportSummary>('import_inline', { files, vaults, tzOffsetMinutes: -new Date().getTimezoneOffset() }),
  jobStatus: (jobId: string) => api.call<JobInfo>('job_status', { jobId }),
  jobCancel: (jobId: string) => api.call<boolean>('job_cancel', { jobId }),
  jobList: () => api.call<JobInfo[]>('job_list'),
  asrStatus: () => api.call<AsrStatus>('asr_status'),
  asrSetSettings: (s: AsrSettings) => api.call<AsrStatus>('asr_set_settings', { ...s }),
  asrDeleteModel: (model: string) => api.call<AsrStatus>('asr_delete_model', { model }),
  asrDownloadStart: (model: string) => api.call<{ jobId: string }>('asr_download_start', { model }),
  transcribeStart: (mediaId: string, model?: string, language?: string) => api.call<{ jobId: string }>('transcribe_start', { mediaId, model, language }),
  exportMarkdown: () => api.call<ExportResult>('export_markdown'),
};

/** Reads a browser File into the library, using the file's last-modified date as "when it happened". */
export async function importBrowserFile(f: File): Promise<ImportOutcome> {
  return api.importFile(f.name, new Uint8Array(await f.arrayBuffer()), f.lastModified);
}
