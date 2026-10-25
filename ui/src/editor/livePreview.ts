// Obsidian-style live preview for CodeMirror 6: the Markdown stays plain text in the document; markup is
// hidden/rendered for everything the cursor is not touching. Built purely from the @lezer/markdown syntax tree
// (GFM tables, task lists, strikethrough) plus two regex layers for [[wiki links]] and #tags.
import { ensureSyntaxTree, syntaxTree } from '@codemirror/language';
import { EditorState, Facet, Range, StateEffect, StateField } from '@codemirror/state';
import { Decoration, DecorationSet, EditorView, WidgetType } from '@codemirror/view';
import type { NoteRef } from '../lib/types';

export interface PreviewEnv {
  titles: () => NoteRef[];
  mediaUrl: (id: string) => string;
}
export const previewEnv = Facet.define<PreviewEnv, PreviewEnv>({ combine: (v) => v[0] ?? { titles: () => [], mediaUrl: (i) => i } });
export const refreshPreview = StateEffect.define<null>();
const focusEffect = StateEffect.define<boolean>();

export const fold = (s: string) => s.normalize('NFD').replace(/[\u0300-\u036f]/g, '').toLowerCase().trim();

// ------------------------------------------------------------------- widgets
class BulletWidget extends WidgetType {
  toDOM() { const s = document.createElement('span'); s.className = 'cm-bullet'; s.textContent = '•'; return s; }
  eq() { return true; }
}
class HrWidget extends WidgetType {
  toDOM() { const h = document.createElement('div'); h.className = 'cm-hr'; return h; }
  eq() { return true; }
}
class CheckboxWidget extends WidgetType {
  constructor(readonly checked: boolean) { super(); }
  eq(o: CheckboxWidget) { return o.checked === this.checked; }
  toDOM(view: EditorView) {
    const box = document.createElement('input');
    box.type = 'checkbox';
    box.checked = this.checked;
    box.className = 'cm-task-box';
    box.setAttribute('data-testid', 'task-box');
    box.addEventListener('mousedown', (e) => {
      e.preventDefault();
      const pos = view.posAtDOM(box);
      const cur = view.state.sliceDoc(pos, pos + 3);
      if (!/^\[[ xX]\]$/.test(cur)) return;
      view.dispatch({ changes: { from: pos + 1, to: pos + 2, insert: cur[1] === ' ' ? 'x' : ' ' } });
    });
    return box;
  }
  ignoreEvent() { return true; }
}
class ImageWidget extends WidgetType {
  constructor(readonly src: string, readonly alt: string) { super(); }
  eq(o: ImageWidget) { return o.src === this.src && o.alt === this.alt; }
  toDOM(view: EditorView) {
    const img = document.createElement('img');
    img.src = this.src;
    img.alt = this.alt;
    img.className = 'cm-image';
    img.addEventListener('mousedown', (e) => {
      e.preventDefault();
      const pos = view.posAtDOM(img);
      view.dispatch({ selection: { anchor: pos } });
      view.focus();
    });
    return img;
  }
  ignoreEvent() { return true; }
}

function escapeHtml(s: string) {
  return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}
