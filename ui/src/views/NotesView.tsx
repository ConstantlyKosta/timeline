import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { format } from 'date-fns';
import { ArrowLeft, Bold, CalendarDays, Code, Download, FileText, Heading1, Image as ImageIcon, Italic, Link2, List, ListChecks, Plus, Quote, SquareCode, Strikethrough, Table2, Trash2 } from 'lucide-react';
import { cmd, importBrowserFile } from '../lib/api';
import { useStore } from '../lib/store';
import type { Note, NoteContext } from '../lib/types';
import { VirtualList } from '../components/VirtualList';
import { ImportButtons, useImportActions } from '../components/ImportDialog';
import { Editor, type EditorHandle } from '../editor/Editor';
import { insertCodeBlock, insertTable, linePrefix, wrap } from '../editor/commands';

const toLocalInput = (ms: number) => format(new Date(ms), "yyyy-MM-dd'T'HH:mm");

export function NotesView() {
  const { notes, tags, selectedNoteId, openNote, newNote, openByTitle, refresh, fail, notify, setView, setSearchQuery } = useStore();
  const importer = useImportActions();
  const [filter, setFilter] = useState('');
  const [tagFilter, setTagFilter] = useState<string | null>(null);

  const shown = useMemo(() => {
    const q = filter.trim().toLowerCase();
    return notes
      .filter((n) => (!tagFilter || n.tags.includes(tagFilter)) && (!q || n.title.toLowerCase().includes(q) || n.snippet.toLowerCase().includes(q)))
      .sort((a, b) => b.date - a.date);
  }, [notes, filter, tagFilter]);

  useEffect(() => {
    if (!selectedNoteId && shown.length && window.matchMedia('(min-width: 900px)').matches) openNote(shown[0].id);
  }, [selectedNoteId, shown, openNote]);

  return (
    <div className="notes" data-has-note={!!selectedNoteId} data-testid="notes-view">
      <aside className="note-list">
        <div className="note-list-head">
          <input placeholder="Filter notes…" value={filter} onChange={(e) => setFilter(e.target.value)} data-testid="note-filter" />
          <button className="primary" onClick={() => newNote().catch(fail)} data-testid="new-note" title="New note"><Plus size={15} /> New</button>
        </div>
        {tags.length > 0 && (
          <div className="tag-row">
            {tags.slice(0, 14).map((t) => (
              <button key={t.tag} className={`chip ${tagFilter === t.tag ? 'on' : ''}`} onClick={() => setTagFilter(tagFilter === t.tag ? null : t.tag)}>#{t.tag} <small>{t.count}</small></button>
            ))}
          </div>
        )}
        {shown.length > 0 ? (
          <VirtualList items={shown} rowHeight={84} keyOf={(n) => n.id} className="note-vlist" testId="note-list"
            render={(n) => (
              <button className={`note-item ${n.id === selectedNoteId ? 'on' : ''}`} onClick={() => openNote(n.id)} data-testid="note-item">
                <b>{n.title || 'Untitled'}</b>
                <small>{format(new Date(n.date), 'd MMM yyyy')}{n.folder ? ` · ${n.folder}` : ''}{n.taskCount > 0 && ` · ☑ ${n.doneCount}/${n.taskCount}`}</small>
                <span>{n.snippet}</span>
              </button>
            )} />
        ) : <ul><li className="empty-small">No notes match.</li></ul>}
        <ImportButtons actions={importer} />
        <button className="ghost export" onClick={() => cmd.exportMarkdown().then((r) => notify(`Exported ${r.count} notes to ${r.dir}`)).catch(fail)}><Download size={14} /> Export all as .md</button>
      </aside>
      {selectedNoteId ? (
        <NotePane key={selectedNoteId} id={selectedNoteId} onBack={() => openNote(null)} onTag={(t) => { setSearchQuery('#' + t); setView('search'); }} onOpenTitle={(t) => openByTitle(t).catch(fail)} onDeleted={() => { openNote(null); refresh(); }} />
      ) : (
        <div className="note-pane empty-pane"><FileText size={28} /><p>Select a note or create a new one.</p></div>
      )}
    </div>
  );
}

