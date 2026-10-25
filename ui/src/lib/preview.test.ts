import { describe, expect, it } from 'vitest';
import { makeMapping, MS_PER_DAY } from './timeline';
import { previewLayout, previewText, startTier, tierSpecs, type PreviewLayout } from './preview';
import type { TimelineEntry } from './types';

const T0 = Date.UTC(2026, 0, 1);
let seed = 7;
const rnd = () => { seed = (seed * 1664525 + 1013904223) >>> 0; return seed / 2 ** 32; };
const entry = (i: number, dayOffset: number, over: Partial<TimelineEntry> = {}): TimelineEntry => ({
  id: `e${String(i).padStart(5, '0')}`, kind: 'note', start: T0 + dayOffset * MS_PER_DAY, duration: 0, title: `Note ${i}`,
  snippet: 'A snippet of the note body', tags: [], preview: 'First line of text\nSecond line of text\nThird line here', image: null, folder: '', ...over,
});
const rects = (l: PreviewLayout) => [
  ...l.cards.map((c) => ({ id: c.id, ...c.frame })), ...l.dots.map((c) => ({ id: c.id, ...c.frame })), ...l.clusters.map((c) => ({ id: c.id, ...c.frame })),
];
const accounted = (l: PreviewLayout) => l.cards.length + l.dots.length + l.clusters.reduce((s, c) => s + c.count, 0);
const intersect = (a: ReturnType<typeof rects>[number], b: ReturnType<typeof rects>[number]) =>
  a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height;

describe('tier sizes scale with zoom', () => {
  it('grows with points per day and is clamped', () => {
    const lo = tierSpecs(12), mid = tierSpecs(160), hi = tierSpecs(400);
    expect(lo.rich.width).toBeLessThan(mid.rich.width);
    expect(hi.rich.width).toBeLessThanOrEqual(270);
    expect(lo.title.width).toBeGreaterThanOrEqual(88);
    expect(lo.rich.thumb).toBe(false);
    expect(hi.rich.thumb).toBe(true);
  });
});

describe('previewLayout', () => {
  it('gives sparse entries the richest card with preview text', () => {
    const es = [entry(1, 0), entry(2, 20), entry(3, 40)];
    const l = previewLayout(es, makeMapping(T0, 160), 460);
    expect(l.cards.map((c) => c.tier)).toEqual(['rich', 'rich', 'rich']);
    expect(l.cards[0].text).toContain('First line');
    expect(l.dots.length + l.clusters.length).toBe(0);
  });

  it('entries without body start as title chips; photos and note images get thumbnails when wide enough', () => {
    const bare = entry(1, 0, { preview: '', snippet: '', image: null });
    expect(startTier(bare)).toBe(2);
    const es = [entry(2, 0, { kind: 'photo', preview: '', snippet: '' }), entry(3, 30, { image: 'media1' })];
    const l = previewLayout(es, makeMapping(T0, 400), 460);
    expect(l.cards[0].thumb).toBe('e00002');
    expect(l.cards[1].thumb).toBe('media1');
    const narrow = previewLayout(es, makeMapping(T0, 40), 460);
    expect(narrow.cards.every((c) => c.thumb === null)).toBe(true);
  });

  it('degrades a dense burst: nothing overflows the height, richest cards demoted first, dots merge into counts', () => {
    const es = Array.from({ length: 60 }, (_, i) => entry(i, 10 + i * 0.05));
    const l = previewLayout(es, makeMapping(T0, 36), 400);
    expect(l.contentHeight).toBeLessThanOrEqual(400);
    expect(accounted(l)).toBe(60);
    expect(l.clusters.length).toBeGreaterThan(0);
    expect(l.tierCounts.rich).toBeLessThan(60);
    // a sparse neighbour far away keeps its rich card
    const withFar = previewLayout([...es, entry(999, 200)], makeMapping(T0, 36), 400);
    expect(withFar.cards.find((c) => c.id === 'e00999')?.tier).toBe('rich');
  });

  it('more vertical room allows richer cards', () => {
    const es = Array.from({ length: 6 }, (_, i) => entry(i, 10 + i * 0.2));
    const small = previewLayout(es, makeMapping(T0, 160), 240);
    const big = previewLayout(es, makeMapping(T0, 160), 900);
    expect(big.tierCounts.rich).toBeGreaterThan(small.tierCounts.rich);
  });

  it('zooming in never reduces the number of rich cards for the same data', () => {
    const es = Array.from({ length: 80 }, (_, i) => entry(i, rnd() * 60));
    const counts = [20, 36, 76, 160, 300].map((ppd) => previewLayout(es, makeMapping(T0, ppd), 480).tierCounts.rich);
    for (let i = 1; i < counts.length; i++) expect(counts[i]).toBeGreaterThanOrEqual(counts[i - 1]);
  });

  it('never overlaps and accounts for every entry exactly once (random data, several zooms and heights)', () => {
    for (const [n, span, ppd, h] of [[200, 90, 36, 420], [500, 30, 160, 500], [300, 400, 12, 300], [80, 5, 300, 250]] as const) {
      const es = Array.from({ length: n }, (_, i) => entry(i, rnd() * span, { kind: (['note', 'recording', 'photo', 'video'] as const)[i % 4], preview: i % 3 ? 'text\nmore' : '' }));
      const l = previewLayout(es, makeMapping(T0, ppd), h);
      expect(accounted(l)).toBe(n);
      const rs = rects(l);
      const ids = new Set<string>([...l.cards.map((c) => c.id), ...l.dots.map((d) => d.id), ...l.clusters.flatMap((c) => c.entryIds)]);
      expect(ids.size).toBe(n);
      rs.sort((a, b) => a.x - b.x);
      for (let i = 0; i < rs.length; i++) for (let j = i + 1; j < rs.length && rs[j].x < rs[i].x + rs[i].width; j++) expect(intersect(rs[i], rs[j]), `${rs[i].id} x ${rs[j].id}`).toBe(false);
    }
  });

  it('handles thousands of notes quickly', () => {
    const es = Array.from({ length: 5000 }, (_, i) => entry(i, rnd() * 700));
    const t = performance.now();
    const l = previewLayout(es, makeMapping(T0, 36), 450);
    const ms = performance.now() - t;
    expect(accounted(l)).toBe(5000);
    expect(ms).toBeLessThan(500);
  });

  it('is empty-safe', () => {
    const l = previewLayout([], makeMapping(T0, 36), 400);
    expect(l.cards.length + l.dots.length + l.clusters.length).toBe(0);
  });
});

describe('previewText', () => {
  it('fits lines and ellipsizes', () => {
    const e = entry(1, 0, { preview: 'short line\n' + 'long words '.repeat(30) });
    const t = previewText(e, 3, 30);
    expect(t.startsWith('short line')).toBe(true);
    expect(t.endsWith('…')).toBe(true);
    expect(previewText(entry(1, 0, { preview: '', snippet: 'fallback snippet' }), 2, 30)).toBe('fallback snippet');
  });
});
