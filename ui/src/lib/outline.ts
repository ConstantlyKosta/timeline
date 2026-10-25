// Turns one note's Markdown into the tree the Markmap renderer draws: headings nest by level, bullets and
// numbered / task items nest by indentation. Every node remembers its 0-based source line, so a click in the
// map can jump the editor there. (markmap-lib's own parser has no line mapping, so this small parser is ours;
// markmap-view does the drawing.)

export interface OutlineNode {
  content: string; // HTML, already escaped
  children: OutlineNode[];
  payload: { line: number; kind: 'title' | 'heading' | 'item' };
}

const esc = (s: string) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');

/** A small inline-Markdown → safe-HTML pass for node labels. */
export function inlineHtml(raw: string): string {
  let s = esc(raw.trim());
  s = s.replace(/!\[([^\]]*)\]\([^)]*\)/g, (_m, a) => a || 'image');
  s = s.replace(/\[\[([^\]|]*)\|([^\]]*)\]\]/g, '$2').replace(/\[\[([^\]]*)\]\]/g, '$1');
  s = s.replace(/\[([^\]]+)\]\([^)]*\)/g, '$1');
  s = s.replace(/`([^`]+)`/g, '<code>$1</code>');
  s = s.replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>').replace(/__([^_]+)__/g, '<strong>$1</strong>');
  s = s.replace(/(^|[\s(])\*([^*\s][^*]*)\*/g, '$1<em>$2</em>').replace(/(^|[\s(])_([^_\s][^_]*)_(?=$|[\s).,;:!?])/g, '$1<em>$2</em>');
  s = s.replace(/~~([^~]+)~~/g, '<del>$1</del>');
  return s;
}

const HEADING = /^ {0,3}(#{1,6})\s+(.*?)\s*#*\s*$/;
const ITEM = /^([ \t]*)([-*+]|\d+[.)])\s+(\[[ xX]\]\s+)?(.*)$/;
const indentOf = (ws: string) => ws.replace(/\t/g, '    ').length;

interface Frame { kind: 'root' | 'h' | 'l'; level: number; node: OutlineNode }

export function parseOutline(markdown: string, title: string): OutlineNode {
  const root: OutlineNode = { content: inlineHtml(title || 'Untitled'), children: [], payload: { line: 0, kind: 'title' } };
  const stack: Frame[] = [{ kind: 'root', level: 0, node: root }];
  const lines = markdown.replace(/\r\n?/g, '\n').split('\n');
  let i = 0;
  if (lines[0]?.trim() === '---') { // YAML front-matter
    const end = lines.findIndex((l, k) => k > 0 && (l.trim() === '---' || l.trim() === '...'));
    if (end > 0) i = end + 1;
  }
  let fence: string | null = null;
  for (; i < lines.length; i++) {
    const line = lines[i];
    const f = /^\s*(```|~~~)/.exec(line);
    if (f) { fence = fence === null ? f[1] : fence === f[1] ? null : fence; continue; }
    if (fence) continue;
    const h = HEADING.exec(line);
    if (h) {
      const level = h[1].length;
      while (stack.length > 1 && (stack[stack.length - 1].kind === 'l' || stack[stack.length - 1].level >= level)) stack.pop();
      const node: OutlineNode = { content: inlineHtml(h[2]) || '&nbsp;', children: [], payload: { line: i, kind: 'heading' } };
      stack[stack.length - 1].node.children.push(node);
      stack.push({ kind: 'h', level, node });
      continue;
    }
    const m = ITEM.exec(line);
    if (m) {
      const ind = indentOf(m[1]);
      while (stack.length > 1 && stack[stack.length - 1].kind === 'l' && stack[stack.length - 1].level >= ind) stack.pop();
      const mark = m[3] ? (/x/i.test(m[3]) ? '☑ ' : '☐ ') : '';
      const node: OutlineNode = { content: mark + (inlineHtml(m[4]) || '&nbsp;'), children: [], payload: { line: i, kind: 'item' } };
      stack[stack.length - 1].node.children.push(node);
      stack.push({ kind: 'l', level: ind, node });
    }
  }
  // a note that is just "# Title" + content: the single top heading becomes the root
  if (root.children.length === 1 && root.children[0].payload.kind === 'heading') return root.children[0];
  return root;
}

export const outlineIsEmpty = (root: OutlineNode) => root.children.length === 0;
