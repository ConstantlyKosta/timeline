import { forwardRef, useEffect, useImperativeHandle, useRef } from 'react';
import { EditorState, Compartment } from '@codemirror/state';
import { EditorView, drawSelection, highlightActiveLine, keymap, placeholder } from '@codemirror/view';
import { defaultKeymap, history, historyKeymap, indentWithTab } from '@codemirror/commands';
import { markdown, markdownLanguage } from '@codemirror/lang-markdown';
import { languages } from '@codemirror/language-data';
import { syntaxHighlighting } from '@codemirror/language';
import { closeBrackets } from '@codemirror/autocomplete';
import { searchKeymap, highlightSelectionMatches } from '@codemirror/search';
import { tags as t, tagHighlighter } from '@lezer/highlight';
import { useStore } from '../lib/store';
import { api } from '../lib/api';
import { importBrowserFile } from '../lib/api';
import { livePreview, previewEnv, refreshPreview } from './livePreview';
import { autocompleteExt } from './completions';
import { wrap } from './commands';

const codeHighlight = tagHighlighter([
  { tag: [t.keyword, t.modifier, t.operatorKeyword, t.controlKeyword], class: 'tok-keyword' },
  { tag: [t.string, t.special(t.string), t.regexp], class: 'tok-string' },
  { tag: [t.number, t.bool, t.null, t.atom], class: 'tok-number' },
  { tag: [t.comment, t.lineComment, t.blockComment], class: 'tok-comment' },
  { tag: [t.function(t.variableName), t.function(t.propertyName), t.macroName], class: 'tok-function' },
  { tag: [t.typeName, t.className, t.namespace], class: 'tok-type' },
  { tag: [t.propertyName, t.attributeName], class: 'tok-property' },
  { tag: [t.operator, t.punctuation, t.bracket], class: 'tok-punct' },
  { tag: [t.tagName, t.heading], class: 'tok-tag' },
]);

export interface EditorHandle { view(): EditorView | null }

interface Props {
  noteId: string;
  initialDoc: string;
  onChange: (doc: string) => void;
  onOpenLink: (title: string) => void;
  onTag?: (tag: string) => void;
}

/** CodeMirror 6 markdown editor with live preview, wiki-link/tag autocomplete, slash menu and image paste. */
export const Editor = forwardRef<EditorHandle, Props>(function Editor({ noteId, initialDoc, onChange, onOpenLink, onTag }, ref) {
  const host = useRef<HTMLDivElement>(null);
  const viewRef = useRef<EditorView | null>(null);
  const cb = useRef({ onChange, onOpenLink, onTag });
  cb.current = { onChange, onOpenLink, onTag };
  useImperativeHandle(ref, () => ({ view: () => viewRef.current }));
  const titles = useStore((s) => s.titles);
  const compartment = useRef(new Compartment());

  useEffect(() => {
    if (!host.current) return;
    const getTitles = () => useStore.getState().titles;
    const getTags = () => useStore.getState().tags;
    const insertImageFile = async (view: EditorView, file: File) => {
      try {
        const out = await importBrowserFile(file);
        const alt = out.item.title.replace(/[\[\]]/g, '');
        const r = view.state.selection.main;
        const md = `![${alt}](media:${out.item.id})`;
        view.dispatch({ changes: { from: r.from, to: r.to, insert: md }, selection: { anchor: r.from + md.length } });
        useStore.getState().refresh();
      } catch (e) { useStore.getState().fail(e); }
    };
    const pickImage = (view: EditorView) => {
      const input = document.createElement('input');
      input.type = 'file';
      input.accept = 'image/*';
      input.onchange = () => { const f = input.files?.[0]; if (f) insertImageFile(view, f); };
      input.click();
    };
    const state = EditorState.create({
      doc: initialDoc,
      extensions: [
        history(),
        drawSelection(),
        highlightActiveLine(),
        highlightSelectionMatches(),
        closeBrackets(),
        EditorView.lineWrapping,
        placeholder('Start writing…  Use [[ to link notes, #tags, and / for commands.'),
        markdown({ base: markdownLanguage, codeLanguages: languages, addKeymap: true }),
        syntaxHighlighting(codeHighlight),
        previewEnv.of({ titles: getTitles, mediaUrl: (id) => api.mediaUrl(id) }),
        livePreview,
        autocompleteExt({ titles: getTitles, tags: getTags, pickImage }),
        keymap.of([
          { key: 'Mod-b', run: (v) => (wrap(v, '**'), true) },
          { key: 'Mod-i', run: (v) => (wrap(v, '*'), true) },
          { key: 'Mod-Shift-x', run: (v) => (wrap(v, '~~'), true) },
          { key: 'Mod-e', run: (v) => (wrap(v, '`'), true) },
          ...searchKeymap, ...historyKeymap, indentWithTab, ...defaultKeymap,
        ]),
        EditorView.domEventHandlers({
          mousedown(e, view) {
            const el = e.target as HTMLElement;
            const wl = el.closest?.('.cm-wikilink-rendered') as HTMLElement | null;
            if (wl && !e.shiftKey && e.button === 0) {
              e.preventDefault();
              cb.current.onOpenLink(wl.dataset.target ?? wl.textContent ?? '');
              return true;
            }
            const tag = el.closest?.('.cm-tag') as HTMLElement | null;
            if (tag && (e.metaKey || e.ctrlKey)) { cb.current.onTag?.(tag.dataset.tag ?? ''); return true; }
            const link = el.closest?.('.cm-link, .cm-url') as HTMLElement | null;
            if (link?.dataset.href && (e.metaKey || e.ctrlKey)) {
              e.preventDefault();
              window.open(link.dataset.href, '_blank', 'noopener');
              return true;
            }
            void view;
            return false;
          },
          paste(e, view) {
            const files = [...(e.clipboardData?.files ?? [])].filter((f) => f.type.startsWith('image/'));
            if (!files.length) return false;
            e.preventDefault();
            files.forEach((f) => insertImageFile(view, f.name ? f : new File([f], `pasted-${Date.now()}.png`, { type: f.type })));
            return true;
          },
          drop(e, view) {
            const files = [...(e.dataTransfer?.files ?? [])].filter((f) => f.type.startsWith('image/'));
            if (!files.length) return false;
            e.preventDefault();
            const pos = view.posAtCoords({ x: e.clientX, y: e.clientY });
            if (pos != null) view.dispatch({ selection: { anchor: pos } });
            files.forEach((f) => insertImageFile(view, f));
            return true;
          },
        }),
        compartment.current.of([]),
        EditorView.updateListener.of((u) => { if (u.docChanged) cb.current.onChange(u.state.doc.toString()); }),
      ],
    });
    const view = new EditorView({ state, parent: host.current });
    viewRef.current = view;
    if (!initialDoc.trim()) view.focus(); // brand-new note: start typing immediately
    return () => { view.destroy(); viewRef.current = null; };
    // the editor is re-created per note (key=noteId in the parent); initialDoc is intentionally read once
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [noteId]);

  // ghost/resolved styling of [[links]] follows the live title list
  useEffect(() => { viewRef.current?.dispatch({ effects: refreshPreview.of(null) }); }, [titles]);

  return <div className="cm-host" ref={host} data-testid="editor" />;
});
