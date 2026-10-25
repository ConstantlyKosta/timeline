import { create } from 'zustand';
import { cmd } from './api';
import { useStore } from './store';
import type { AsrSettings, AsrStatus, JobInfo, MediaItem, TranscribeResult } from './types';

const KINDS = ['transcribe', 'asr-download'];

interface AsrState {
  status: AsrStatus | null;
  /** Running transcription / model-download jobs. */
  jobs: JobInfo[];
  /** mediaId -> model being downloaded first; transcription starts when it finishes. */
  waiting: Record<string, string>;
  /** bumped whenever a transcription finishes so open transcript views reload */
  version: number;
  load(): Promise<void>;
  setSettings(patch: Partial<AsrSettings>): Promise<void>;
  transcribe(mediaId: string): Promise<void>;
  download(model: string): Promise<void>;
  deleteModel(model: string): Promise<void>;
  cancel(jobId: string): Promise<void>;
  /** Called after new media was imported: transcribes it when the user turned that on. */
  autoTranscribe(items: MediaItem[]): Promise<void>;
}

let timer: ReturnType<typeof setTimeout> | undefined;

export const useAsr = create<AsrState>((set, get) => {
  const notify = (m: string) => useStore.getState().notify(m);

  const tick = async () => {
    timer = undefined;
    let all: JobInfo[];
    try { all = (await cmd.jobList()).filter((j) => KINDS.includes(j.kind)); } catch { return; }
    const before = get().jobs;
    const running = all.filter((j) => j.state === 'running');
    set({ jobs: running });
    for (const prev of before) {
      const now = all.find((j) => j.id === prev.id);
      if (!now || now.state === 'running') continue;
      await finished(now);
    }
    if (running.length) timer = setTimeout(tick, 400);
  };
  const ensurePolling = () => { if (!timer) timer = setTimeout(tick, 150); };

  const finished = async (j: JobInfo) => {
    if (j.kind === 'asr-download') {
      const model = j.subject ?? '';
      await get().load();
      if (j.state === 'error') notify(j.error ?? 'Model download failed');
      else if (j.state === 'cancelled') notify('Model download cancelled');
      else notify('Speech model downloaded');
      const waiting = Object.entries(get().waiting).filter(([, m]) => m === model).map(([id]) => id);
      set((s) => ({ waiting: Object.fromEntries(Object.entries(s.waiting).filter(([, m]) => m !== model)) }));
      if (j.state === 'done') for (const id of waiting) await get().transcribe(id);
    } else {
      await useStore.getState().refresh();
      set((s) => ({ version: s.version + 1 }));
      if (j.state === 'done') {
        const r = j.result as TranscribeResult;
        notify(r.segments ? `Transcribed ${r.segments} segment${r.segments === 1 ? '' : 's'}${r.language ? ` (${r.language})` : ''} in ${r.elapsedSeconds.toFixed(1)}s` : 'No speech found in this recording');
      } else if (j.state === 'error') notify(j.error ?? 'Transcription failed');
      else notify('Transcription cancelled');
    }
  };

  return {
    status: null, jobs: [], waiting: {}, version: 0,
    async load() {
      try {
        const status = await cmd.asrStatus();
        set({ status });
        const running = (await cmd.jobList()).filter((j) => KINDS.includes(j.kind) && j.state === 'running');
        if (running.length) { set({ jobs: running }); ensurePolling(); }
      } catch (e) { useStore.getState().fail(e); }
    },
    async setSettings(patch) {
      const cur = get().status?.settings;
      if (!cur) return;
      const next = { ...cur, ...patch };
      set((s) => ({ status: s.status && { ...s.status, settings: next } })); // optimistic, so the control reacts at once
      try { set({ status: await cmd.asrSetSettings(next) }); } catch (e) { useStore.getState().fail(e); await get().load(); }
    },
    async transcribe(mediaId) {
      const st = get().status ?? (await cmd.asrStatus());
      if (!st.available) { notify('Speech-to-text is not available in this build'); return; }
      if (get().jobs.some((j) => j.kind === 'transcribe' && j.subject === mediaId)) return;
      const model = st.models.find((m) => m.id === st.settings.model);
      try {
        if (model && !model.installed) {
          set((s) => ({ waiting: { ...s.waiting, [mediaId]: model.id } }));
          if (!get().jobs.some((j) => j.kind === 'asr-download' && j.subject === model.id)) await get().download(model.id);
          return;
        }
        const { jobId } = await cmd.transcribeStart(mediaId);
        const info = await cmd.jobStatus(jobId);
        set((s) => ({ jobs: [...s.jobs, info] }));
        ensurePolling();
      } catch (e) { useStore.getState().fail(e); }
    },
    async download(model) {
      try {
        const { jobId } = await cmd.asrDownloadStart(model);
        const info = await cmd.jobStatus(jobId);
        set((s) => ({ jobs: [...s.jobs, info] }));
        ensurePolling();
      } catch (e) { useStore.getState().fail(e); }
    },
    async deleteModel(model) {
      try { set({ status: await cmd.asrDeleteModel(model) }); } catch (e) { useStore.getState().fail(e); }
    },
    async cancel(jobId) {
      await cmd.jobCancel(jobId).catch(() => undefined);
      // a transcription waiting on a download is dropped when that download is cancelled
      const j = get().jobs.find((x) => x.id === jobId);
      if (j?.kind === 'asr-download') set((s) => ({ waiting: Object.fromEntries(Object.entries(s.waiting).filter(([, m]) => m !== j.subject)) }));
    },
    async autoTranscribe(items) {
      const st = get().status ?? (await cmd.asrStatus().catch(() => null));
      if (!st?.available || !st.settings.autoTranscribe) return;
      for (const m of items) if (m.kind !== 'photo' && !m.hasTranscript) await get().transcribe(m.id);
    },
  };
});
