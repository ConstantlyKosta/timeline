import { useCallback, useEffect, useRef, useState } from 'react';
import { format } from 'date-fns';
import { AudioLines, Captions, Download, Film, Image as ImageIcon, Mic, Trash2, Upload, X } from 'lucide-react';
import { cmd, importBrowserFile, isTauri } from '../lib/api';
import { useStore } from '../lib/store';
import type { MediaItem, Transcript } from '../lib/types';
import { MediaPlayer, type PlayerHandle } from '../components/MediaPlayer';
import { AsrPanel } from '../components/AsrPanel';
import { useAsr } from '../lib/asrStore';

const fmt = (s: number) => `${Math.floor(s / 60)}:${String(Math.floor(s % 60)).padStart(2, '0')}`;
const Icon = ({ k }: { k: MediaItem['kind'] }) => (k === 'audio' ? <AudioLines size={16} /> : k === 'video' ? <Film size={16} /> : <ImageIcon size={16} />);

export function LibraryView() {
  const { media, selectedMediaId, openMedia, refresh, fail, notify } = useStore();
  const [drag, setDrag] = useState(false);
  const input = useRef<HTMLInputElement>(null);

  const importFiles = useCallback(async (files: FileList | File[]) => {
    let last: string | null = null; let dup = 0; let n = 0;
    const fresh: MediaItem[] = [];
    for (const f of Array.from(files)) {
      try { const o = await importBrowserFile(f); last = o.item.id; if (o.duplicate) dup++; else { n++; fresh.push(o.item); } } catch (e) { fail(e); }
    }
    await refresh();
    if (last) openMedia(last);
    notify(`Imported ${n} file${n === 1 ? '' : 's'}${dup ? ` (${dup} already in library)` : ''}`);
    useAsr.getState().autoTranscribe(fresh); // only when the user switched it on in settings
  }, [fail, refresh, openMedia, notify]);

  useEffect(() => { if (!selectedMediaId && media.length) openMedia(media[0].id); }, [selectedMediaId, media, openMedia]);
  const selected = media.find((m) => m.id === selectedMediaId) ?? null;

  return (
    <div className={`library ${drag ? 'drag' : ''}`} data-has-item={!!selected} data-testid="library-view"
      onDragOver={(e) => { e.preventDefault(); setDrag(true); }} onDragLeave={() => setDrag(false)}
      onDrop={() => setDrag(false)}>
      <aside className="note-list">
        <div className="note-list-head">
          <button className="primary wide" onClick={() => input.current?.click()} data-testid="import-media"><Upload size={15} /> Import audio / video / photos</button>
          <input ref={input} type="file" multiple hidden accept="audio/*,video/*,image/*,.wav,.mp3,.m4a,.flac,.ogg" data-testid="import-input"
            onChange={(e) => { if (e.target.files) importFiles(e.target.files); e.target.value = ''; }} />
        </div>
        <ul>
          {media.map((m) => (
            <li key={m.id}>
              <button className={`note-item ${m.id === selectedMediaId ? 'on' : ''}`} onClick={() => openMedia(m.id)} data-testid="media-item">
                <b><Icon k={m.kind} /> {m.title}</b>
                <small>{format(new Date(m.recordedAt), 'd MMM yyyy, HH:mm')}{m.duration ? ` · ${fmt(m.duration)}` : ''}{m.hasTranscript ? ' · transcript' : ''}</small>
              </button>
            </li>
          ))}
          {!media.length && <li className="empty-small">Drop files here to import. Recordings are copied into your private library and de-duplicated.</li>}
        </ul>
        <AsrPanel />
      </aside>
      {selected ? <MediaDetail key={selected.id} item={selected} /> : <div className="note-pane empty-pane"><AudioLines size={28} /><p>Import a recording to see it here.</p></div>}
      {drag && <div className="drop-overlay">Drop to import</div>}
    </div>
  );
}

