// Line-for-line TypeScript port of crates/timeline-core/src/timeline.rs.
// Used for per-frame zoom/pan so gestures never cross the IPC boundary.
// Parity with the Rust implementation is enforced by timeline.test.ts against fixtures/timeline_golden.json.
import type { EntryKind, Ms, TimelineEntry } from './types';

export type ZoomLevel = 'month' | 'week' | 'day';
export const MS_PER_DAY = 86_400_000;
export const MIN_POINTS_PER_DAY = 12;
export const MAX_POINTS_PER_DAY = 2400;

const LEVEL_PPD: Record<ZoomLevel, number> = { month: 36, week: 160, day: 1200 };
export const levelPointsPerDay = (l: ZoomLevel) => LEVEL_PPD[l];
export const zoomedIn = (l: ZoomLevel): ZoomLevel | null => (l === 'month' ? 'week' : l === 'week' ? 'day' : null);
export const zoomedOut = (l: ZoomLevel): ZoomLevel | null => (l === 'day' ? 'week' : l === 'week' ? 'month' : null);

export function levelForPointsPerDay(ppd: number): ZoomLevel {
  const monthWeek = Math.sqrt(LEVEL_PPD.month * LEVEL_PPD.week);
  const weekDay = Math.sqrt(LEVEL_PPD.week * LEVEL_PPD.day);
  return ppd < monthWeek ? 'month' : ppd < weekDay ? 'week' : 'day';
}
export const clampPpd = (ppd: number) => Math.min(MAX_POINTS_PER_DAY, Math.max(MIN_POINTS_PER_DAY, ppd));

/** Rust's f64::round rounds half away from zero; Math.round rounds half up. */
const roundAway = (x: number) => (x < 0 ? -Math.round(-x) : Math.round(x));

export interface TimeMapping { origin: Ms; pointsPerDay: number }
export const makeMapping = (origin: Ms, ppd: number): TimeMapping => ({ origin, pointsPerDay: clampPpd(ppd) });
const ppms = (m: TimeMapping) => m.pointsPerDay / MS_PER_DAY;
export const mappingLevel = (m: TimeMapping) => levelForPointsPerDay(m.pointsPerDay);
export const xOf = (m: TimeMapping, t: Ms) => (t - m.origin) * ppms(m);
export const timeAt = (m: TimeMapping, x: number): Ms => m.origin + roundAway(x / ppms(m));
export const widthForSeconds = (m: TimeMapping, s: number) => s * 1000 * ppms(m);
export const withPpd = (m: TimeMapping, ppd: number) => makeMapping(m.origin, ppd);

export interface ZoomResult { mapping: TimeMapping; scrollOffset: number }

export function zoom(m: TimeMapping, factor: number, anchorViewportX: number, scrollOffset: number): ZoomResult {
  const nm = withPpd(m, m.pointsPerDay * factor);
  const anchorExact = (scrollOffset + anchorViewportX) / ppms(m);
  const newX = anchorExact * ppms(nm);
  return { mapping: nm, scrollOffset: newX - anchorViewportX };
}
export function snap(m: TimeMapping, level: ZoomLevel, viewportWidth: number, scrollOffset: number): ZoomResult {
  return zoom(m, LEVEL_PPD[level] / m.pointsPerDay, viewportWidth / 2, scrollOffset);
}
export function zoomIn(m: TimeMapping, t: Ms, viewportWidth: number): ZoomResult {
  const lv = mappingLevel(m);
  const nm = withPpd(m, LEVEL_PPD[zoomedIn(lv) ?? lv]);
  return { mapping: nm, scrollOffset: xOf(nm, t) - viewportWidth / 2 };
}

// ---------------------------------------------------------------- clustering
export interface EntryCluster {
  id: string; entryIds: string[]; start: Ms; end: Ms; title: string;
  recordingCount: number; noteCount: number; mediaCount: number;
}
export interface ClusterConfig { maxGap: number; maxSpan: number }
export const defaultClusterConfig: ClusterConfig = { maxGap: 2 * MS_PER_DAY, maxSpan: 14 * MS_PER_DAY };

const entryEnd = (e: TimelineEntry) => e.start + roundAway(e.duration * 1000);
const cmpStr = (a: string, b: string) => (a < b ? -1 : a > b ? 1 : 0);
function sortedEntries(entries: TimelineEntry[]): TimelineEntry[] {
  return [...entries].sort((a, b) => a.start - b.start || cmpStr(a.id, b.id));
}

export function cluster(entries: TimelineEntry[], cfg: ClusterConfig = defaultClusterConfig): EntryCluster[] {
  const groups: TimelineEntry[][] = [];
  let current: TimelineEntry[] = [];
  let currentEnd = -Infinity;
  for (const e of sortedEntries(entries)) {
    if (current.length) {
      const gap = e.start - currentEnd;
      const span = entryEnd(e) - current[0].start;
      if (gap > cfg.maxGap || span > cfg.maxSpan) {
        groups.push(current);
        current = [];
      }
    }
    current.push(e);
    currentEnd = Math.max(entryEnd(e), ...current.map(entryEnd));
  }
  if (current.length) groups.push(current);
  return groups.map(makeCluster);
}

