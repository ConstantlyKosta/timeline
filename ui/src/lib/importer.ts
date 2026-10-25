// Importing Markdown notes: native dialogs + drag-drop (desktop app: real paths, the Rust side walks
// folders) and a browser fallback (File objects uploaded as text in chunks).
import { cmd, isTauri } from './api';
import type { ImportSummary, JobInfo } from './types';

export const NOTE_EXTS = ['md', 'markdown', 'mdown', 'txt'];
export const MEDIA_EXTS = ['wav', 'mp3', 'm4a', 'flac', 'ogg', 'aac', 'opus', 'mp4', 'mov', 'm4v', 'webm', 'mkv', 'png', 'jpg', 'jpeg', 'gif', 'webp', 'heic', 'avif'];

export const extOf = (name: string) => {
  const i = name.lastIndexOf('.');
  const slash = Math.max(name.lastIndexOf('/'), name.lastIndexOf('\\'));
  return i > slash ? name.slice(i + 1).toLowerCase() : '';
};

/** Splits dropped native paths into notes/folders (go to the Markdown importer) and media files. */
export function classifyPaths(paths: string[]): { notes: string[]; media: string[]; other: string[] } {
  const out = { notes: [] as string[], media: [] as string[], other: [] as string[] };
  for (const p of paths) {
    const e = extOf(p);
    if (NOTE_EXTS.includes(e) && e !== 'txt') out.notes.push(p);
    else if (MEDIA_EXTS.includes(e)) out.media.push(p);
    else if (e === '') out.notes.push(p); // no extension: probably a folder, the importer checks
    else out.other.push(p);
  }
  return out;
}

export const emptySummary = (): ImportSummary => ({
  filesSeen: 0, imported: 0, skippedDuplicate: 0, skippedOther: 0, errorCount: 0, skipped: [], errors: [],
  attachmentsImported: 0, attachmentsMissing: 0, linksResolved: 0, linksUnresolved: 0, folders: 0, vaults: [], cancelled: false, elapsedMs: 0, firstNoteId: null,
});

export function mergeSummary(a: ImportSummary, b: ImportSummary): ImportSummary {
  return {
    filesSeen: a.filesSeen + b.filesSeen,
    imported: a.imported + b.imported,
    skippedDuplicate: a.skippedDuplicate + b.skippedDuplicate,
    skippedOther: a.skippedOther + b.skippedOther,
    errorCount: a.errorCount + b.errorCount,
    skipped: [...a.skipped, ...b.skipped].slice(0, 200),
    errors: [...a.errors, ...b.errors].slice(0, 200),
    attachmentsImported: a.attachmentsImported + b.attachmentsImported,
    attachmentsMissing: a.attachmentsMissing + b.attachmentsMissing,
    linksResolved: a.linksResolved + b.linksResolved,
    linksUnresolved: a.linksUnresolved + b.linksUnresolved,
    folders: Math.max(a.folders, b.folders),
    vaults: [...new Set([...a.vaults, ...b.vaults])],
    cancelled: a.cancelled || b.cancelled,
    elapsedMs: a.elapsedMs + b.elapsedMs,
    firstNoteId: a.firstNoteId ?? b.firstNoteId,
  };
}

export interface Progress { fraction: number; message: string }
export interface Cancel { cancelled: boolean; jobId?: string }

/** Starts the native (path based) importer and polls until it finishes. */
export async function runPathImport(paths: string[], onProgress: (p: Progress) => void, cancel: Cancel): Promise<ImportSummary> {
  const { jobId } = await cmd.importStart(paths);
  cancel.jobId = jobId;
  for (;;) {
    if (cancel.cancelled) await cmd.jobCancel(jobId).catch(() => undefined);
    const s: JobInfo = await cmd.jobStatus(jobId);
    onProgress({ fraction: s.progress, message: s.message });
    if (s.state === 'error') throw new Error(s.error ?? 'import failed');
    if (s.state !== 'running') return { ...emptySummary(), ...(s.result as ImportSummary) };
    await new Promise((r) => setTimeout(r, 200));
  }
}

export interface PickedFile { path: string; file: File }

const CHUNK_FILES = 150;
const CHUNK_BYTES = 4_000_000;

/** Browser fallback: reads files as text and uploads them in chunks. Directory structure comes from `path`. */
export async function runInlineImport(files: PickedFile[], onProgress: (p: Progress) => void, cancel: Cancel): Promise<ImportSummary> {
  let total = emptySummary();
  const vaults = new Set<string>();
  const notes = files.filter((f) => NOTE_EXTS.includes(extOf(f.path)));
  const nonNotes = files.filter((f) => !NOTE_EXTS.includes(extOf(f.path)) && !f.path.split('/').some((seg) => seg.startsWith('.'))).length;
  for (const f of files) {
    const seg = f.path.split('/');
    const i = seg.indexOf('.obsidian');
    if (i >= 0) vaults.add(i > 0 ? seg.slice(0, i).join('/') : 'vault');
  }
  let chunk: { path: string; text: string; modifiedMs?: number }[] = [];
  let bytes = 0; let done = 0;
  const flush = async () => {
    if (!chunk.length) return;
    total = mergeSummary(total, await cmd.importInline(chunk, [...vaults]));
    chunk = []; bytes = 0;
  };
  for (const f of notes) {
    if (cancel.cancelled) { total.cancelled = true; break; }
    chunk.push({ path: f.path, text: await f.file.text(), modifiedMs: f.file.lastModified });
    bytes += f.file.size; done++;
    if (chunk.length >= CHUNK_FILES || bytes >= CHUNK_BYTES) { await flush(); onProgress({ fraction: done / notes.length, message: `Importing ${done} of ${notes.length}…` }); }
  }
  await flush();
  if (nonNotes) {
    total.skippedOther += nonNotes;
    total.filesSeen += nonNotes;
    total.skipped.push({ path: `${nonNotes} other file${nonNotes === 1 ? '' : 's'}`, reason: 'not Markdown. Images and other attachments are only imported by the desktop app.' });
  }
  return total;
}

