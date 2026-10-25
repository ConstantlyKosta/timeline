import { describe, expect, it } from 'vitest';
import { inlineHtml, outlineIsEmpty, parseOutline, type OutlineNode } from './outline';

const shape = (n: OutlineNode): unknown => (n.children.length ? [n.content, n.children.map(shape)] : n.content);

describe('outline', () => {
  it('nests headings by level and bullets by indentation, with source lines', () => {
    const md = '# Plan\n\nintro text\n\n## A\n- one\n  - nested\n- two\n\n## B\n1. first\n- [ ] todo\n- [x] done\n';
    const root = parseOutline(md, 'ignored');
    expect(root.content).toBe('Plan'); // single top heading is hoisted
    expect(shape(root)).toEqual(['Plan', [['A', [['one', ['nested']], 'two']], ['B', ['first', '☐ todo', '☑ done']]]]);
    expect(root.children[0].children[0].children[0].payload.line).toBe(6); // "  - nested"
    expect(root.children[0].payload).toEqual({ line: 4, kind: 'heading' });
  });
  it('uses the note title as root when there are several top-level parts', () => {
    const root = parseOutline('## A\n- x\n## B\n', 'My note');
    expect(root.content).toBe('My note');
    expect(root.payload.kind).toBe('title');
    expect(shape(root)).toEqual(['My note', [['A', ['x']], 'B']]);
  });
  it('skips front-matter and fenced code (a # inside code is not a heading)', () => {
    const root = parseOutline('---\ntags: [a]\n---\n## Real\n```sh\n# comment\n- not an item\n```\n- item\n', 't');
    expect(shape(root)).toEqual(['Real', ['item']]);
  });
  it('handles skipped levels, tabs, and heading closing hashes', () => {
    const root = parseOutline('# T\n### Deep ##\n- a\n\t- b\n## Back\n', 't');
    expect(shape(root)).toEqual(['T', [['Deep', [['a', ['b']]]], 'Back']]);
  });
  it('reports an empty outline for plain prose', () => {
    expect(outlineIsEmpty(parseOutline('just some words\nmore words', 'Idea'))).toBe(true);
  });
  it('escapes HTML and renders inline marks and wiki-links as text', () => {
    expect(inlineHtml('<b>x</b> & **bold** *it* `c` ~~s~~ [[Page|alias]] [[Other]] [t](http://x) ![img](media:1)')).toBe(
      '&lt;b&gt;x&lt;/b&gt; &amp; <strong>bold</strong> <em>it</em> <code>c</code> <del>s</del> alias Other t img');
  });
});
