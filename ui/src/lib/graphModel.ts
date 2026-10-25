// Pure logic of the mind map: settings, filtering, colouring, sizing, local graph, queries.
// No DOM, no WebGL; everything here is unit-tested. (Feature set specified by the README of the
// Obsidian "Extended Graph" plugin; this is an independent implementation, no code was copied.)
import type { GEdgeRaw, GNodeRaw, GraphRaw } from './types';

export const SHAPES = ['circle', 'square', 'diamond', 'triangle', 'hexagon', 'star'] as const;
export type Shape = (typeof SHAPES)[number];
export type ColorBy = 'kind' | 'tag' | 'folder' | 'property' | 'none';
export type ShapeBy = 'kind' | 'tag' | 'folder' | 'none';
export type SizeBy = 'degree' | 'in' | 'out' | 'words' | 'recent' | 'pagerank' | 'uniform';
export type LabelMode = 'auto' | 'always' | 'never';
export type DKind = 'note' | 'ghost' | 'attachment' | 'tag' | 'folder';

export interface Group { id: string; name: string; query: string; color: string }
export interface Forces { center: number; repel: number; link: number; distance: number; collide: number }

export interface GraphSettings {
  // filters
  query: string;
  tagsInclude: string[];
  tagsExclude: string[];
  folders: string[];
  propKey: string;
  propValue: string;
  hiddenLinkKinds: string[];
  showOrphans: boolean;
  showUnresolved: boolean;
  showAttachments: boolean;
  showTagNodes: boolean;
  showFolderNodes: boolean;
  // local graph
  local: boolean;
  depth: number;
  focusId: string | null;
  // display
  colorBy: ColorBy;
  colorProp: string;
  shapeBy: ShapeBy;
  sizeBy: SizeBy;
  sizeScale: number;
  showImages: boolean;
  showArrows: boolean;
  curved: boolean;
  labels: LabelMode;
  edgeThickness: number;
  linkColors: Record<string, string>;
  forces: Forces;
  groups: Group[];
  pinned: string[];
}

export const DEFAULT_FORCES: Forces = { center: 0.1, repel: 110, link: 0.3, distance: 45, collide: 1 };

export const DEFAULT_SETTINGS: GraphSettings = {
  query: '', tagsInclude: [], tagsExclude: [], folders: [], propKey: '', propValue: '', hiddenLinkKinds: [],
  showOrphans: true, showUnresolved: true, showAttachments: true, showTagNodes: false, showFolderNodes: false,
  local: false, depth: 2, focusId: null,
  colorBy: 'kind', colorProp: '', shapeBy: 'kind', sizeBy: 'degree', sizeScale: 1,
  showImages: true, showArrows: true, curved: false, labels: 'auto', edgeThickness: 1,
  linkColors: {}, forces: DEFAULT_FORCES, groups: [], pinned: [],
};

/** Fill in anything missing in a stored/untrusted settings object. */
export function mergeSettings(partial: unknown): GraphSettings {
  const p = (partial && typeof partial === 'object' ? partial : {}) as Partial<GraphSettings>;
  const s: GraphSettings = { ...DEFAULT_SETTINGS, ...p, forces: { ...DEFAULT_FORCES, ...(p.forces ?? {}) } };
  for (const k of ['tagsInclude', 'tagsExclude', 'folders', 'hiddenLinkKinds', 'pinned'] as const) if (!Array.isArray(s[k])) s[k] = [];
  if (!Array.isArray(s.groups)) s.groups = [];
  if (!s.linkColors || typeof s.linkColors !== 'object') s.linkColors = {};
  s.depth = Math.min(8, Math.max(1, Math.round(Number(s.depth) || 2)));
  return s;
}

/** Built-in starting points shown next to the user's saved views. */
export const PRESETS: { name: string; settings: Partial<GraphSettings> }[] = [
  { name: 'Overview', settings: {} },
  { name: 'Tags map', settings: { colorBy: 'tag', shapeBy: 'tag', showTagNodes: true, showAttachments: false } },
  { name: 'Folders', settings: { colorBy: 'folder', shapeBy: 'kind', showFolderNodes: true, showAttachments: false } },
  { name: 'Connections only', settings: { showOrphans: false, showUnresolved: false, showAttachments: false, sizeBy: 'pagerank' } },
  { name: 'Gallery', settings: { showImages: true, showAttachments: true, sizeBy: 'uniform', sizeScale: 1.6 } },
];

