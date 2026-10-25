import { useEffect, useRef } from 'react';
import { AudioLines, CalendarClock, Moon, Network, Search, StickyNote, Sun, Command as CmdIcon } from 'lucide-react';
import { useStore, type View } from './lib/store';
import { TimelineView } from './views/TimelineView';
import { NotesView } from './views/NotesView';
import { GraphView } from './views/GraphView';
import { SearchView } from './views/SearchView';
import { LibraryView } from './views/LibraryView';
import { QuickSwitcher } from './components/QuickSwitcher';
import { ImportDialog, useImportActions } from './components/ImportDialog';
import { isTauri } from './lib/api';
import { cmd } from './lib/api';

const NAV: { id: View; label: string; icon: typeof Search }[] = [
  { id: 'timeline', label: 'Timeline', icon: CalendarClock },
  { id: 'notes', label: 'Notes', icon: StickyNote },
  { id: 'graph', label: 'Mind map', icon: Network },
  { id: 'search', label: 'Search', icon: Search },
  { id: 'library', label: 'Library', icon: AudioLines },
];

export function App() {
  const { ready, error, view, setView, theme, setTheme, init, toast, setSwitcher, stats, entries, refresh, fail, notify } = useStore();
  useEffect(() => { init(); }, [init]);
  const importer = useImportActions();
  const dropPaths = useRef(importer.dropPaths); dropPaths.current = importer.dropPaths;
  const dropBrowser = useRef(importer.dropBrowser); dropBrowser.current = importer.dropBrowser;
  useEffect(() => {
    // Desktop: the webview hands us real file paths (folders included) via Tauri's drag-drop event.
    if (isTauri()) {
      let un: (() => void) | undefined; let dead = false;
      import('@tauri-apps/api/webview').then(({ getCurrentWebview }) => getCurrentWebview().onDragDropEvent((e) => {
        if (e.payload.type === 'drop') dropPaths.current(e.payload.paths);
      })).then((f) => { if (dead) f(); else un = f; }).catch(() => undefined);
      return () => { dead = true; un?.(); };
    }
    // Browser: HTML5 drop, descending into dropped folders.
    const over = (e: DragEvent) => { if (e.dataTransfer?.types.includes('Files')) e.preventDefault(); };
    const drop = (e: DragEvent) => { if (!e.dataTransfer?.types.includes('Files')) return; e.preventDefault(); dropBrowser.current(e.dataTransfer); };
    window.addEventListener('dragover', over); window.addEventListener('drop', drop);
    return () => { window.removeEventListener('dragover', over); window.removeEventListener('drop', drop); };
  }, []);
  useEffect(() => { document.documentElement.dataset.theme = theme; }, [theme]);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = e.metaKey || e.ctrlKey;
      if (mod && e.key.toLowerCase() === 'k') { e.preventDefault(); setSwitcher(!useStore.getState().switcherOpen); }
      else if (mod && e.key.toLowerCase() === 'n' && !e.shiftKey) { e.preventDefault(); useStore.getState().newNote().catch(fail); }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [setSwitcher, fail]);

  if (error) return <div className="boot error-screen" data-testid="boot-error"><h2>Timeline couldn’t start</h2><pre>{error}</pre></div>;
  if (!ready) return <div className="boot">Opening your library…</div>;

  const empty = entries.length === 0;
  return (
    <div className="app" data-testid="app" data-view={view}>
      <nav className="nav" aria-label="Main">
        <div className="brand" title="Timeline">⏳<span>Timeline</span></div>
        {NAV.map(({ id, label, icon: I }) => (
          <button key={id} className={view === id ? 'on' : ''} onClick={() => setView(id)} data-testid={`nav-${id}`}><I size={19} /><span>{label}</span></button>
        ))}
        <span className="spacer" />
        <button onClick={() => setSwitcher(true)} title="Quick switcher (Ctrl/Cmd+K)" data-testid="open-switcher"><CmdIcon size={19} /><span>Ctrl K</span></button>
        <button onClick={() => setTheme(theme === 'dark' ? 'light' : 'dark')} title="Toggle theme" data-testid="theme">{theme === 'dark' ? <Sun size={19} /> : <Moon size={19} />}<span>Theme</span></button>
      </nav>
      <main className="main">
        {empty && (
          <div className="welcome" data-testid="welcome">
            <b>Your timeline is empty.</b> Write a note or import a recording — or
            <button className="link" onClick={() => cmd.loadSample().then(() => refresh()).then(() => notify('Sample data loaded')).catch(fail)} data-testid="load-sample">load sample data</button>.
          </div>
        )}
        {view === 'timeline' && <TimelineView />}
        {view === 'notes' && <NotesView />}
        {view === 'graph' && <GraphView />}
        {view === 'search' && <SearchView />}
        {view === 'library' && <LibraryView />}
        <footer className="statusbar">
          <span>{stats?.notes ?? 0} notes · {stats?.recordings ?? 0} recordings · {stats?.photos ?? 0} photos · {stats?.transcripts ?? 0} transcripts</span>
          <span>local-only · search: {stats?.embedding}</span>
        </footer>
      </main>
      {toast && <div className="toast" role="status" data-testid="toast">{toast}</div>}
      <QuickSwitcher />
      <ImportDialog />
    </div>
  );
}
