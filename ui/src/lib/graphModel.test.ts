import { describe, expect, it } from 'vitest';
import { buildDisplay, DEFAULT_SETTINGS, facets, fade, matchNode, mergeSettings, neighborhood, pageRank, parseQuery, type GraphSettings } from './graphModel';
import type { GEdgeRaw, GNodeRaw, GraphRaw } from './types';

const n = (id: string, over: Partial<GNodeRaw> = {}): GNodeRaw => ({ id, title: id.toUpperCase(), kind: 'note', tags: [], folder: '', props: {}, degree: 0, inDegree: 0, outDegree: 0, words: 10, date: 1000, image: null, preview: '', ...over });
const e = (from: string, to: string, kind = 'link'): GEdgeRaw => ({ from, to, kind });
const data: GraphRaw = {
  nodes: [
    n('a', { tags: ['work', 'big'], folder: 'Projects', props: { status: 'active' }, words: 300, date: 5000, image: 'm1' }),
    n('b', { tags: ['work'], folder: 'Projects/Sub', props: { status: 'done' } }),
    n('c', { tags: ['life'], folder: 'Daily' }),
    n('d'), // orphan
    n('ghost:x', { kind: 'ghost', title: 'Nowhere' }),
    n('att:1', { kind: 'attachment', title: 'pic.png', image: '1' }),
    n('f'), n('g'),
  ],
  edges: [e('a', 'b'), e('b', 'c'), e('a', 'ghost:x'), e('a', 'att:1', 'attachment'), e('c', 'a', 'prop:related'), e('f', 'g'), e('c', 'f')],
  propertyKeys: ['status'],
};
const S = (o: Partial<GraphSettings> = {}): GraphSettings => ({ ...DEFAULT_SETTINGS, ...o });
const ids = (d: ReturnType<typeof buildDisplay>) => d.nodes.map((x) => x.id).sort();

describe('queries', () => {
  it('parses fields and negation', () => {
    expect(parseQuery('tag:work -folder:Daily #big prop:status=act hello')).toEqual([
      { neg: false, field: 'tag', value: 'work' }, { neg: true, field: 'folder', value: 'daily' }, { neg: false, field: 'tag', value: 'big' },
      { neg: false, field: 'prop', key: 'status', value: 'act' }, { neg: false, field: 'text', value: 'hello' }]);
  });
  it('matches nodes', () => {
    const a = data.nodes[0];
    expect(matchNode(a, parseQuery('tag:work folder:projects'))).toBe(true);
    expect(matchNode(a, parseQuery('tag:work -tag:big'))).toBe(false);
    expect(matchNode(a, parseQuery('prop:status=act'))).toBe(true);
    expect(matchNode(a, parseQuery('prop:missing'))).toBe(false);
    expect(matchNode(data.nodes[4], parseQuery('kind:ghost nowh'))).toBe(true);
    expect(matchNode(n('x', { tags: ['a/b'] }), parseQuery('tag:a'))).toBe(true);
  });
});

describe('filters', () => {
  it('keeps everything by default and counts', () => {
    const d = buildDisplay(data, S());
    expect(d.nodes).toHaveLength(8);
    expect(d.edges).toHaveLength(7);
  });
  it('hides orphans, unresolved and attachments', () => {
    const d = buildDisplay(data, S({ showOrphans: false, showUnresolved: false, showAttachments: false }));
    expect(ids(d)).toEqual(['a', 'b', 'c', 'f', 'g']);
    expect(d.edges.every((x) => !x.kind.startsWith('attach'))).toBe(true);
  });
  it('filters by tag include/exclude, folder, property', () => {
    expect(ids(buildDisplay(data, S({ tagsInclude: ['life'] })))).toEqual(['c']);
    expect(ids(buildDisplay(data, S({ tagsExclude: ['work'], showOrphans: false, showUnresolved: false, showAttachments: false })))).toEqual(['c', 'f', 'g']);
    expect(ids(buildDisplay(data, S({ folders: ['Projects'], showOrphans: false, showAttachments: false, showUnresolved: false })))).toEqual(['a', 'b']);
    expect(ids(buildDisplay(data, S({ propKey: 'status', propValue: 'done', showUnresolved: false, showAttachments: false })))).toEqual(['b']);
  });
  it('hides a link type', () => {
    const d = buildDisplay(data, S({ hiddenLinkKinds: ['prop:related'] }));
    expect(d.edges.find((x) => x.kind === 'prop:related')).toBeUndefined();
    expect(d.legend.links.map((l) => l.kind)).not.toContain('prop:related');
  });
});