// ── browser drag-drop / folder pickers ──────────────────────────────────────────────────────────

interface FsEntry { isFile: boolean; isDirectory: boolean; name: string; file?: (cb: (f: File) => void, err: (e: unknown) => void) => void; createReader?: () => { readEntries: (cb: (e: FsEntry[]) => void, err: (e: unknown) => void) => void } }

async function walkEntry(e: FsEntry, prefix: string, out: PickedFile[]): Promise<void> {
  if (e.isFile && e.file) {
    const file = await new Promise<File>((res, rej) => e.file!(res, rej));
    out.push({ path: prefix + e.name, file });
  } else if (e.isDirectory && e.createReader) {
    if (e.name.startsWith('.') && prefix !== '' ) { if (e.name === '.obsidian') out.push({ path: `${prefix}.obsidian/.keep`, file: new File([], '.keep') }); return; }
    const reader = e.createReader();
    for (;;) {
      const batch = await new Promise<FsEntry[]>((res, rej) => reader.readEntries(res, rej));
      if (!batch.length) break;
      for (const c of batch) await walkEntry(c, prefix + e.name + '/', out);
    }
  }
}

/** Collects dropped files, descending into folders. Paths are relative to the dropped folder (root name removed). Must be called synchronously inside the drop event for `items`, then awaited. */
export function collectDropped(dt: DataTransfer): Promise<{ files: PickedFile[]; media: File[] }> {
  const entries: (FsEntry | null)[] = [];
  const plain: File[] = [];
  if (dt.items && dt.items.length) {
    for (const it of Array.from(dt.items)) {
      if (it.kind !== 'file') continue;
      const e = (it as unknown as { webkitGetAsEntry?: () => FsEntry | null }).webkitGetAsEntry?.() ?? null;
      entries.push(e);
      if (!e) { const f = it.getAsFile(); if (f) plain.push(f); }
    }
  } else plain.push(...Array.from(dt.files));
  return (async () => {
    const files: PickedFile[] = [];
    for (const e of entries) {
      if (!e) continue;
      if (e.isDirectory) {
        const sub: PickedFile[] = [];
        await walkEntry(e, '', sub);
        // drop the root folder name from the relative path
        files.push(...sub.map((s) => ({ ...s, path: s.path.split('/').slice(1).join('/') })));
      } else await walkEntry(e, '', files);
    }
    for (const f of plain) files.push({ path: f.name, file: f });
    const media: File[] = [];
    const rest: PickedFile[] = [];
    for (const f of files) {
      // loose media files dropped directly (no folder) go to the media library; media inside folders are ignored by the note importer
      if (!f.path.includes('/') && MEDIA_EXTS.includes(extOf(f.path))) media.push(f.file);
      else rest.push(f);
    }
    return { files: rest.filter((f) => !f.path.endsWith('/.keep') || f.path.includes('.obsidian')), media };
  })();
}

/** Files from `<input type=file webkitdirectory>`: webkitRelativePath is "Root/sub/file.md". */
export function fromInput(list: FileList): PickedFile[] {
  return Array.from(list).map((f) => {
    const rel = (f as File & { webkitRelativePath?: string }).webkitRelativePath;
    return { path: rel ? rel.split('/').slice(1).join('/') || f.name : f.name, file: f };
  });
}

// ── native dialogs (desktop app) ─────────────────────────────────────────────────────────────────

export async function pickNativeFiles(): Promise<string[]> {
  const { open } = await import('@tauri-apps/plugin-dialog');
  const r = await open({ multiple: true, directory: false, title: 'Import Markdown notes', filters: [{ name: 'Markdown', extensions: ['md', 'markdown', 'mdown', 'txt'] }] });
  return r ? (Array.isArray(r) ? r : [r]) : [];
}
export async function pickNativeFolders(): Promise<string[]> {
  const { open } = await import('@tauri-apps/plugin-dialog');
  const r = await open({ multiple: true, directory: true, title: 'Import a folder of notes (e.g. an Obsidian vault)' });
  return r ? (Array.isArray(r) ? r : [r]) : [];
}

export async function importMediaPaths(paths: string[]): Promise<{ imported: number; duplicates: number; errors: string[] }> {
  const { invoke } = await import('@tauri-apps/api/core');
  const res = await invoke<{ name: string; ok?: { duplicate: boolean }; error?: string }[]>('import_media_paths', { paths });
  return { imported: res.filter((r) => r.ok && !r.ok.duplicate).length, duplicates: res.filter((r) => r.ok?.duplicate).length, errors: res.filter((r) => r.error).map((r) => `${r.name}: ${r.error}`) };
}

export { isTauri };