export function inlineHtml(raw: string): string {
  let s = escapeHtml(raw);
  s = s.replace(/`([^`]+)`/g, '<code>$1</code>');
  s = s.replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>');
  s = s.replace(/(^|[^*])\*([^*\s][^*]*)\*/g, '$1<em>$2</em>');
  s = s.replace(/~~([^~]+)~~/g, '<del>$1</del>');
  s = s.replace(/\[\[([^\]|\n]+)(?:\|([^\]\n]+))?\]\]/g, (_m, t, a) => `<span class="cm-wikilink">${a ?? t}</span>`);
  return s;
}
export function splitRow(line: string): string[] {
  let t = line.trim();
  if (t.startsWith('|')) t = t.slice(1);
  if (t.endsWith('|') && !t.endsWith('\\|')) t = t.slice(0, -1);
  const cells: string[] = [];
  let cur = '';
  for (let i = 0; i < t.length; i++) {
    if (t[i] === '\\' && t[i + 1] === '|') { cur += '|'; i++; }
    else if (t[i] === '|') { cells.push(cur.trim()); cur = ''; }
    else cur += t[i];
  }
  cells.push(cur.trim());
  return cells;
}
class TableWidget extends WidgetType {
  constructor(readonly src: string) { super(); }
  eq(o: TableWidget) { return o.src === this.src; }
  toDOM(view: EditorView) {
    const lines = this.src.split('\n').filter((l) => l.trim());
    const wrap = document.createElement('div');
    wrap.className = 'cm-table-wrap';
    wrap.setAttribute('data-testid', 'table-widget');
    const table = document.createElement('table');
    const head = splitRow(lines[0] ?? '');
    const aligns = splitRow(lines[1] ?? '').map((c) => (c.startsWith(':') && c.endsWith(':') ? 'center' : c.endsWith(':') ? 'right' : ''));
    const thead = table.createTHead().insertRow();
    head.forEach((c, i) => { const th = document.createElement('th'); th.innerHTML = inlineHtml(c); if (aligns[i]) th.style.textAlign = aligns[i]; thead.appendChild(th); });
    const tbody = table.createTBody();
    for (const l of lines.slice(2)) {
      const tr = tbody.insertRow();
      splitRow(l).forEach((c, i) => { const td = tr.insertCell(); td.innerHTML = inlineHtml(c); if (aligns[i]) td.style.textAlign = aligns[i]; });
    }
    wrap.appendChild(table);
    wrap.addEventListener('mousedown', (e) => {
      e.preventDefault();
      const pos = view.posAtDOM(wrap);
      view.dispatch({ selection: { anchor: pos } });
      view.focus();
    });
    return wrap;
  }
  ignoreEvent() { return true; }
}

class LangWidget extends WidgetType {
  constructor(readonly lang: string) { super(); }
  eq(o: LangWidget) { return o.lang === this.lang; }
  toDOM() { const e = document.createElement('span'); e.className = 'cm-lang-badge'; e.textContent = this.lang || 'code'; return e; }
  ignoreEvent() { return false; }
}

// ---------------------------------------------------------------- decorations
const hide = Decoration.replace({});
const lineDeco = (cls: string) => Decoration.line({ class: cls });
const mark = (cls: string, attrs?: Record<string, string>) => Decoration.mark({ class: cls, attributes: attrs });

const focusField = StateField.define<boolean>({
  create: () => false,
  update(v, tr) { for (const e of tr.effects) if (e.is(focusEffect)) return e.value; return v; },
});

function build(state: EditorState): DecorationSet {
  const env = state.facet(previewEnv);
  const focused = state.field(focusField);
  const doc = state.doc;
  const out: Range<Decoration>[] = [];
  const ranges = state.selection.ranges;
  const touches = (from: number, to: number) => focused && ranges.some((r) => r.from <= to && r.to >= from);
  const lineTouched = (pos: number) => { const l = doc.lineAt(pos); return touches(l.from, l.to); };
  const codeRanges: [number, number][] = [];
  const addLines = (from: number, to: number, cls: string) => {
    for (let n = doc.lineAt(from).number; n <= doc.lineAt(to).number; n++) out.push(lineDeco(cls).range(doc.line(n).from));
  };

  const tree = ensureSyntaxTree(state, doc.length, 80) ?? syntaxTree(state);
  tree.iterate({
    enter(ref) {
      const { name, from, to } = ref;
      let m: RegExpMatchArray | null;
      if ((m = name.match(/^ATXHeading([1-6])$/))) {
        out.push(lineDeco(`cm-h cm-h${m[1]}`).range(doc.lineAt(from).from));
      } else if (name === 'SetextHeading1' || name === 'SetextHeading2') {
        addLines(from, to, `cm-h cm-h${name.slice(-1)}`);
      } else if (name === 'HeaderMark' && ref.node.parent?.name.startsWith('ATX')) {
        if (!lineTouched(from)) out.push(hide.range(from, Math.min(to + 1, doc.lineAt(from).to)));
      } else if (name === 'StrongEmphasis' || name === 'Emphasis' || name === 'Strikethrough') {
        out.push(mark(name === 'StrongEmphasis' ? 'cm-strong' : name === 'Emphasis' ? 'cm-em' : 'cm-strike').range(from, to));
      } else if (name === 'EmphasisMark' || name === 'StrikethroughMark') {
        const p = ref.node.parent!;
        if (!touches(p.from, p.to)) out.push(hide.range(from, to));
      } else if (name === 'InlineCode') {
        out.push(mark('cm-inline-code').range(from, to));
        codeRanges.push([from, to]);
      } else if (name === 'CodeMark' && ref.node.parent?.name === 'InlineCode') {
        const p = ref.node.parent;
        if (!touches(p.from, p.to)) out.push(hide.range(from, to));
      } else if (name === 'FencedCode' || name === 'CodeBlock') {
        codeRanges.push([from, to]);
        const first = doc.lineAt(from).number, last = doc.lineAt(to).number;
        if (name === 'FencedCode' && last > first && !touches(from, to)) {
          // collapse the fences: language badge on top, nothing on the bottom edge
          const fl = doc.line(first), ll = doc.line(last);
          const info = doc.sliceString(fl.from, fl.to).replace(/^[\s`~]+/, '').trim();
          out.push(Decoration.replace({ widget: new LangWidget(info) }).range(fl.from, fl.to));
          if (/^\s*(`{3,}|~{3,})\s*$/.test(doc.sliceString(ll.from, ll.to))) {
            out.push(hide.range(ll.from, ll.to));
            out.push(lineDeco('cm-code-collapsed').range(ll.from));
          }
        }
        for (let n = first; n <= last; n++) {
          const edge = name === 'FencedCode' ? (n === first ? ' cm-code-first' : n === last ? ' cm-code-last' : '') : '';
          out.push(lineDeco('cm-codeblock' + edge).range(doc.line(n).from));
        }
      } else if (name === 'Blockquote') {
        addLines(from, to, 'cm-quote');
      } else if (name === 'QuoteMark') {
        if (!lineTouched(from)) out.push(hide.range(from, Math.min(to + 1, doc.lineAt(from).to)));
      } else if (name === 'HorizontalRule') {
        if (!lineTouched(from)) out.push(Decoration.replace({ widget: new HrWidget() }).range(from, to));
        else out.push(lineDeco('cm-hr-src').range(doc.lineAt(from).from));
      } else if (name === 'ListMark') {
        const item = ref.node.parent;
        const isTask = !!item?.getChild('Task');
        const bullet = item?.parent?.name === 'BulletList';
        out.push(lineDeco('cm-li').range(doc.lineAt(from).from));
        if (!lineTouched(from)) {
          if (isTask) out.push(hide.range(from, Math.min(to + 1, doc.lineAt(from).to)));
          else if (bullet) out.push(Decoration.replace({ widget: new BulletWidget() }).range(from, to));
        }
      } else if (name === 'TaskMarker') {
        const done = /\[[xX]\]/.test(doc.sliceString(from, to));
        if (done) { const p = ref.node.parent!; out.push(mark('cm-task-done').range(to, Math.max(to, p.to))); }
        if (!lineTouched(from)) out.push(Decoration.replace({ widget: new CheckboxWidget(done) }).range(from, to));
      } else if (name === 'Link') {
        const marks = ref.node.getChildren('LinkMark');
        const url = ref.node.getChild('URL');
        if (marks.length >= 2) {
          const href = url ? doc.sliceString(url.from, url.to) : '';
          out.push(mark('cm-link', { 'data-href': href }).range(marks[0].to, marks[1].from));
          if (!touches(from, to)) {
            out.push(hide.range(marks[0].from, marks[0].to));
            out.push(hide.range(marks[1].from, to));
          } else out.push(mark('cm-link-src').range(from, to));
        }
        return false;
      } else if (name === 'URL' && ref.node.parent?.name !== 'Link' && ref.node.parent?.name !== 'Image') {
        out.push(mark('cm-url', { 'data-href': doc.sliceString(from, to) }).range(from, to));
      } else if (name === 'Image') {
        const src = doc.sliceString(from, to);
        const im = src.match(/^!\[([^\]]*)\]\(([^)\s]+)(?:\s+"[^"]*")?\)$/);
        if (im && !touches(from, to)) {
          const url = im[2].startsWith('media:') ? env.mediaUrl(im[2].slice(6)) : im[2];
          out.push(Decoration.replace({ widget: new ImageWidget(url, im[1]) }).range(from, to));
        } else out.push(mark('cm-link-src').range(from, to));
        return false;
      } else if (name === 'Table') {
        codeRanges.push([from, to]);
        if (!touches(from, to)) {
          out.push(Decoration.replace({ widget: new TableWidget(doc.sliceString(from, to)), block: true }).range(from, to));
        } else addLines(from, to, 'cm-table-src');
        return false;
      }
      return undefined;
    },
  });

  // wiki links + tags (regex layer, skipping code/table source)
  const inCode = (a: number, b: number) => codeRanges.some(([f, t]) => a >= f && b <= t);
  const known = new Set(env.titles().map((t) => fold(t.title)));
  const text = doc.toString();
  const wl = /\[\[([^\[\]\n]+?)\]\]/g;
  let m: RegExpExecArray | null;
  while ((m = wl.exec(text))) {
    const from = m.index, to = from + m[0].length;
    if (inCode(from, to)) continue;
    const inner = m[1];
    const pipe = inner.indexOf('|');
    const targetRaw = (pipe >= 0 ? inner.slice(0, pipe) : inner).split('#')[0];
    const resolved = known.has(fold(targetRaw));
    const cls = 'cm-wikilink' + (resolved ? '' : ' cm-wikilink-ghost');
    const attrs = { 'data-target': targetRaw.trim() };
    if (touches(from, to)) {
      out.push(mark(cls + ' cm-wikilink-raw', attrs).range(from, to));
    } else {
      out.push(hide.range(from, from + 2));
      const labelStart = pipe >= 0 ? from + 2 + pipe + 1 : from + 2;
      if (pipe >= 0) out.push(hide.range(from + 2, labelStart));
      out.push(mark(cls + ' cm-wikilink-rendered', attrs).range(labelStart, to - 2));
      out.push(hide.range(to - 2, to));
    }
  }
  const tg = /(^|[\s(])#([\p{L}\p{N}_][\p{L}\p{N}_/-]*)/gu;
  while ((m = tg.exec(text))) {
    const from = m.index + m[1].length, to = from + 1 + m[2].length;
    if (inCode(from, to)) continue;
    if (/^\d+$/.test(m[2])) continue;
    out.push(mark('cm-tag', { 'data-tag': m[2] }).range(from, to));
  }
  return Decoration.set(out, true);
}

const previewField = StateField.define<DecorationSet>({
  create: (s) => build(s),
  update(deco, tr) {
    if (tr.docChanged || tr.selection || tr.effects.some((e) => e.is(focusEffect) || e.is(refreshPreview))) return build(tr.state);
    return deco;
  },
  provide: (f) => EditorView.decorations.from(f),
});

export const livePreview = [
  focusField,
  previewField,
  EditorView.focusChangeEffect.of((_s, focusing) => focusEffect.of(focusing)),
];