describe('local graph', () => {
  it('limits to N hops around the focus node', () => {
    expect(ids(buildDisplay(data, S({ local: true, focusId: 'a', depth: 1 })))).toEqual(['a', 'att:1', 'b', 'c', 'ghost:x']);
    expect(ids(buildDisplay(data, S({ local: true, focusId: 'a', depth: 2 })))).toEqual(['a', 'att:1', 'b', 'c', 'f', 'ghost:x']);
    expect(ids(buildDisplay(data, S({ local: true, focusId: 'a', depth: 3 })))).toContain('g');
  });
  it('neighborhood ignores direction', () => {
    expect([...neighborhood([e('x', 'y'), e('z', 'y')], 'x', 2)].sort()).toEqual(['x', 'y', 'z']);
  });
  it('ignores an unknown focus node', () => {
    expect(buildDisplay(data, S({ local: true, focusId: 'nope' })).nodes).toHaveLength(8);
  });
});

describe('hubs, colour, shape, size', () => {
  it('adds tag and folder hub nodes with membership edges', () => {
    const d = buildDisplay(data, S({ showTagNodes: true, showFolderNodes: true }));
    expect(d.nodes.filter((x) => x.kind === 'tag').map((x) => x.label).sort()).toEqual(['#big', '#life', '#work']);
    expect(d.nodes.find((x) => x.id === 'tag:work')!.members).toBe(2);
    expect(d.nodes.filter((x) => x.kind === 'folder').map((x) => x.id).sort()).toEqual(['folder:Daily', 'folder:Projects', 'folder:Projects/Sub']);
    expect(d.edges.filter((x) => x.kind === 'foldernode')).toHaveLength(4); // Projects>Sub, Projects>a, Sub>b, Daily>c
    expect(d.edges.filter((x) => x.directed).length).toBeGreaterThan(0);
    expect(d.edges.filter((x) => x.kind === 'tagnode').every((x) => !x.directed)).toBe(true);
  });
  it('colours by tag using the most frequent tag, same colour per tag', () => {
    const d = buildDisplay(data, S({ colorBy: 'tag' }));
    const col = (id: string) => d.nodes.find((x) => x.id === id)!.color;
    expect(col('a')).toBe(col('b')); // both "work" is most frequent
    expect(col('a')).not.toBe(col('c'));
    expect(d.legend.colors[0]).toMatchObject({ label: 'work', count: 2 });
  });
  it('colours by property and folder', () => {
    const d = buildDisplay(data, S({ colorBy: 'property', colorProp: 'status' }));
    expect(d.nodes.find((x) => x.id === 'a')!.color).not.toBe(d.nodes.find((x) => x.id === 'b')!.color);
    const f = buildDisplay(data, S({ colorBy: 'folder' }));
    expect(f.legend.colors.map((c) => c.label)).toContain('Projects');
  });
  it('assigns shapes by kind and by tag', () => {
    const d = buildDisplay(data, S());
    expect(d.nodes.find((x) => x.id === 'a')!.shape).toBe('circle');
    expect(d.nodes.find((x) => x.id === 'ghost:x')!.shape).toBe('diamond');
    expect(d.nodes.find((x) => x.id === 'att:1')!.shape).toBe('square');
    const t = buildDisplay(data, S({ shapeBy: 'tag' }));
    expect(t.nodes.find((x) => x.id === 'a')!.shape).toBe(t.nodes.find((x) => x.id === 'b')!.shape);
    expect(t.nodes.find((x) => x.id === 'a')!.shape).not.toBe('circle');
    expect(t.nodes.find((x) => x.id === 'd')!.shape).toBe('circle');
    expect(buildDisplay(data, S({ shapeBy: 'none' })).nodes.every((x) => x.shape === 'circle')).toBe(true);
  });
  it('sizes by degree, words, uniform and scale', () => {
    const d = buildDisplay(data, S());
    const sz = (id: string) => d.nodes.find((x) => x.id === id)!.size;
    expect(sz('a')).toBeGreaterThan(sz('d'));
    expect(sz('a')).toBeLessThanOrEqual(19 + 1e-9);
    const w = buildDisplay(data, S({ sizeBy: 'words' }));
    expect(w.nodes.find((x) => x.id === 'a')!.size).toBeGreaterThan(w.nodes.find((x) => x.id === 'b')!.size);
    const u = buildDisplay(data, S({ sizeBy: 'uniform', sizeScale: 2 }));
    expect(new Set(u.nodes.filter((x) => x.kind === 'note').map((x) => x.size))).toEqual(new Set([16]));
  });
  it('shows images only when enabled', () => {
    expect(buildDisplay(data, S()).nodes.find((x) => x.id === 'a')!.image).toBe('m1');
    expect(buildDisplay(data, S({ showImages: false })).nodes.find((x) => x.id === 'a')!.image).toBeNull();
  });
  it('groups override colour and are counted', () => {
    const d = buildDisplay(data, S({ groups: [{ id: 'g1', name: 'Work', query: 'tag:work', color: '#ff0000' }] }));
    expect(d.nodes.find((x) => x.id === 'a')!.color).toBe('#ff0000');
    expect(d.legend.groups).toEqual([{ label: 'Work', color: '#ff0000', count: 2 }]);
  });
  it('link colours: defaults per type, overridable', () => {
    const d = buildDisplay(data, S({ linkColors: { link: '#123456' } }));
    expect(d.edges.find((x) => x.kind === 'link')!.color).toBe('#123456');
    expect(d.edges.find((x) => x.kind === 'attachment')!.color).not.toBe('#123456');
  });
});