// ---------------------------------------------------------------------------------------------
// Query language (search box and groups): `tag:x` `#x` `folder:x` `prop:key=value` `kind:ghost` `-term`

export interface QueryToken { neg: boolean; field: 'text' | 'tag' | 'folder' | 'prop' | 'kind'; key?: string; value: string }

export function parseQuery(q: string): QueryToken[] {
  const out: QueryToken[] = [];
  for (const raw of q.trim().split(/\s+/).filter(Boolean)) {
    let neg = false;
    let w = raw;
    if (w.startsWith('-') && w.length > 1) { neg = true; w = w.slice(1); }
    const lower = w.toLowerCase();
    if (lower.startsWith('tag:') || lower.startsWith('#')) out.push({ neg, field: 'tag', value: lower.replace(/^tag:|^#/, '') });
    else if (lower.startsWith('folder:') || lower.startsWith('path:')) out.push({ neg, field: 'folder', value: lower.replace(/^folder:|^path:/, '') });
    else if (lower.startsWith('kind:')) out.push({ neg, field: 'kind', value: lower.slice(5) });
    else if (lower.startsWith('prop:')) {
      const [k, ...v] = w.slice(5).split('=');
      out.push({ neg, field: 'prop', key: k.toLowerCase(), value: v.join('=').toLowerCase() });
    } else out.push({ neg, field: 'text', value: lower });
  }
  return out.filter((t) => t.value !== '' || t.field === 'prop');
}

export function matchNode(n: GNodeRaw, toks: QueryToken[]): boolean {
  for (const t of toks) {
    let ok: boolean;
    switch (t.field) {
      case 'tag': ok = n.tags.some((x) => x === t.value || x.startsWith(t.value + '/')); break;
      case 'folder': ok = n.folder.toLowerCase().startsWith(t.value); break;
      case 'kind': ok = n.kind === t.value; break;
      case 'prop': {
        const e = Object.entries(n.props).find(([k]) => k.toLowerCase() === t.key);
        ok = !!e && (t.value === '' || e[1].toLowerCase().includes(t.value));
        break;
      }
      default: ok = n.title.toLowerCase().includes(t.value) || n.preview.toLowerCase().includes(t.value);
    }
    if (ok === t.neg) return false;
  }
  return true;
}

// ---------------------------------------------------------------------------------------------
// Display model

export interface DNode {
  id: string; label: string; kind: DKind; color: string; shape: Shape; size: number; image: string | null;
  raw: GNodeRaw | null; degree: number; matched: boolean; members?: number; group?: string; pinned: boolean;
}
export interface DEdge { id: string; from: string; to: string; kind: string; color: string; directed: boolean }
export interface Legend {
  colors: { label: string; color: string; count: number }[];
  shapes: { label: string; shape: Shape }[];
  links: { kind: string; label: string; color: string; count: number }[];
  groups: { label: string; color: string; count: number }[];
}
export interface Display {
  nodes: DNode[]; edges: DEdge[]; legend: Legend; matchCount: number; signature: string;
  totals: { nodes: number; edges: number }; hasQuery: boolean;
}

export const PALETTE = ['#5b8def', '#e8743b', '#38b27a', '#c65bd3', '#e5b93c', '#2cb5c9', '#e0566b', '#8d6ae0', '#7fb233', '#d17aa1', '#4f9aa8', '#b88a5a'];
const NEUTRAL = '#8b95a7';
export const KIND_COLORS: Record<DKind, string> = { note: '#6c8cff', ghost: '#9aa3b2', attachment: '#d9a441', tag: '#3fae83', folder: '#a07bd8' };
const KIND_SHAPES: Record<DKind, Shape> = { note: 'circle', ghost: 'diamond', attachment: 'square', tag: 'triangle', folder: 'hexagon' };

export function linkKindLabel(kind: string): string {
  if (kind === 'link') return 'Link in text';
  if (kind === 'attachment') return 'Embeds';
  if (kind === 'tagnode') return 'Tag membership';
  if (kind === 'foldernode') return 'Folder membership';
  if (kind.startsWith('prop:')) return `Property: ${kind.slice(5)}`;
  return kind;
}
export function hash(s: string): number { let h = 2166136261; for (let i = 0; i < s.length; i++) { h ^= s.charCodeAt(i); h = Math.imul(h, 16777619); } return h >>> 0; }
export function defaultLinkColor(kind: string, theme: 'dark' | 'light'): string {
  if (kind === 'link') return theme === 'dark' ? '#7d879a' : '#8892a6';
  if (kind === 'attachment') return '#d9a441';
  if (kind === 'tagnode') return '#3fae83';
  if (kind === 'foldernode') return '#a07bd8';
  return PALETTE[hash(kind) % PALETTE.length];
}
const isDirected = (kind: string) => kind === 'link' || kind === 'attachment' || kind.startsWith('prop:');

/** Neighbourhood of `start` within `depth` hops (links followed in both directions). */
export function neighborhood(edges: { from: string; to: string }[], start: string, depth: number): Set<string> {
  const adj = new Map<string, string[]>();
  for (const e of edges) {
    (adj.get(e.from) ?? adj.set(e.from, []).get(e.from)!).push(e.to);
    (adj.get(e.to) ?? adj.set(e.to, []).get(e.to)!).push(e.from);
  }
  const seen = new Set([start]);
  let frontier = [start];
  for (let d = 0; d < depth && frontier.length; d++) {
    const next: string[] = [];
    for (const id of frontier) for (const o of adj.get(id) ?? []) if (!seen.has(o)) { seen.add(o); next.push(o); }
    frontier = next;
  }
  return seen;
}

export function pageRank(ids: string[], edges: { from: string; to: string }[], iterations = 25, d = 0.85): Map<string, number> {
  const n = ids.length;
  const idx = new Map(ids.map((id, i) => [id, i]));
  let r = new Array<number>(n).fill(1 / Math.max(1, n));
  const outs = new Array<number>(n).fill(0);
  const es = edges.filter((e) => idx.has(e.from) && idx.has(e.to)).map((e) => [idx.get(e.from)!, idx.get(e.to)!] as const);
  for (const [a] of es) outs[a]++;
  for (let it = 0; it < iterations; it++) {
    const nr = new Array<number>(n).fill(0);
    let dangling = 0;
    for (let i = 0; i < n; i++) if (outs[i] === 0) dangling += r[i];
    for (const [a, b] of es) nr[b] += (d * r[a]) / outs[a];
    for (let i = 0; i < n; i++) nr[i] += (1 - d) / n + (d * dangling) / n;
    r = nr;
  }
  return new Map(ids.map((id, i) => [id, r[i]]));
}

export function buildDisplay(data: GraphRaw, s: GraphSettings, theme: 'dark' | 'light' = 'dark'): Display {
  const pinned = new Set(s.pinned);
  const tagsIn = new Set(s.tagsInclude), tagsEx = new Set(s.tagsExclude);
  const folders = s.folders.map((f) => f.toLowerCase());
  const propKey = s.propKey.toLowerCase(), propVal = s.propValue.toLowerCase();
  const hiddenKinds = new Set(s.hiddenLinkKinds);

  // 1. node filters (notes decide; ghosts/attachments follow their links)
  const keep = new Map<string, GNodeRaw>();
  for (const n of data.nodes) {
    if (n.kind === 'ghost' && !s.showUnresolved) continue;
    if (n.kind === 'attachment' && !s.showAttachments) continue;
    if (n.kind === 'note') {
      if (tagsIn.size && !n.tags.some((t) => tagsIn.has(t))) continue;
      if (tagsEx.size && n.tags.some((t) => tagsEx.has(t))) continue;
      if (folders.length && !folders.some((f) => n.folder.toLowerCase() === f || n.folder.toLowerCase().startsWith(f + '/'))) continue;
      if (propKey) {
        const e = Object.entries(n.props).find(([k]) => k.toLowerCase() === propKey);
        if (!e || (propVal && !e[1].toLowerCase().includes(propVal))) continue;
      }
    }
    keep.set(n.id, n);
  }
  // 2. edges
  let edges: GEdgeRaw[] = data.edges.filter((e) => keep.has(e.from) && keep.has(e.to) && !hiddenKinds.has(e.kind));

  // 3. tag / folder hub nodes
  const hubs = new Map<string, { kind: 'tag' | 'folder'; label: string; members: number }>();
  const hubEdges: GEdgeRaw[] = [];
  if (s.showTagNodes) {
    for (const n of keep.values()) for (const t of n.tags) {
      const id = `tag:${t}`;
      const h = hubs.get(id) ?? hubs.set(id, { kind: 'tag', label: `#${t}`, members: 0 }).get(id)!;
      h.members++;
      hubEdges.push({ from: n.id, to: id, kind: 'tagnode' });
    }
  }
  if (s.showFolderNodes) {
    const ensure = (path: string) => {
      const id = `folder:${path}`;
      if (!hubs.has(id)) {
        hubs.set(id, { kind: 'folder', label: path.split('/').pop() || path, members: 0 });
        const parent = path.includes('/') ? path.slice(0, path.lastIndexOf('/')) : '';
        if (parent) hubEdges.push({ from: ensure(parent), to: id, kind: 'foldernode' });
      }
      return id;
    };
    for (const n of keep.values()) if (n.kind === 'note' && n.folder) {
      const id = ensure(n.folder);
      hubs.get(id)!.members++;
      hubEdges.push({ from: id, to: n.id, kind: 'foldernode' });
    }
  }
  edges = [...edges, ...hubEdges.filter((e) => !hiddenKinds.has(e.kind))];

  // 4. orphans (ghosts/attachments/hubs only live through their links)
  const touched = new Set<string>();
  for (const e of edges) { touched.add(e.from); touched.add(e.to); }
  const ids = new Set<string>();
  for (const n of keep.values()) if (touched.has(n.id) || (n.kind === 'note' && s.showOrphans)) ids.add(n.id);
  for (const id of hubs.keys()) if (touched.has(id)) ids.add(id);
  edges = edges.filter((e) => ids.has(e.from) && ids.has(e.to));

  // 5. local graph
  if (s.local && s.focusId && ids.has(s.focusId)) {
    const near = neighborhood(edges, s.focusId, s.depth);
    for (const id of [...ids]) if (!near.has(id)) ids.delete(id);
    edges = edges.filter((e) => ids.has(e.from) && ids.has(e.to));
  }

  // 6. metrics on what is actually shown
  const idList = [...ids].sort();
  const deg = new Map<string, number>(), din = new Map<string, number>(), dout = new Map<string, number>();
  const nb = new Map<string, Set<string>>();
  for (const e of edges) {
    dout.set(e.from, (dout.get(e.from) ?? 0) + 1);
    din.set(e.to, (din.get(e.to) ?? 0) + 1);
    (nb.get(e.from) ?? nb.set(e.from, new Set()).get(e.from)!).add(e.to);
    (nb.get(e.to) ?? nb.set(e.to, new Set()).get(e.to)!).add(e.from);
  }
  for (const id of idList) deg.set(id, nb.get(id)?.size ?? 0);
  const pr = s.sizeBy === 'pagerank' ? pageRank(idList, edges) : null;
  const dates = [...keep.values()].map((n) => n.date).filter(Boolean);
  const minDate = dates.length ? Math.min(...dates) : 0;
  const metric = (id: string): number => {
    const n = keep.get(id);
    switch (s.sizeBy) {
      case 'degree': return deg.get(id) ?? 0;
      case 'in': return din.get(id) ?? 0;
      case 'out': return dout.get(id) ?? 0;
      case 'words': return n?.words ?? 0;
      case 'recent': return n && n.date ? n.date - minDate : 0;
      case 'pagerank': return pr?.get(id) ?? 0;
      default: return 0;
    }
  };

  // 7. categories (colour + shape)
  const catOf = (n: GNodeRaw, by: 'tag' | 'folder' | 'property', tagFreq: Map<string, number>): string => {
    if (by === 'tag') return n.tags.length ? [...n.tags].sort((a, b) => (tagFreq.get(b)! - tagFreq.get(a)!) || a.localeCompare(b))[0] : '';
    if (by === 'folder') return n.folder;
    const e = Object.entries(n.props).find(([k]) => k.toLowerCase() === s.colorProp.toLowerCase());
    return e ? e[1] : '';
  };
  const tagFreq = new Map<string, number>();
  for (const id of idList) for (const t of keep.get(id)?.tags ?? []) tagFreq.set(t, (tagFreq.get(t) ?? 0) + 1);
  const catByNode = new Map<string, string>();
  const catCount = new Map<string, number>();
  const shapeCat = s.shapeBy === 'tag' || s.shapeBy === 'folder' ? s.shapeBy : null;
  const colorCatBy = s.colorBy === 'tag' || s.colorBy === 'folder' || s.colorBy === 'property' ? s.colorBy : null;
  const colorCat = new Map<string, string>(), shapeCatMap = new Map<string, string>();
  for (const id of idList) {
    const n = keep.get(id);
    if (!n || n.kind !== 'note') continue;
    if (colorCatBy) { const c = catOf(n, colorCatBy, tagFreq); colorCat.set(id, c); catCount.set(c, (catCount.get(c) ?? 0) + 1); }
    if (shapeCat) shapeCatMap.set(id, catOf(n, shapeCat, tagFreq));
  }
  const ranked = [...catCount.entries()].filter(([c]) => c !== '').sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
  const catColor = new Map(ranked.map(([c], i) => [c, PALETTE[i % PALETTE.length]]));
  const shapeRank = new Map<string, number>();
  { const cnt = new Map<string, number>(); for (const c of shapeCatMap.values()) if (c) cnt.set(c, (cnt.get(c) ?? 0) + 1);
    [...cnt.entries()].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0])).forEach(([c], i) => shapeRank.set(c, i)); }

  const groupToks = s.groups.map((g) => ({ g, toks: parseQuery(g.query) }));
  const searchToks = parseQuery(s.query);
  const hasQuery = searchToks.length > 0;
  const groupCount = new Map<string, number>();

  // size normalisation
  const vals = idList.filter((id) => keep.has(id)).map(metric);
  const lo = vals.length ? Math.min(...vals) : 0, hi = vals.length ? Math.max(...vals) : 0;
  const sizeOf = (id: string): number => {
    if (s.sizeBy === 'uniform' || hi === lo) return 8 * s.sizeScale;
    const t = (metric(id) - lo) / (hi - lo);
    return (5 + 14 * Math.sqrt(t)) * s.sizeScale;
  };

  const nodes: DNode[] = [];
  let matchCount = 0;
  for (const id of idList) {
    const raw = keep.get(id);
    if (raw) {
      const kind = raw.kind as DKind;
      let color = KIND_COLORS[kind];
      let shape: Shape = KIND_SHAPES[kind];
      if (kind === 'note') {
        if (colorCatBy) { const c = colorCat.get(id) ?? ''; color = c ? catColor.get(c) ?? NEUTRAL : NEUTRAL; }
        else if (s.colorBy === 'none') color = NEUTRAL;
        if (shapeCat) { const c = shapeCatMap.get(id) ?? ''; shape = c ? SHAPES[1 + ((shapeRank.get(c) ?? 0) % (SHAPES.length - 1))] : 'circle'; }
        else if (s.shapeBy === 'none') shape = 'circle';
      } else if (s.colorBy === 'none') color = NEUTRAL;
      if (s.shapeBy === 'none') shape = 'circle';
      let group: string | undefined;
      for (const { g, toks } of groupToks) if (toks.length && matchNode(raw, toks)) { color = g.color; group = g.id; groupCount.set(g.id, (groupCount.get(g.id) ?? 0) + 1); break; }
      const matched = !hasQuery || matchNode(raw, searchToks);
      if (hasQuery && matched) matchCount++;
      const sz = sizeOf(id) * (kind === 'note' ? 1 : 0.8);
      nodes.push({ id, label: raw.title, kind, color, shape, size: sz, image: s.showImages ? raw.image : null, raw, degree: deg.get(id) ?? 0, matched, group, pinned: pinned.has(id) });
    } else {
      const h = hubs.get(id)!;
      let color = KIND_COLORS[h.kind];
      if (h.kind === 'tag' && s.colorBy === 'tag') color = catColor.get(h.label.slice(1)) ?? color;
      if (h.kind === 'folder' && s.colorBy === 'folder') color = catColor.get(id.slice(7)) ?? color;
      const matched = !hasQuery || searchToks.every((t) => t.field === 'text' ? (h.label.toLowerCase().includes(t.value)) !== t.neg : true);
      nodes.push({ id, label: h.label, kind: h.kind, color, shape: s.shapeBy === 'none' ? 'circle' : KIND_SHAPES[h.kind], size: Math.min(22, 6 + 2 * Math.sqrt(h.members)) * s.sizeScale, image: null, raw: null, degree: deg.get(id) ?? 0, matched, members: h.members, pinned: pinned.has(id) });
    }
  }

  const linkCount = new Map<string, number>();
  const dedges: DEdge[] = edges.map((e, i) => {
    linkCount.set(e.kind, (linkCount.get(e.kind) ?? 0) + 1);
    return { id: `e${i}`, from: e.from, to: e.to, kind: e.kind, color: s.linkColors[e.kind] ?? defaultLinkColor(e.kind, theme), directed: isDirected(e.kind) };
  });

  const legend: Legend = {
    colors: s.colorBy === 'kind'
      ? (['note', 'ghost', 'attachment', 'tag', 'folder'] as DKind[]).map((k) => ({ k, c: nodes.filter((n) => n.kind === k && !n.group).length })).filter((x) => x.c > 0).map((x) => ({ label: kindLabel(x.k), color: KIND_COLORS[x.k], count: x.c }))
      : s.colorBy === 'none' ? [] : ranked.slice(0, 14).map(([c, count]) => ({ label: c || '(none)', color: catColor.get(c) ?? NEUTRAL, count })),
    shapes: [
      ...(shapeCat ? [...shapeRank.entries()].sort((x, y) => x[1] - y[1]).slice(0, 8).map(([c, i]) => ({ label: c, shape: SHAPES[1 + (i % (SHAPES.length - 1))] })) : []),
      ...[...new Map(nodes.filter((n) => (n.shape !== 'circle' || n.kind === 'note') && !(shapeCat && n.kind === 'note')).map((n) => [`${n.shape}:${n.kind}`, n])).values()]
        .map((n) => ({ label: kindLabel(n.kind), shape: n.shape })),
    ].filter((x, i, a) => a.findIndex((y) => y.label === x.label && y.shape === x.shape) === i).slice(0, 12),
    links: [...linkCount.entries()].sort().map(([kind, count]) => ({ kind, label: linkKindLabel(kind), color: s.linkColors[kind] ?? defaultLinkColor(kind, theme), count })),
    groups: s.groups.map((g) => ({ label: g.name || g.query, color: g.color, count: groupCount.get(g.id) ?? 0 })),
  };
  const signature = `${idList.join('|')}#${dedges.map((e) => `${e.from}>${e.to}`).join('|')}`;
  return { nodes, edges: dedges, legend, matchCount, signature, totals: { nodes: data.nodes.length, edges: data.edges.length }, hasQuery };
}

