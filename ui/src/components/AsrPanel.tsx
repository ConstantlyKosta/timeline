import { useEffect } from 'react';
import { Download, Mic, Trash2, X } from 'lucide-react';
import { useAsr } from '../lib/asrStore';

const LANGS: [string, string][] = [['auto', 'Auto-detect'], ['en', 'English'], ['ru', 'Russian'], ['de', 'German'], ['fr', 'French'], ['es', 'Spanish'], ['it', 'Italian'], ['pt', 'Portuguese'], ['uk', 'Ukrainian'], ['he', 'Hebrew'], ['ja', 'Japanese'], ['zh', 'Chinese']];
const mb = (n: number) => (n >= 1e9 ? `${(n / 1e9).toFixed(1)} GB` : `${Math.round(n / 1e6)} MB`);

/** Speech-to-text settings: model (download / delete), language, auto-transcribe. */
export function AsrPanel() {
  const { status, jobs, load, setSettings, download, deleteModel, cancel } = useAsr();
  useEffect(() => { load(); }, [load]);
  if (!status) return null;
  if (!status.available) return <div className="asr-panel muted small">Speech-to-text is not available in this build.</div>;
  const s = status.settings;
  const cur = status.models.find((m) => m.id === s.model);
  return (
    <details className="asr-panel" data-testid="asr-panel">
      <summary><Mic size={14} /> Speech-to-text <small className="muted">{cur ? `${cur.label}${cur.installed ? '' : ' (not downloaded)'}` : ''}</small></summary>
      <p className="muted small">Runs on this computer with whisper.cpp. Recordings never leave your machine; a model is downloaded once from Hugging Face.</p>
      <label className="asr-row">Language
        <select value={s.language} data-testid="asr-language" onChange={(e) => setSettings({ language: e.target.value })}>
          {LANGS.map(([c, n]) => <option key={c} value={c} disabled={c !== 'auto' && c !== 'en' && !!cur && !cur.multilingual}>{n}</option>)}
        </select>
      </label>
      <label className="asr-row check"><input type="checkbox" checked={s.autoTranscribe} data-testid="asr-auto" onChange={(e) => setSettings({ autoTranscribe: e.target.checked })} /> Transcribe new recordings automatically</label>
      <ul className="asr-models">
        {status.models.map((m) => {
          const job = jobs.find((j) => j.kind === 'asr-download' && j.subject === m.id && j.state === 'running');
          return (
            <li key={m.id} className={m.id === s.model ? 'on' : ''} data-testid="asr-model" data-model={m.id}>
              <label>
                <input type="radio" name="asr-model" checked={m.id === s.model} onChange={() => setSettings({ model: m.id, ...(!m.multilingual && s.language !== 'en' ? { language: 'auto' } : {}) })} />
                <span><b>{m.label}</b> <small className="muted">{mb(m.sizeBytes)}</small><br /><small className="muted">{m.description}</small></span>
              </label>
              {job ? (
                <div className="asr-dl"><div className="bar" data-testid="asr-download-progress"><i style={{ width: `${Math.round(job.progress * 100)}%` }} /></div><small>{job.message}</small>
                  <button className="ghost" title="Cancel download" onClick={() => cancel(job.id)}><X size={13} /></button></div>
              ) : m.installed ? (
                <button className="ghost" title="Delete downloaded model" onClick={() => deleteModel(m.id)} data-testid="asr-delete"><Trash2 size={13} /></button>
              ) : (
                <button className="ghost" onClick={() => download(m.id)} data-testid="asr-download"><Download size={13} /> Download</button>
              )}
            </li>
          );
        })}
      </ul>
    </details>
  );
}