function NotePane({ id, onBack, onTag, onOpenTitle, onDeleted }: { id: string; onBack(): void; onTag(t: string): void; onOpenTitle(t: string): void; onDeleted(): void }) {
  const { refresh, fail, openNote } = useStore();
  const [note, setNote] = useState<Note | null>(null);
  const [ctx, setCtx] = useState<NoteContext | null>(null);
  const [status, setStatus] = useState<'saved' | 'saving' | 'error'>('saved');
  const [confirmDelete, setConfirmDelete] = useState(false);
  const editor = useRef<EditorHandle>(null);
  const pending = useRef<string | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);

  const loadCtx = useCallback(() => cmd.noteContext(id).then(setCtx).catch(() => undefined), [id]);
  useEffect(() => { cmd.getNote(id).then(setNote).catch(fail); loadCtx(); }, [id, fail, loadCtx]);

  const flush = useCallback(async () => {
    clearTimeout(timer.current);
    const body = pending.current;
    if (body == null) return;
    pending.current = null;
    try {
      await cmd.updateNote(id, { body });
      setStatus('saved');
      await Promise.all([refresh(), loadCtx()]);
    } catch (e) { setStatus('error'); fail(e); }
  }, [id, refresh, fail, loadCtx]);
  // flush unsaved edits when leaving the note
  useEffect(() => () => { if (pending.current != null) { const b = pending.current; cmd.updateNote(id, { body: b }).then(() => useStore.getState().refresh()); } }, [id]);

  const onChange = useCallback((doc: string) => {
    pending.current = doc;
    setStatus('saving');
    clearTimeout(timer.current);
    timer.current = setTimeout(flush, 450);
  }, [flush]);

  if (!note) return <div className="note-pane" />;
  const withView = (fn: (v: NonNullable<ReturnType<EditorHandle['view']>>) => void) => () => { const v = editor.current?.view(); if (v) fn(v); };
  const pickImage = withView((v) => {
    const input = document.createElement('input');
    input.type = 'file'; input.accept = 'image/*';
    input.onchange = async () => {
      const f = input.files?.[0];
      if (!f) return;
      const out = await importBrowserFile(f);
      const r = v.state.selection.main;
      const md = `![${out.item.title}](media:${out.item.id})`;
      v.dispatch({ changes: { from: r.from, to: r.to, insert: md }, selection: { anchor: r.from + md.length } });
      refresh();
    };
    input.click();
  });

  return (
    <section className="note-pane" data-testid="note-pane">
      <header className="note-head">
        <button className="ghost back" onClick={onBack} aria-label="Back to notes"><ArrowLeft size={16} /></button>
        <label className="date-pill" title="Where this note sits on the timeline">
          <CalendarDays size={14} />
          <input type="datetime-local" value={toLocalInput(note.date)} data-testid="note-date"
            onChange={(e) => { const ms = new Date(e.target.value).getTime(); if (!Number.isNaN(ms)) cmd.updateNote(id, { date: ms }).then((n) => { setNote(n); refresh(); }).catch(fail); }} />
        </label>
        <span className="spacer" />
        <span className={`save-state ${status}`} data-testid="save-state">{status === 'saved' ? 'Saved' : status === 'saving' ? 'Saving…' : 'Not saved'}</span>
        <button className={`ghost danger ${confirmDelete ? 'armed' : ''}`} data-testid="delete-note"
          onClick={() => { if (!confirmDelete) { setConfirmDelete(true); setTimeout(() => setConfirmDelete(false), 3000); } else cmd.deleteNote(id).then(onDeleted).catch(fail); }}>
          <Trash2 size={14} /> {confirmDelete ? 'Click again to delete' : ''}
        </button>
      </header>
      <div className="toolbar" role="toolbar" aria-label="Formatting">
        <button title="Bold (Ctrl/Cmd+B)" onClick={withView((v) => wrap(v, '**'))}><Bold size={15} /></button>
        <button title="Italic (Ctrl/Cmd+I)" onClick={withView((v) => wrap(v, '*'))}><Italic size={15} /></button>
        <button title="Strikethrough" onClick={withView((v) => wrap(v, '~~'))}><Strikethrough size={15} /></button>
        <button title="Inline code" onClick={withView((v) => wrap(v, '`'))}><Code size={15} /></button>
        <span className="sep" />
        <button title="Heading" data-testid="tb-heading" onClick={withView((v) => linePrefix(v, '## '))}><Heading1 size={15} /></button>
        <button title="Bullet list" onClick={withView((v) => linePrefix(v, '- '))}><List size={15} /></button>
        <button title="Task list" data-testid="tb-task" onClick={withView((v) => linePrefix(v, '- [ ] '))}><ListChecks size={15} /></button>
        <button title="Quote" onClick={withView((v) => linePrefix(v, '> '))}><Quote size={15} /></button>
        <span className="sep" />
        <button title="Link to note [[ ]]" data-testid="tb-wikilink" onClick={withView((v) => wrap(v, '[[', ']]', ''))}><Link2 size={15} /></button>
        <button title="Table" data-testid="tb-table" onClick={withView(insertTable)}><Table2 size={15} /></button>
        <button title="Code block" onClick={withView(insertCodeBlock)}><SquareCode size={15} /></button>
        <button title="Insert image" onClick={pickImage}><ImageIcon size={15} /></button>
      </div>
      <div className="note-body">
        <div className="editor-col">
          <Editor ref={editor} noteId={id} initialDoc={note.body} onChange={onChange} onOpenLink={(t) => flush().then(() => onOpenTitle(t))} onTag={onTag} />
        </div>
        <aside className="context" data-testid="context-panel">
          {ctx && (
            <>
              {ctx.tasks.length > 0 && (
                <section><h4>Tasks <small>{ctx.tasks.filter((t) => t.isDone).length}/{ctx.tasks.length}</small></h4>
                  <ul className="plain">{ctx.tasks.map((t) => <li key={t.line} className={t.isDone ? 'done' : ''}>{t.isDone ? '☑' : '☐'} {t.text}</li>)}</ul></section>
              )}
              <section><h4>Backlinks <small data-testid="backlink-count">{ctx.backlinks.length}</small></h4>
                {ctx.backlinks.length === 0 && <p className="muted">No notes link here yet. Use <code>[[{ctx.note.title}]]</code>.</p>}
                <ul className="plain">{ctx.backlinks.map((b) => (
                  <li key={b.noteId + b.line}><button className="link" onClick={() => openNote(b.noteId)} data-testid="backlink">{b.title}</button><small>{b.snippet}</small></li>
                ))}</ul></section>
              {ctx.outgoing.length > 0 && (
                <section><h4>Links</h4>
                  <ul className="plain">{ctx.outgoing.map((o, i) => (
                    <li key={i}><button className={`link ${o.noteId ? '' : 'ghost-link'}`} onClick={() => (o.noteId ? openNote(o.noteId) : onOpenTitle(o.target))}>{o.alias ?? o.target}</button>{!o.noteId && <small>not created yet</small>}</li>
                  ))}</ul></section>
              )}
              <section><h4>Related <small>semantic</small></h4>
                {ctx.related.length === 0 && <p className="muted">Nothing similar yet.</p>}
                <ul className="plain">{ctx.related.slice(0, 6).map((r) => (
                  <li key={r.id}><button className="link" onClick={() => (r.kind === 'note' ? openNote(r.ownerId) : useStore.getState().openMedia(r.ownerId))}>{r.title}</button><small>{Math.round(r.score * 100)}% · {r.kind === 'note' ? 'note' : 'recording'}</small></li>
                ))}</ul></section>
              {ctx.headings.length > 0 && (
                <section><h4>Outline</h4>
                  <ul className="plain outline">{ctx.headings.map((h, i) => (
                    <li key={i} style={{ paddingLeft: (h.level - 1) * 10 }}>
                      <button className="link" onClick={withView((v) => { const l = v.state.doc.line(Math.min(v.state.doc.lines, h.line + 1)); v.dispatch({ selection: { anchor: l.from }, scrollIntoView: true }); v.focus(); })}>{h.text}</button>
                    </li>
                  ))}</ul></section>
              )}
            </>
          )}
        </aside>
      </div>
    </section>
  );
}
