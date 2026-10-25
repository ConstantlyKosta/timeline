import { describe, expect, it } from 'vitest';
import golden from '../../../fixtures/timeline_golden.json';
import { layout, makeMapping, snap, zoom, zoomIn, type ZoomLevel } from './timeline';
import type { TimelineEntry } from './types';

// The golden file is produced by the Rust core (UPDATE_GOLDEN=1 cargo test -p timeline-core).
// Any divergence between the TS port and Rust fails here.
const entries = golden.entries as unknown as TimelineEntry[];
const origin = golden.origin;
const close = (a: unknown, b: unknown, path = ''): void => {
  if (typeof a === 'number' && typeof b === 'number') {
    expect(Math.abs(a - b), path).toBeLessThan(1e-6 * Math.max(1, Math.abs(b)));
  } else if (Array.isArray(a) && Array.isArray(b)) {
    expect(a.length, path).toBe(b.length);
    a.forEach((x, i) => close(x, b[i], `${path}[${i}]`));
  } else if (a && b && typeof a === 'object' && typeof b === 'object') {
    expect(Object.keys(a as object).sort(), path).toEqual(Object.keys(b as object).sort());
    for (const k of Object.keys(b as object)) close((a as any)[k], (b as any)[k], `${path}.${k}`);
  } else expect(a, path).toBe(b);
};

describe('TS timeline port matches the Rust golden fixture', () => {
  it.each(golden.layouts.map((l) => [l.pointsPerDay, l] as const))('layout @ %s pt/day', (ppd, c) => {
    close(layout(entries, makeMapping(origin, ppd)), c.layout, `layout@${ppd}`);
  });
  it('covers all three levels', () => {
    const levels = new Set(golden.layouts.map((l) => l.layout.level as ZoomLevel));
    expect([...levels].sort()).toEqual(['day', 'month', 'week']);
  });
  it.each(golden.zooms.map((z, i) => [i, z] as const))('zoom case %s', (_i, z) => {
    close(zoom(makeMapping(origin, 160), z.factor, z.anchor, z.offset), z.result);
  });
  it.each(golden.snaps.map((s) => [s.level, s] as const))('snap to %s', (_l, s) => {
    close(snap(makeMapping(origin, 160), s.level as ZoomLevel, 1000, 400), s.result);
  });
  it('zoomIn on a cluster', () => {
    const z = golden.zoomIn;
    close(zoomIn(makeMapping(origin, 36), z.target, z.viewportWidth), z.result);
  });
});