export function kindLabel(k: DKind): string {
  return ({ note: 'Note', ghost: 'Not created yet', attachment: 'Attachment', tag: 'Tag', folder: 'Folder' } as const)[k];
}

/** Every tag / folder / property key in the data, for the filter pickers. */
export function facets(data: GraphRaw) {
  const tags = new Map<string, number>(), folders = new Map<string, number>();
  for (const n of data.nodes) {
    if (n.kind !== 'note') continue;
    for (const t of n.tags) tags.set(t, (tags.get(t) ?? 0) + 1);
    if (n.folder) { const parts = n.folder.split('/'); for (let i = 1; i <= parts.length; i++) { const p = parts.slice(0, i).join('/'); folders.set(p, (folders.get(p) ?? 0) + 1); } }
  }
  const sort = (m: Map<string, number>) => [...m.entries()].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
  const kinds = [...new Set(data.edges.map((e) => e.kind))].sort();
  return { tags: sort(tags), folders: sort(folders), linkKinds: kinds, propertyKeys: data.propertyKeys };
}

/** Blend `hex` toward `bg` by `t` (0 = bg, 1 = hex); used to fade non-highlighted items. */
export function fade(hex: string, bg: string, t: number): string {
  const p = (h: string) => { const x = h.replace('#', ''); const f = x.length === 3 ? x.split('').map((c) => c + c).join('') : x; return [0, 2, 4].map((i) => parseInt(f.slice(i, i + 2), 16)); };
  const a = p(hex), b = p(bg);
  return '#' + a.map((v, i) => Math.round(b[i] + (v - b[i]) * t).toString(16).padStart(2, '0')).join('');
}