function makeCluster(g: TimelineEntry[]): EntryCluster {
  const counts = new Map<string, number>();
  for (const e of g) for (const t of new Set(e.tags)) counts.set(t, (counts.get(t) ?? 0) + 1);
  let top: string | null = null;
  let topN = 0;
  for (const k of [...counts.keys()].sort(cmpStr)) {
    const n = counts.get(k)!;
    if (n > topN) { top = k; topN = n; } // strict > keeps the alphabetically first on ties
  }
  const count = (f: (k: EntryKind) => boolean) => g.filter((e) => f(e.kind)).length;
  return {
    id: g[0].id,
    entryIds: g.map((e) => e.id),
    start: Math.min(...g.map((e) => e.start)),
    end: Math.max(...g.map(entryEnd)),
    title: top !== null ? `#${top}` : g[0].title || 'Untitled',
    recordingCount: count((k) => k === 'recording'),
    noteCount: count((k) => k === 'note'),
    mediaCount: count((k) => k === 'photo' || k === 'video'),
  };
}

// -------------------------------------------------------------------- layout
export interface LayoutRect { x: number; y: number; width: number; height: number }
export interface PlacedItem { id: string; kind: EntryKind; frame: LayoutRect; lane: number; title: string; snippet: string; showsSnippet: boolean }
export interface PlacedCluster {
  id: string; frame: LayoutRect; lane: number; title: string; count: number;
  recordingCount: number; noteCount: number; mediaCount: number; entryIds: string[]; start: Ms; end: Ms;
}
export interface LayoutResult { level: ZoomLevel; items: PlacedItem[]; clusters: PlacedCluster[]; laneCount: number; contentHeight: number }
export interface LayoutConfig { laneGap: number; horizontalGap: number; topInset: number }
export const defaultLayoutConfig: LayoutConfig = { laneGap: 8, horizontalGap: 6, topInset: 12 };

export const itemHeight = (l: ZoomLevel) => (l === 'month' ? 40 : l === 'week' ? 52 : 76);

export function itemWidth(e: TimelineEntry, m: TimeMapping): number {
  switch (mappingLevel(m)) {
    case 'month': return 40;
    case 'week':
      if (e.kind === 'recording' || e.kind === 'video') {
        const minutes = Math.max(e.duration, 0) / 60;
        return Math.min(480, Math.max(40, 40 + 18 * Math.sqrt(minutes)));
      }
      return e.kind === 'note' ? 120 : 56;
    case 'day': {
      const prop = widthForSeconds(m, e.duration);
      if (e.kind === 'recording' || e.kind === 'video') return Math.max(prop, 180);
      return e.kind === 'note' ? 200 : 96;
    }
  }
}

export function assignLanes(rects: [number, number][], horizontalGap: number): { lanes: number[]; laneCount: number } {
  const laneEnds: number[] = [];
  const lanes = new Array<number>(rects.length).fill(0);
  rects.forEach(([x, w], i) => {
    const lane = laneEnds.findIndex((end) => end + horizontalGap <= x);
    if (lane >= 0) { lanes[i] = lane; laneEnds[lane] = x + w; }
    else { lanes[i] = laneEnds.length; laneEnds.push(x + w); }
  });
  return { lanes, laneCount: laneEnds.length };
}

export function layout(
  entries: TimelineEntry[], m: TimeMapping,
  cfg: LayoutConfig = defaultLayoutConfig, ccfg: ClusterConfig = defaultClusterConfig,
): LayoutResult {
  const level = mappingLevel(m);
  const h = itemHeight(level);
  const yOf = (lane: number) => cfg.topInset + lane * (h + cfg.laneGap);
  if (level === 'month') {
    const clusters = cluster(entries, ccfg);
    const rects: [number, number][] = clusters.map((c) => {
      const x0 = xOf(m, c.start);
      return [x0, Math.max(xOf(m, c.end) - x0, 72)];
    });
    const { lanes, laneCount } = assignLanes(rects, cfg.horizontalGap);
    return {
      level, items: [], laneCount, contentHeight: yOf(laneCount) + cfg.topInset,
      clusters: clusters.map((c, i) => ({
        id: c.id, frame: { x: rects[i][0], y: yOf(lanes[i]), width: rects[i][1], height: h }, lane: lanes[i],
        title: c.title, count: c.entryIds.length, recordingCount: c.recordingCount, noteCount: c.noteCount,
        mediaCount: c.mediaCount, entryIds: c.entryIds, start: c.start, end: c.end,
      })),
    };
  }
  const sorted = sortedEntries(entries);
  const rects: [number, number][] = sorted.map((e) => [xOf(m, e.start), itemWidth(e, m)]);
  const { lanes, laneCount } = assignLanes(rects, cfg.horizontalGap);
  return {
    level, clusters: [], laneCount, contentHeight: yOf(laneCount) + cfg.topInset,
    items: sorted.map((e, i) => {
      const shows = level === 'day' && e.snippet.length > 0;
      return {
        id: e.id, kind: e.kind, frame: { x: rects[i][0], y: yOf(lanes[i]), width: rects[i][1], height: h },
        lane: lanes[i], title: e.title, snippet: shows ? e.snippet : '', showsSnippet: shows,
      };
    }),
  };
}

export function extent(entries: TimelineEntry[], paddingMs: number, now: Ms): [Ms, Ms] {
  if (!entries.length) return [now - 14 * MS_PER_DAY, now + 3 * MS_PER_DAY];
  const first = Math.min(...entries.map((e) => e.start));
  const last = Math.max(...entries.map(entryEnd));
  return [first - paddingMs, Math.max(last, now) + paddingMs];
}