function MediaDetail({ item }: { item: MediaItem }) {
  const { refresh, fail, notify, openMedia, seekTo } = useStore();
  const [tr, setTr] = useState<Transcript | null>(null);
  const [time, setTime] = useState(0);
  const [title, setTitle] = useState(item.title);
  const player = useRef<PlayerHandle>(null);
  const trInput = useRef<HTMLInputElement>(null);
  const [confirm, setConfirm] = useState(false);
  const [rearm, setRearm] = useState(false);
  const rearmTimer = useRef<ReturnType<typeof setTimeout>>();
  useEffect(() => () => clearTimeout(rearmTimer.current), []);
  const { status, jobs, waiting, version, load, transcribe, cancel } = useAsr();
  const job = jobs.find((j) => j.state === 'running' && ((j.kind === 'transcribe' && j.subject === item.id) || (j.kind === 'asr-download' && j.subject === waiting[item.id])));
  const busy = !!job || !!waiting[item.id];

  useEffect(() => { load(); }, [load]);
  useEffect(() => { cmd.getTranscript(item.id).then(setTr).catch(fail); }, [item.id, item.hasTranscript, version, fail]);
  useEffect(() => {
    if (seekTo == null) return;
    const h = setTimeout(() => { player.current?.seek(seekTo, true); useStore.setState({ seekTo: null }); }, 700);
    return () => clearTimeout(h);
  }, [seekTo, item.id]);

  const active = tr?.segments.findIndex((s) => time >= s.start && time < s.end) ?? -1;
  const activeRef = useRef<HTMLLIElement>(null);
  useEffect(() => { activeRef.current?.scrollIntoView({ block: 'nearest' }); }, [active]);

  const attach = async (f: File) => {
    try { const t = await cmd.attachTranscript(item.id, f.name, await f.text()); setTr(t); await refresh(); notify(`Transcript attached (${t.segments.length} segments)`); } catch (e) { fail(e); }
  };
  const download = async () => {
    const text = await cmd.exportTranscript(item.id);
    if (isTauri()) {
      try { // native save dialog (WKWebView ignores <a download>); falls through to the anchor on mobile
        const { save } = await import('@tauri-apps/plugin-dialog');
        const { invoke } = await import('@tauri-apps/api/core');
        const path = await save({ defaultPath: `${item.title}.txt`, filters: [{ name: 'Text', extensions: ['txt'] }] });
        if (path) await invoke('save_text', { path, text });
        return;
      } catch { /* mobile: dialog save unsupported */ }
    }
    const a = document.createElement('a');
    a.href = URL.createObjectURL(new Blob([text], { type: 'text/plain' }));
    a.download = `${item.title}.txt`; a.click();
  };

  return (
    <section className="note-pane media-pane" data-testid="media-detail">
      <header className="note-head">
        <input className="title-input" value={title} data-testid="media-title" onChange={(e) => setTitle(e.target.value)}
          onBlur={() => title !== item.title && cmd.updateMedia(item.id, { title }).then(refresh).catch(fail)} />
        <span className="muted small">{format(new Date(item.recordedAt), 'EEE d MMM yyyy, HH:mm')} · {(item.fileSize / 1024 / 1024).toFixed(2)} MB</span>
        <button className={`ghost danger ${confirm ? 'armed' : ''}`} onClick={() => {
          if (!confirm) { setConfirm(true); setTimeout(() => setConfirm(false), 3000); return; }
          cmd.deleteMedia(item.id).then(async () => { openMedia(null); await refresh(); }).catch(fail);
        }}><Trash2 size={14} /> {confirm ? 'Click again to delete' : ''}</button>
      </header>
      {item.isMissing && <p className="error">The file is missing from the library folder.</p>}
      <MediaPlayer ref={player} item={item} onTime={setTime} />
      {item.kind !== 'photo' && (
        <div className="transcript" data-testid="transcript">
          <div className="transcript-head">
            <h4><Captions size={15} /> Transcript {tr?.language && <small data-testid="transcript-language">{tr.language}</small>} {tr?.hasSynthesizedTiming && <small title="The file had no timestamps; timing is estimated.">estimated timing</small>}</h4>
            <span className="spacer" />
            {status?.available && !busy && (
              <button className={`ghost ${rearm ? 'armed' : ''}`} data-testid="transcribe" title="Transcribe on this computer with whisper.cpp"
                onClick={() => {
                  if (tr && !rearm) { setRearm(true); clearTimeout(rearmTimer.current); rearmTimer.current = setTimeout(() => setRearm(false), 3000); return; }
                  clearTimeout(rearmTimer.current); setRearm(false); transcribe(item.id);
                }}>
                <Mic size={14} /> {rearm ? 'Replace current transcript?' : tr ? 'Re-transcribe' : 'Transcribe'}
              </button>
            )}
            <button className="ghost" onClick={() => trInput.current?.click()} data-testid="attach-transcript">{tr ? 'Replace' : 'Attach'} (.srt .vtt .txt .json)</button>
            <input ref={trInput} type="file" hidden accept=".srt,.vtt,.txt,.json" data-testid="transcript-input" onChange={(e) => { const f = e.target.files?.[0]; if (f) attach(f); e.target.value = ''; }} />
            {tr && <button className="ghost" onClick={download}><Download size={14} /></button>}
            {tr && <button className="ghost" onClick={() => cmd.removeTranscript(item.id).then(() => { setTr(null); refresh(); }).catch(fail)}>Remove</button>}
          </div>
          {busy && (
            <div className="asr-job" data-testid="transcribe-progress">
              <div className="bar"><i style={{ width: `${Math.round((job?.progress ?? 0) * 100)}%` }} /></div>
              <small>{job?.message ?? 'Starting…'}</small>
              <button className="ghost" data-testid="transcribe-cancel" onClick={() => job ? cancel(job.id) : undefined}><X size={13} /> Cancel</button>
            </div>
          )}
          {tr ? (
            <ol className="segments">
              {tr.segments.map((s, i) => (
                <li key={s.id} ref={i === active ? activeRef : undefined} className={i === active ? 'active' : ''} data-testid="segment">
                  <button onClick={() => player.current?.seek(s.start, true)}><time>{fmt(s.start)}</time><span>{s.speaker && <b>{s.speaker}: </b>}{s.text}</span></button>
                </li>
              ))}
            </ol>
          ) : <p className="muted">No transcript yet. Press Transcribe to create one on this computer (a speech model is downloaded the first time), or attach a subtitle or transcript file (.srt .vtt .txt .json) from another tool. Transcripts are searchable by meaning.</p>}
        </div>
      )}
    </section>
  );
}