describe('search and misc', () => {
  it('marks matches', () => {
    const d = buildDisplay(data, S({ query: 'tag:work' }));
    expect(d.matchCount).toBe(2);
    expect(d.nodes.filter((x) => x.matched).map((x) => x.id).sort()).toEqual(['a', 'b']);
    expect(buildDisplay(data, S()).nodes.every((x) => x.matched)).toBe(true);
  });
  it('signature changes with topology only', () => {
    const a = buildDisplay(data, S()).signature;
    expect(buildDisplay(data, S({ colorBy: 'tag', sizeBy: 'words' })).signature).toBe(a);
    expect(buildDisplay(data, S({ showOrphans: false })).signature).not.toBe(a);
  });
  it('pagerank favours well linked nodes and sums to 1', () => {
    const pr = pageRank(['a', 'b', 'c'], [e('a', 'c'), e('b', 'c')]);
    expect(pr.get('c')!).toBeGreaterThan(pr.get('a')!);
    expect([...pr.values()].reduce((x, y) => x + y, 0)).toBeCloseTo(1, 6);
  });
  it('facets list tags, nested folders and link kinds', () => {
    const f = facets(data);
    expect(f.tags[0]).toEqual(['work', 2]);
    expect(f.folders.map((x) => x[0])).toContain('Projects/Sub');
    expect(f.linkKinds).toEqual(['attachment', 'link', 'prop:related']);
  });
  it('merges stored settings safely', () => {
    const s = mergeSettings({ colorBy: 'tag', forces: { repel: 10 }, depth: 99, pinned: 'bad' });
    expect(s.colorBy).toBe('tag');
    expect(s.forces.repel).toBe(10);
    expect(s.forces.link).toBe(DEFAULT_SETTINGS.forces.link);
    expect(s.depth).toBe(8);
    expect(s.pinned).toEqual([]);
    expect(mergeSettings(null)).toEqual(DEFAULT_SETTINGS);
  });
  it('fades colours toward the background', () => {
    expect(fade('#ffffff', '#000000', 0.5)).toBe('#808080');
    expect(fade('#ff0000', '#ff0000', 0.1)).toBe('#ff0000');
  });
});
