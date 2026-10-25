// Adaptive preview cards for the Month and Week timeline levels.
//
// Every entry gets the richest card that (a) its zoom level allows and (b) fits in the vertical space together with its
// neighbours. Cards that collide horizontally form a "crowd" that is stacked into lanes; if the crowd needs more lanes than
// the viewport has height for, its richest cards are demoted one step (rich → medium → title → dot) and the layout is
// re-solved until every crowd fits. Dots that touch merge into a count badge. This is TS-only (no Rust twin): it is pure,
// deterministic, and covered by preview.test.ts.
import { MS_PER_DAY, assignLanes, makeMapping, mappingLevel, xOf, type LayoutRect, type TimeMapping } from './timeline';
import type { EntryKind, Ms, TimelineEntry } from './types';

export type Tier = 'rich' | 'medium' | 'title' | 'dot';
export const TIERS: Tier[] = ['rich', 'medium', 'title', 'dot'];

export interface TierSpec { tier: Tier; width: number; height: number; lines: number; thumb: boolean }
export interface PreviewConfig { laneGap: number; horizontalGap: number; topInset: number; dotSize: number; badgeSize: number }
export const defaultPreviewConfig: PreviewConfig = { laneGap: 8, horizontalGap: 6, topInset: 10, dotSize: 14, badgeSize: 28 };

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

/** Card sizes for a zoom level (points per day). More points per day → wider, taller-text cards. */
export function tierSpecs(ppd: number, cfg: PreviewConfig = defaultPreviewConfig): Record<Tier, TierSpec> {
  const rich = clamp(120 + ppd * 0.9, 170, 270);
  return {
    rich: { tier: 'rich', width: rich, height: 108, lines: 4, thumb: rich >= 200 },
    medium: { tier: 'medium', width: clamp(100 + ppd * 0.7, 140, 220), height: 76, lines: 2, thumb: false },
    title: { tier: 'title', width: clamp(60 + ppd * 0.8, 88, 170), height: 34, lines: 0, thumb: false },
    dot: { tier: 'dot', width: cfg.dotSize, height: cfg.dotSize, lines: 0, thumb: false },
  };
}

export interface PreviewCard {
  id: string; kind: EntryKind; frame: LayoutRect; tier: Exclude<Tier, 'dot'>; lane: number;
  title: string; text: string; lines: number;
  /** Media id of a thumbnail to show (photo itself, or a note's first image), only when the tier has room for it. */
  thumb: string | null; start: Ms; duration: number;
}
export interface PreviewDot { id: string; kind: EntryKind; frame: LayoutRect; title: string; start: Ms }
export interface PreviewCluster { id: string; frame: LayoutRect; count: number; entryIds: string[]; start: Ms; end: Ms; noteCount: number; mediaCount: number; recordingCount: number }
export interface PreviewLayout {
  cards: PreviewCard[]; dots: PreviewDot[]; clusters: PreviewCluster[];
  /** Tallest crowd, px (≤ available height unless even dots do not fit). */
  contentHeight: number;
  tierCounts: Record<Tier, number>;
}

const cmpStr = (a: string, b: string) => (a < b ? -1 : a > b ? 1 : 0);
const hasBody = (e: TimelineEntry) => e.kind === 'photo' || !!(e.preview && e.preview.trim()) || !!e.image || (e.kind === 'note' && e.snippet.trim().length > 0 && e.snippet.trim() !== e.title.trim());

/** First tier an entry can use at all: entries with nothing to preview start at a title chip. */
export const startTier = (e: TimelineEntry): number => (hasBody(e) ? 0 : 2);

interface Rect { x: number; w: number; h: number; items: number[]; dotGroup: boolean }

function buildRects(xs: number[], tiers: number[], specs: Record<Tier, TierSpec>, cfg: PreviewConfig): Rect[] {
  const rects: Rect[] = [];
  let group: { first: number; end: number; items: number[] } | null = null;
  const flush = () => {
    if (!group) return;
    const many = group.items.length > 1;
    const w = many ? Math.max(cfg.badgeSize, group.end - xs[group.first]) : cfg.dotSize;
    rects.push({ x: xs[group.first], w, h: many ? cfg.badgeSize : cfg.dotSize, items: group.items, dotGroup: true });
    group = null;
  };
  // entries are sorted by x; dot-tier entries that touch merge into one rect
  for (let i = 0; i < xs.length; i++) {
    if (tiers[i] === 3) {
      if (group && xs[i] < group.end + 2) { group.items.push(i); group.end = Math.max(group.end, xs[i] + cfg.dotSize); }
      else { flush(); group = { first: i, end: xs[i] + cfg.dotSize, items: [i] }; }
    } else rects.push({ x: xs[i], w: specs[TIERS[tiers[i]]].width, h: specs[TIERS[tiers[i]]].height, items: [i], dotGroup: false });
  }
  flush();
  rects.sort((a, b) => a.x - b.x || a.items[0] - b.items[0]);
  return rects;
}

/** Splits x-sorted rects into crowds (transitively overlapping, including the horizontal gap). */
function crowds(rects: Rect[], gap: number): Rect[][] {
  const out: Rect[][] = [];
  let cur: Rect[] = [];
  let end = -Infinity;
  for (const r of rects) {
    if (cur.length && r.x >= end + gap) { out.push(cur); cur = []; end = -Infinity; }
    cur.push(r);
    end = Math.max(end, r.x + r.w);
  }
  if (cur.length) out.push(cur);
  return out;
}

