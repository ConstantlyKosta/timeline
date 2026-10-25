import { autocompletion, type Completion, type CompletionContext, type CompletionResult } from '@codemirror/autocomplete';
import { format } from 'date-fns';
import type { EditorView } from '@codemirror/view';
import type { NoteRef, TagCount } from '../lib/types';
import { insertBlock, insertCodeBlock, insertDivider, insertTable, linePrefix, TABLE_TEMPLATE } from './commands';

export interface CompletionEnv {
  titles: () => NoteRef[];
  tags: () => TagCount[];
  pickImage: (view: EditorView) => void;
}

/** `[[` → note titles (fuzzy matched by CodeMirror); typing an unknown title offers to create it on open. */
function wikiSource(env: CompletionEnv) {
  return (ctx: CompletionContext): CompletionResult | null => {
    const m = ctx.matchBefore(/\[\[[^\[\]\n|]*/);
    if (!m) return null;
    const query = m.text.slice(2);
    const after = ctx.state.sliceDoc(ctx.pos, ctx.pos + 2);
    const close = after === ']]' ? '' : ']]';
    const q = query.trim().toLowerCase();
    const rank = (title: string) => {
      const t = title.toLowerCase();
      if (!q) return 3;
      if (t === q) return 0;
      if (t.startsWith(q)) return 1;
      if (t.includes(q)) return 2;
      let i = 0; for (const ch of t) if (ch === q[i]) i++; // subsequence ("lbt" → Lisbon Trip)
      return i === q.length ? 4 : -1;
    };
    const titles = env.titles()
      .map((t) => ({ t, r: rank(t.title) }))
      .filter((x) => x.r >= 0)
      .sort((a, b) => a.r - b.r || a.t.title.localeCompare(b.t.title))
      .map((x) => x.t);
    const options: Completion[] = titles.map((t) => ({
      label: t.title, type: 'text', detail: 'note',
      apply: (view, _c, from, to) => {
        const insert = t.title + close;
        view.dispatch({ changes: { from, to, insert }, selection: { anchor: from + insert.length + (close ? 0 : 2) } });
      },
    }));
    const exact = env.titles().some((t) => t.title.toLowerCase() === q);
    if (q && !exact) {
      options.push({
        label: query.trim(), displayLabel: `Create “${query.trim()}”`, type: 'keyword', detail: 'new note',
        apply: (view, _c, from, to) => {
          const insert = query.trim() + close;
          view.dispatch({ changes: { from, to, insert }, selection: { anchor: from + insert.length + (close ? 0 : 2) } });
        },
      });
    }
    return { from: m.from + 2, options, filter: false };
  };
}

function tagSource(env: CompletionEnv) {
  return (ctx: CompletionContext): CompletionResult | null => {
    const m = ctx.matchBefore(/(?:^|[\s(])#[\p{L}\p{N}_/-]*/u);
    if (!m) return null;
    const hashAt = m.text.indexOf('#');
    const from = m.from + hashAt;
    const typed = m.text.slice(hashAt + 1);
    if (!ctx.explicit && typed === '' && ctx.pos === from + 1 && ctx.state.doc.lineAt(from).text.startsWith('# ')) return null;
    const options: Completion[] = env.tags().map((t) => ({ label: '#' + t.tag, detail: `${t.count}`, type: 'variable' }));
    return { from, options, validFor: /^#[\p{L}\p{N}_/-]*$/u };
  };
}

interface Slash { label: string; detail: string; keywords: string; run: (v: EditorView) => void }

export function slashCommands(env: CompletionEnv): Slash[] {
  return [
    { label: 'Heading 1', detail: '#', keywords: 'h1 title', run: (v) => linePrefix(v, '# ') },
    { label: 'Heading 2', detail: '##', keywords: 'h2', run: (v) => linePrefix(v, '## ') },
    { label: 'Heading 3', detail: '###', keywords: 'h3', run: (v) => linePrefix(v, '### ') },
    { label: 'Bulleted list', detail: '-', keywords: 'ul list bullet', run: (v) => linePrefix(v, '- ') },
    { label: 'Numbered list', detail: '1.', keywords: 'ol list number', run: (v) => linePrefix(v, '1. ') },
    { label: 'Task list', detail: '- [ ]', keywords: 'todo checkbox task', run: (v) => linePrefix(v, '- [ ] ') },
    { label: 'Quote', detail: '>', keywords: 'blockquote', run: (v) => linePrefix(v, '> ') },
    { label: 'Code block', detail: '```', keywords: 'code fence snippet', run: insertCodeBlock },
    { label: 'Table', detail: 'GFM', keywords: 'table grid', run: insertTable },
    { label: 'Divider', detail: '---', keywords: 'hr rule line', run: insertDivider },
    { label: 'Link to note', detail: '[[ ]]', keywords: 'wiki link note', run: (v) => insertBlock(v, '[[]]', 2) },
    { label: 'Today’s date', detail: format(new Date(), 'yyyy-MM-dd'), keywords: 'date today', run: (v) => insertBlock(v, format(new Date(), 'EEEE d MMMM yyyy')) },
    { label: 'Image…', detail: 'file', keywords: 'image photo picture', run: env.pickImage },
  ];
}

function slashSource(env: CompletionEnv) {
  const cmds = slashCommands(env);
  return (ctx: CompletionContext): CompletionResult | null => {
    const line = ctx.state.doc.lineAt(ctx.pos);
    const before = line.text.slice(0, ctx.pos - line.from);
    const m = before.match(/^\s*\/([\w ]*)$/);
    if (!m) return null;
    const from = line.from + before.indexOf('/');
    const options: Completion[] = cmds.map((c) => ({
      label: '/' + c.label.toLowerCase(), displayLabel: c.label, detail: c.detail, type: 'function', info: undefined,
      apply: (view: EditorView, _c: Completion, f: number, t: number) => {
        view.dispatch({ changes: { from: f, to: t, insert: '' } });
        c.run(view);
      },
      // searchable alias keywords piggyback on `section` free text via label matching below
    }));
    // CodeMirror filters on `label`; extend with keywords through a lightweight pre-filter
    const q = m[1].toLowerCase().trim();
    const filtered = q ? options.filter((o, i) => (o.label + ' ' + cmds[i].keywords).includes(q) || o.label.includes(q)) : options;
    return { from, options: filtered, filter: false, validFor: /^\/[\w ]*$/ };
  };
}

export { TABLE_TEMPLATE };

export const autocompleteExt = (env: CompletionEnv) =>
  autocompletion({
    override: [wikiSource(env), tagSource(env), slashSource(env)],
    activateOnTyping: true,
    icons: false,
    defaultKeymap: true,
    maxRenderedOptions: 12,
  });
