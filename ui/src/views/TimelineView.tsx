import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { format } from 'date-fns';
import { AudioLines, Camera, FileText, Film, Plus } from 'lucide-react';
import { useStore } from '../lib/store';
import {
  MS_PER_DAY, extent, layout, levelPointsPerDay, makeMapping, mappingLevel, snap, timeAt, xOf, zoom, zoomIn,
  type PlacedCluster, type PlacedItem, type TimeMapping, type ZoomLevel,
} from '../lib/timeline';
import { ticks } from '../lib/ticks';
import type { EntryKind } from '../lib/types';

const AXIS_H = 34;
const kindIcon: Record<EntryKind, JSX.Element> = {
  note: <FileText size={14} />, recording: <AudioLines size={14} />, photo: <Camera size={14} />, video: <Film size={14} />,
};
const fmtDur = (s: number) => (s >= 3600 ? `${Math.floor(s / 3600)}h ${Math.round((s % 3600) / 60)}m` : s >= 60 ? `${Math.round(s / 60)} min` : s > 0 ? `${Math.round(s)} s` : '');

export function TimelineView() {
  const { entries, tl, setTl, openNote, openMedia, newNote } = useStore();
  const ref = useRef<HTMLDivElement>(null);
  const [vw, setVw] = useState(900);
  const [vh, setVh] = useState(500);
  const now = useMemo(() => Date.now(), []);
  const [origin, endMs] = useMemo(() => {
    const [a, b] = extent(entries, 30 * MS_PER_DAY, now);
    return [a, b] as const;
  }, [entries, now]);

  const [view, setView] = useState<{ ppd: number; scroll: number } | null>(null);
  const viewRef = useRef(view);
  viewRef.current = view;
  const mapping: TimeMapping = useMemo(() => makeMapping(origin, view?.ppd ?? 160), [origin, view?.ppd]);
  const scroll = view?.scroll ?? 0;

  // measure viewport
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const ro = new ResizeObserver(() => { setVw(el.clientWidth); setVh(el.clientHeight); });
    ro.observe(el);
    setVw(el.clientWidth); setVh(el.clientHeight);
    return () => ro.disconnect();
  }, []);

  const clampScroll = useCallback((s: number, m: TimeMapping) => {
    const total = xOf(m, endMs);
    return Math.max(-vw / 2, Math.min(total - vw / 2, s));
  }, [endMs, vw]);

  // initial placement (restore saved position, else week level centred on "now")
  useEffect(() => {
    if (view || !vw) return;
    const ppd = tl?.ppd ?? levelPointsPerDay('week');
    const m = makeMapping(origin, ppd);
    const centre = tl?.centerMs ?? now - 2 * MS_PER_DAY;
    setView({ ppd, scroll: xOf(m, centre) - vw / 2 });
  }, [view, vw, origin, tl, now]);

  // remember position for when we come back
  useEffect(() => {
    if (!view) return;
    setTl({ ppd: view.ppd, centerMs: timeAt(mapping, view.scroll + vw / 2) });
  }, [view, mapping, vw, setTl]);

  // ---- animation between scales
  const anim = useRef<number | null>(null);
  const animGoal = useRef<{ ppd: number; scroll: number } | null>(null);
  const animateTo = useCallback((target: (m: TimeMapping, s: number) => { mapping: TimeMapping; scrollOffset: number }) => {
    const v = viewRef.current;
    if (!v) return;
    // a second command issued mid-animation builds on where the first one is heading
    const base = animGoal.current ?? { ppd: v.ppd, scroll: v.scroll };
    const goal = target(makeMapping(origin, base.ppd), base.scroll);
    animGoal.current = { ppd: goal.mapping.pointsPerDay, scroll: goal.scrollOffset };
    const m0 = makeMapping(origin, v.ppd);
    // interpolate in log-scale, keeping the viewport-centre instant moving linearly
    const t0 = performance.now();
    const c0 = timeAt(m0, v.scroll + vw / 2);
    const c1 = timeAt(goal.mapping, goal.scrollOffset + vw / 2);
    if (anim.current) cancelAnimationFrame(anim.current);
    const step = (now2: number) => {
      const k = Math.min(1, (now2 - t0) / 260);
      const e = 1 - Math.pow(1 - k, 3);
      const ppd = Math.exp(Math.log(v.ppd) + (Math.log(goal.mapping.pointsPerDay) - Math.log(v.ppd)) * e);
      const m = makeMapping(origin, ppd);
      const c = c0 + (c1 - c0) * e;
      setView({ ppd: m.pointsPerDay, scroll: xOf(m, c) - vw / 2 });
      anim.current = k < 1 ? requestAnimationFrame(step) : null;
      if (k >= 1) animGoal.current = null;
    };
    anim.current = requestAnimationFrame(step);
  }, [origin, vw]);

  const gotoLevel = (lv: ZoomLevel) => animateTo((m, s) => snap(m, lv, vw, s));
  const goToday = () => animateTo((m) => {
    const nm = m;
    return { mapping: nm, scrollOffset: xOf(nm, Date.now()) - vw / 2 };
  });

  // ---- gestures: ctrl/pinch wheel zoom, wheel pan, drag pan, 2-pointer pinch, Safari gesture events
  const applyZoom = useCallback((factor: number, anchorX: number) => {
    const v = viewRef.current;
    if (!v) return;
    if (anim.current) { cancelAnimationFrame(anim.current); anim.current = null; }
    animGoal.current = null;
    const r = zoom(makeMapping(origin, v.ppd), factor, anchorX, v.scroll);
    setView({ ppd: r.mapping.pointsPerDay, scroll: clampScroll(r.scrollOffset, r.mapping) });
  }, [origin, clampScroll]);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      const rect = el.getBoundingClientRect();
      if (e.ctrlKey || e.metaKey) {
        applyZoom(Math.exp(-e.deltaY * (e.deltaMode === 1 ? 0.05 : 0.01)), e.clientX - rect.left);
      } else {
        const v = viewRef.current;
        if (!v) return;
        const dx = Math.abs(e.deltaX) > Math.abs(e.deltaY) ? e.deltaX : e.shiftKey ? e.deltaY : e.deltaX || e.deltaY;
        setView({ ppd: v.ppd, scroll: clampScroll(v.scroll + dx, makeMapping(origin, v.ppd)) });
      }
    };
    let gestureBase = 1;
    const gs = (e: Event) => { e.preventDefault(); gestureBase = 1; };
    const gc = (e: Event) => {
      e.preventDefault();
      const scale = (e as unknown as { scale: number }).scale;
      applyZoom(scale / gestureBase, el.clientWidth / 2);
      gestureBase = scale;
    };
    el.addEventListener('wheel', onWheel, { passive: false });
    el.addEventListener('gesturestart', gs);
    el.addEventListener('gesturechange', gc);
    return () => {
      el.removeEventListener('wheel', onWheel);
      el.removeEventListener('gesturestart', gs);
      el.removeEventListener('gesturechange', gc);
    };
  }, [applyZoom, clampScroll, origin]);

  const pointers = useRef(new Map<number, { x: number; y: number }>());
  const dragRef = useRef<{ x: number; scroll: number; moved: boolean } | null>(null);
  const pinchRef = useRef<number | null>(null);
  const onPointerDown = (e: React.PointerEvent) => {
    if ((e.target as HTMLElement).closest('button')) return;
    (e.currentTarget as HTMLElement).setPointerCapture?.(e.pointerId);
    pointers.current.set(e.pointerId, { x: e.clientX, y: e.clientY });
    if (pointers.current.size === 1 && viewRef.current) dragRef.current = { x: e.clientX, scroll: viewRef.current.scroll, moved: false };
    if (pointers.current.size === 2) {
      const [a, b] = [...pointers.current.values()];
      pinchRef.current = Math.hypot(a.x - b.x, a.y - b.y);
      dragRef.current = null;
    }
  };
  const onPointerMove = (e: React.PointerEvent) => {
    if (!pointers.current.has(e.pointerId)) return;
    pointers.current.set(e.pointerId, { x: e.clientX, y: e.clientY });
    if (pointers.current.size === 2 && pinchRef.current) {
      const [a, b] = [...pointers.current.values()];
      const d = Math.hypot(a.x - b.x, a.y - b.y);
      const rect = ref.current!.getBoundingClientRect();
      applyZoom(d / pinchRef.current, (a.x + b.x) / 2 - rect.left);
      pinchRef.current = d;
    } else if (dragRef.current && viewRef.current) {
      const dx = e.clientX - dragRef.current.x;
      if (Math.abs(dx) > 3) dragRef.current.moved = true;
      if (dragRef.current.moved) {
        const v = viewRef.current;
        setView({ ppd: v.ppd, scroll: clampScroll(dragRef.current.scroll - dx, makeMapping(origin, v.ppd)) });
      }
    }
  };
  const onPointerUp = (e: React.PointerEvent) => {
    pointers.current.delete(e.pointerId);
    if (pointers.current.size < 2) pinchRef.current = null;
    setTimeout(() => { dragRef.current = null; }, 0);
  };
  const wasDrag = () => !!dragRef.current?.moved;

  // keyboard: +/- zoom, arrows pan
  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === '+' || e.key === '=') applyZoom(1.4, vw / 2);
    else if (e.key === '-') applyZoom(1 / 1.4, vw / 2);
    else if (e.key === 'ArrowRight') setView((v) => v && { ...v, scroll: clampScroll(v.scroll + 120, mapping) });
    else if (e.key === 'ArrowLeft') setView((v) => v && { ...v, scroll: clampScroll(v.scroll - 120, mapping) });
  };

  // ---- layout + visible window
  const lay = useMemo(() => layout(entries, mapping), [entries, mapping]);
  const level = mappingLevel(mapping);
  const margin = 400;
  const vis = (f: { x: number; width: number }) => f.x + f.width >= scroll - margin && f.x <= scroll + vw + margin;
  const tickList = useMemo(
    () => ticks(mapping.pointsPerDay, timeAt(mapping, scroll - margin), timeAt(mapping, scroll + vw + margin)),
    [mapping, scroll, vw],
  );
  const nowX = xOf(mapping, now) - scroll;
  const centreDate = timeAt(mapping, scroll + vw / 2);

  const openItem = (it: PlacedItem) => {
    if (wasDrag()) return;
    if (it.kind === 'note') openNote(it.id); else openMedia(it.id);
  };
  const openCluster = (c: PlacedCluster) => {
    if (wasDrag()) return;
    const mid = (c.start + c.end) / 2;
    animateTo((m) => zoomIn(m, mid, vw));
  };
  const entryById = useMemo(() => new Map(entries.map((e) => [e.id, e])), [entries]);

  return (
    <div className="timeline" data-level={level} data-testid="timeline">
      <div className="tl-toolbar">
        <div className="seg" role="tablist" aria-label="Zoom level">
          {(['month', 'week', 'day'] as ZoomLevel[]).map((lv) => (
            <button key={lv} role="tab" aria-selected={level === lv} className={level === lv ? 'on' : ''} data-testid={`zoom-${lv}`} onClick={() => gotoLevel(lv)}>
              {lv[0].toUpperCase() + lv.slice(1)}
            </button>
          ))}
        </div>
        <button className="ghost" onClick={goToday} data-testid="today">Today</button>
        <span className="tl-date" data-testid="centre-date">{format(centreDate, level === 'day' ? 'EEEE d MMMM yyyy, HH:mm' : level === 'week' ? 'd MMMM yyyy' : 'MMMM yyyy')}</span>
        <span className="spacer" />
        <span className="hint">Pinch or ⌘/Ctrl+scroll to zoom · drag to pan</span>
        <button className="primary" onClick={() => newNote('', centreDate)} data-testid="new-note-at-date"><Plus size={14} /> Note here</button>
      </div>
      <div
        className="tl-surface" ref={ref} tabIndex={0} data-testid="tl-surface"
        onPointerDown={onPointerDown} onPointerMove={onPointerMove} onPointerUp={onPointerUp} onPointerCancel={onPointerUp} onKeyDown={onKeyDown}
      >
        <div className="tl-axis" style={{ height: AXIS_H }}>
          {tickList.map((t) => (
            <div key={t.t} className={`tick ${t.major ? 'major' : ''}`} style={{ left: xOf(mapping, t.t) - scroll }}>
              <span>{t.label}</span>
            </div>
          ))}
        </div>
        <div className="tl-grid">
          {tickList.map((t) => <div key={t.t} className={`gridline ${t.major ? 'major' : ''}`} style={{ left: xOf(mapping, t.t) - scroll }} />)}
          {nowX > -2 && nowX < vw + 2 && <div className="now-line" style={{ left: nowX }}><span>now</span></div>}
        </div>
        <div className="tl-content" style={{ top: AXIS_H, height: Math.max(vh - AXIS_H, lay.contentHeight) }}>
          {lay.clusters.filter((c) => vis(c.frame)).map((c) => (
            <button key={c.id} className="block cluster" data-testid="cluster" data-cluster-count={c.count} data-narrow={c.frame.width < 84 ? 1 : 0}
              style={{ left: c.frame.x - scroll, top: c.frame.y, width: c.frame.width, height: c.frame.height }}
              onClick={() => openCluster(c)} title={`${c.count} entries — click to zoom in`}>
              <span className="count">{c.count}</span>
              <span className="label"><b>{c.title}</b>
                <small>{[c.noteCount && `${c.noteCount} note${c.noteCount > 1 ? 's' : ''}`, c.recordingCount && `${c.recordingCount} rec`, c.mediaCount && `${c.mediaCount} media`].filter(Boolean).join(' · ')}</small>
              </span>
            </button>
          ))}
          {lay.items.filter((i) => vis(i.frame)).map((i) => {
            const e = entryById.get(i.id);
            return (
              <button key={i.id} className={`block item ${i.kind}`} data-testid="entry" data-kind={i.kind}
                style={{ left: i.frame.x - scroll, top: i.frame.y, width: i.frame.width, height: i.frame.height }}
                onClick={() => openItem(i)} title={i.title}>
                <span className="ico">{kindIcon[i.kind]}</span>
                <span className="label">
                  {i.frame.width > 70 && <b>{i.title}</b>}
                  {i.showsSnippet && <small>{i.snippet}</small>}
                  {!i.showsSnippet && e && e.duration > 0 && i.frame.width > 110 && <small>{fmtDur(e.duration)}</small>}
                </span>
              </button>
            );
          })}
          {!entries.length && <div className="empty">Nothing here yet. Create a note or import a recording from the Library.</div>}
        </div>
      </div>
    </div>
  );
}