const crowdHeight = (laneCount: number, laneH: number, cfg: PreviewConfig) => cfg.topInset + laneCount * (laneH + cfg.laneGap);

export function previewLayout(entries: TimelineEntry[], m: TimeMapping, availableHeight: number, cfg: PreviewConfig = defaultPreviewConfig): PreviewLayout {
  const sorted = [...entries].sort((a, b) => a.start - b.start || cmpStr(a.id, b.id));
  const n = sorted.length;
  const specs = tierSpecs(m.pointsPerDay, cfg);
  const xs = sorted.map((e) => xOf(m, e.start));
  const tiers = sorted.map(startTier);
  const budget = Math.max(availableHeight, cfg.topInset + cfg.dotSize + cfg.laneGap);

  let rects: Rect[] = [];
  let laneInfo: { lanes: number[]; laneCount: number; laneH: number; crowd: Rect[] }[] = [];
  for (let iter = 0; iter < TIERS.length + 1; iter++) {
    rects = buildRects(xs, tiers, specs, cfg);
    laneInfo = [];
    let changed = false;
    for (const crowd of crowds(rects, cfg.horizontalGap)) {
      const { lanes, laneCount } = assignLanes(crowd.map((r) => [r.x, r.w] as [number, number]), cfg.horizontalGap);
      const laneH = Math.max(...crowd.map((r) => r.h));
      laneInfo.push({ lanes, laneCount, laneH, crowd });
      if (crowdHeight(laneCount, laneH, cfg) > budget) {
        // demote the richest not-yet-dot cards in this crowd by one step
        let best = 3;
        for (const r of crowd) if (!r.dotGroup) best = Math.min(best, tiers[r.items[0]]);
        if (best < 3) {
          for (const r of crowd) if (!r.dotGroup && tiers[r.items[0]] === best) { tiers[r.items[0]] = best + 1; changed = true; }
        }
      }
    }
    if (!changed) break;
  }

  const cards: PreviewCard[] = [];
  const dots: PreviewDot[] = [];
  const clusters: PreviewCluster[] = [];
  const tierCounts: Record<Tier, number> = { rich: 0, medium: 0, title: 0, dot: 0 };
  let contentHeight = 0;
  for (const { lanes, laneCount, laneH, crowd } of laneInfo) {
    contentHeight = Math.max(contentHeight, crowdHeight(laneCount, laneH, cfg));
    crowd.forEach((r, k) => {
      const y = cfg.topInset + lanes[k] * (laneH + cfg.laneGap);
      if (r.dotGroup) {
        if (r.items.length === 1) {
          const e = sorted[r.items[0]];
          dots.push({ id: e.id, kind: e.kind, title: e.title, start: e.start, frame: { x: r.x, y, width: r.w, height: r.h } });
          tierCounts.dot++;
        } else {
          const es = r.items.map((i) => sorted[i]);
          const cnt = (f: (k: EntryKind) => boolean) => es.filter((e) => f(e.kind)).length;
          clusters.push({
            id: es[0].id, entryIds: es.map((e) => e.id), count: es.length, start: es[0].start, end: es[es.length - 1].start,
            noteCount: cnt((x) => x === 'note'), recordingCount: cnt((x) => x === 'recording'), mediaCount: cnt((x) => x === 'photo' || x === 'video'),
            frame: { x: r.x, y, width: r.w, height: r.h },
          });
          tierCounts.dot += es.length;
        }
      } else {
        const i = r.items[0];
        const e = sorted[i];
        const spec = specs[TIERS[tiers[i]]];
        const thumbId = spec.thumb ? (e.kind === 'photo' ? e.id : e.image ?? null) : null;
        const textWidth = spec.width - 16 - (thumbId && e.kind !== 'photo' ? 64 : 0);
        const text = spec.lines > 0 ? previewText(e, spec.lines, Math.max(40, Math.floor(textWidth / 6.4))) : '';
        cards.push({
          id: e.id, kind: e.kind, tier: spec.tier as PreviewCard['tier'], lane: lanes[k], title: e.title, text, lines: spec.lines, thumb: thumbId,
          start: e.start, duration: e.duration, frame: { x: r.x, y, width: r.w, height: spec.height },
        });
        tierCounts[spec.tier]++;
      }
    });
  }
  void n;
  return { cards, dots, clusters, contentHeight, tierCounts };
}

/** First meaningful preview lines that fit in `lines` rows of `charsPerLine` characters each (an estimate; CSS clamps the rest). */
export function previewText(e: TimelineEntry, lines: number, charsPerLine: number): string {
  const src = (e.preview && e.preview.trim() ? e.preview : e.snippet).split('\n').map((l) => l.trim()).filter(Boolean);
  const out: string[] = [];
  let rows = 0;
  for (const l of src) {
    const need = Math.max(1, Math.ceil(l.length / charsPerLine));
    if (rows + need > lines) {
      const room = (lines - rows) * charsPerLine;
      if (room > 12) out.push(l.slice(0, room - 1).trimEnd() + '…');
      break;
    }
    out.push(l);
    rows += need;
    if (rows >= lines) break;
  }
  return out.join(' · ');
}

export const PREVIEW_LEVELS = ['month', 'week'] as const;
export const usesPreviews = (m: TimeMapping) => mappingLevel(m) !== 'day';
export { makeMapping, MS_PER_DAY };
