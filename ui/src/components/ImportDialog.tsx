import { useRef } from 'react';
import { CheckCircle2, FileDown, FolderInput, X } from 'lucide-react';
import { useStore } from '../lib/store';
import { collectDropped, fromInput, importMediaPaths, isTauri, pickNativeFiles, pickNativeFolders, runInlineImport, runPathImport, classifyPaths, type PickedFile } from '../lib/importer';
import { importBrowserFile } from '../lib/api';

/** Entry points: native dialogs on desktop, file inputs in the browser. */
export function useImportActions() {
  const { runImport, fail, notify, refresh } = useStore();
  const files = useRef<HTMLInputElement | null>(null);
  const dir = useRef<HTMLInputElement | null>(null);

  const importPaths = (paths: string[]) => runImport((p, c) => runPathImport(paths, p, c));
  const importPicked = (picked: PickedFile[]) => runImport((p, c) => runInlineImport(picked, p, c));

  const pickFiles = async () => {
    if (isTauri()) { try { const p = await pickNativeFiles(); if (p.length) await importPaths(p); } catch (e) { fail(e); } } else files.current?.click();
  };
  const pickFolder = async () => {
    if (isTauri()) { try { const p = await pickNativeFolders(); if (p.length) await importPaths(p); } catch (e) { fail(e); } } else dir.current?.click();
  };
  /** Files/folders dropped from the OS (desktop: native paths). */
  const dropPaths = async (paths: string[]) => {
    const c = classifyPaths(paths);
    try {
      if (c.media.length) { const r = await importMediaPaths(c.media); await refresh(); notify(`Imported ${r.imported} media file${r.imported === 1 ? '' : 's'}${r.duplicates ? ` (${r.duplicates} already in library)` : ''}${r.errors.length ? `, ${r.errors.length} failed` : ''}`); }
      if (c.notes.length) await importPaths(c.notes);
      else if (!c.media.length) notify('Nothing to import: drop .md files, folders, or audio/video/photos');
    } catch (e) { fail(e); }
  };
  const dropBrowser = async (dt: DataTransfer) => {
    const got = await collectDropped(dt);
    for (const m of got.media) { try { await importBrowserFile(m); } catch (e) { fail(e); } }
    if (got.media.length) { await refresh(); notify(`Imported ${got.media.length} media file${got.media.length === 1 ? '' : 's'}`); }
    if (got.files.length) await importPicked(got.files);
    else if (!got.media.length) notify('Nothing to import: drop .md files, folders, or audio/video/photos');
  };

  const inputs = (
    <>
      <input ref={files} type="file" multiple hidden accept=".md,.markdown,.mdown,.txt" data-testid="import-notes-input"
        onChange={(e) => { if (e.target.files?.length) importPicked(fromInput(e.target.files)); e.target.value = ''; }} />
      <input ref={dir} type="file" hidden data-testid="import-folder-input"
        {...({ webkitdirectory: '', directory: '' } as Record<string, string>)}
        onChange={(e) => { if (e.target.files?.length) importPicked(fromInput(e.target.files)); e.target.value = ''; }} />
    </>
  );
  return { pickFiles, pickFolder, dropPaths, dropBrowser, inputs };
}

export function ImportDialog() {
  const { importing, cancelImport, closeImport, openNote } = useStore();
  if (!importing) return null;
  const { running, fraction, message, summary: s, error } = importing;
  return (
    <div className="modal-back" data-testid="import-dialog" role="dialog" aria-label="Import notes">
      <div className="modal import-modal">
        <header>
          <h3>{running ? 'Importing notes…' : error ? 'Import failed' : s?.cancelled ? 'Import cancelled' : 'Import finished'}</h3>
          {!running && <button className="ghost" onClick={closeImport} aria-label="Close" data-testid="import-close"><X size={16} /></button>}
        </header>
        {running && (
          <div className="import-progress">
            <div className="bar"><i style={{ width: `${Math.round(fraction * 100)}%` }} /></div>
            <p className="muted">{message}</p>
            <button onClick={cancelImport} data-testid="import-cancel">Cancel</button>
          </div>
        )}
        {error && <p className="error">{error}</p>}
        {s && !running && (
          <div className="import-summary" data-testid="import-summary">
            <ul className="stats">
              <li data-testid="sum-imported"><b>{s.imported}</b> imported</li>
              <li data-testid="sum-duplicates"><b>{s.skippedDuplicate}</b> skipped (already imported)</li>
              {s.skippedOther > 0 && <li data-testid="sum-other"><b>{s.skippedOther}</b> skipped (other)</li>}
              <li data-testid="sum-errors" className={s.errorCount ? 'bad' : ''}><b>{s.errorCount}</b> errors</li>
            </ul>
            <p className="muted small">
              {s.folders > 0 && <>{s.folders} folder{s.folders === 1 ? '' : 's'} · </>}
              {s.attachmentsImported} image{s.attachmentsImported === 1 ? '' : 's'} attached{s.attachmentsMissing > 0 && ` (${s.attachmentsMissing} not found)`} ·
              {' '}{s.linksResolved} link{s.linksResolved === 1 ? '' : 's'} connected{s.linksUnresolved > 0 && `, ${s.linksUnresolved} unresolved`} ·
              {' '}{(s.elapsedMs / 1000).toFixed(1)} s
              {s.vaults.length > 0 && <> · Obsidian vault detected, its <code>.obsidian</code> settings were skipped</>}
            </p>
            {(s.errors.length > 0 || s.skipped.length > 0) && (
              <details open={s.errors.length > 0}>
                <summary>Details</summary>
                <ul className="issues">
                  {s.errors.map((e, i) => <li key={`e${i}`} className="bad"><code>{e.path}</code> {e.reason}</li>)}
                  {s.skipped.map((e, i) => <li key={`s${i}`}><code>{e.path}</code> {e.reason}</li>)}
                  {s.errorCount + s.skippedDuplicate + s.skippedOther > s.errors.length + s.skipped.length && <li className="muted">…and more</li>}
                </ul>
              </details>
            )}
            <footer>
              {s.firstNoteId && <button className="primary" onClick={() => { openNote(s.firstNoteId); closeImport(); }} data-testid="import-open-first"><CheckCircle2 size={14} /> Open first imported note</button>}
              <button onClick={closeImport} data-testid="import-done">Close</button>
            </footer>
          </div>
        )}
      </div>
    </div>
  );
}

export function ImportButtons({ actions }: { actions: ReturnType<typeof useImportActions> }) {
  return (
    <div className="import-buttons">
      <button className="ghost" onClick={actions.pickFiles} data-testid="import-notes" title="Import Markdown files"><FileDown size={14} /> Import .md files</button>
      <button className="ghost" onClick={actions.pickFolder} data-testid="import-folder" title="Import a folder or Obsidian vault"><FolderInput size={14} /> Import folder</button>
      {actions.inputs}
    </div>
  );
}
