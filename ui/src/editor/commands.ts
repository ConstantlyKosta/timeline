import type { EditorView } from '@codemirror/view';

/** Wraps the selection (or inserts `placeholder`) in `before`/`after`; unwraps when already wrapped. */
export function wrap(view: EditorView, before: string, after = before, placeholder = 'text') {
  const { state } = view;
  const r = state.selection.main;
  const sel = state.sliceDoc(r.from, r.to);
  const pre = state.sliceDoc(Math.max(0, r.from - before.length), r.from);
  const post = state.sliceDoc(r.to, r.to + after.length);
  if (pre === before && post === after && r.from !== r.to) {
    view.dispatch({ changes: [{ from: r.from - before.length, to: r.from }, { from: r.to, to: r.to + after.length }], selection: { anchor: r.from - before.length, head: r.to - before.length } });
  } else {
    const text = sel || placeholder;
    view.dispatch({
      changes: { from: r.from, to: r.to, insert: before + text + after },
      selection: { anchor: r.from + before.length, head: r.from + before.length + text.length },
    });
  }
  view.focus();
}

/** Toggles a line prefix (`- `, `> `, `# `, `- [ ] `) on every selected line. */
export function linePrefix(view: EditorView, prefix: string, replaceRe = /^(#{1,6} |[-*+] \[[ xX]\] |[-*+] |\d+\. |> )/) {
  const { state } = view;
  const r = state.selection.main;
  const first = state.doc.lineAt(r.from).number, last = state.doc.lineAt(r.to).number;
  const changes = [];
  const allHave = Array.from({ length: last - first + 1 }, (_, i) => state.doc.line(first + i)).every((l) => l.text.startsWith(prefix));
  for (let n = first; n <= last; n++) {
    const l = state.doc.line(n);
    const existing = l.text.match(replaceRe)?.[0] ?? '';
    changes.push({ from: l.from, to: l.from + existing.length, insert: allHave ? '' : prefix });
  }
  view.dispatch({ changes });
  view.focus();
}

/** Inserts a block at the start of the current line (or the current position if the line is empty); cursor lands at `cursor` within the text. */
export function insertBlock(view: EditorView, text: string, cursor = text.length, selectLen = 0) {
  const { state } = view;
  const r = state.selection.main;
  const line = state.doc.lineAt(r.from);
  const empty = line.text.trim() === '';
  const from = empty ? line.from : line.to;
  const to = empty ? line.to : line.to;
  const prefix = empty ? '' : '\n';
  view.dispatch({
    changes: { from, to, insert: prefix + text + '\n' },
    selection: { anchor: from + prefix.length + cursor, head: from + prefix.length + cursor + selectLen },
  });
  view.focus();
}

export const TABLE_TEMPLATE = '| Column 1 | Column 2 | Column 3 |\n| --- | --- | --- |\n|  |  |  |\n|  |  |  |';
export const insertTable = (view: EditorView) => insertBlock(view, TABLE_TEMPLATE, 2, 8);
export const insertCodeBlock = (view: EditorView) => insertBlock(view, '```ts\n\n```', 6);
export const insertDivider = (view: EditorView) => insertBlock(view, '---\n');
