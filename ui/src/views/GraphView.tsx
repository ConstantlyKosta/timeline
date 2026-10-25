import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import Graph from 'graphology';
import Sigma from 'sigma';
import { EdgeLineProgram, NodeCircleProgram, createEdgeArrowProgram } from 'sigma/rendering';
import { createEdgeCurveProgram } from '@sigma/edge-curve';
import { createNodeImageProgram } from '@sigma/node-image';
import { Crosshair, Maximize2, Pin, PinOff, SlidersHorizontal, ZoomIn, ZoomOut, Share2, Eye, Map as MapIcon } from 'lucide-react';
import { api, cmd } from '../lib/api';
import { useStore } from '../lib/store';
import type { GraphRaw, GraphViewRow } from '../lib/types';
import { buildDisplay, facets, fade, kindLabel, mergeSettings, DEFAULT_SETTINGS, type Display, type GraphSettings, type DNode } from '../lib/graphModel';
import { ForceSim, type SimNode } from '../lib/forceSim';
import { shapeIcon } from '../lib/shapeIcons';
import { ContextMenu, type MenuItem } from '../components/ContextMenu';
import { GraphPanel } from './GraphPanel';
import { Legend } from './GraphLegend';

const SETTINGS_KEY = 'timeline.graph.settings.v1';
const loadSettings = (): GraphSettings => {
  try { return mergeSettings(JSON.parse(localStorage.getItem(SETTINGS_KEY) ?? 'null')); } catch { return mergeSettings(null); }
};

const arrowHead = { extremity: 'target' as const, lengthToThicknessRatio: 5, widenessToThicknessRatio: 3.5 };
const nodePrograms = {
  circle: NodeCircleProgram,
  shape: createNodeImageProgram({ drawingMode: 'color', objectFit: 'contain', keepWithinCircle: false, size: { mode: 'force', value: 128 } }),
  thumb: createNodeImageProgram({ drawingMode: 'background', objectFit: 'cover', keepWithinCircle: true, size: { mode: 'max', value: 256 } }),
};
const edgePrograms = {
  line: EdgeLineProgram,
  arrow: createEdgeArrowProgram({ lengthToThicknessRatio: 5, widenessToThicknessRatio: 3.5 }),
  curved: createEdgeCurveProgram({ arrowHead: null }),
  curvedArrow: createEdgeCurveProgram({ arrowHead }),
};
const MAX_IMAGES = 200;

interface Tip { id: string; x: number; y: number }
interface Ctx { x: number; y: number; id: string }

const css = (name: string) => getComputedStyle(document.documentElement).getPropertyValue(name).trim() || '#888888';

export function GraphView() {
  const { openNote, openByTitle, fail, notes, theme, selectedNoteId } = useStore();
  const [data, setData] = useState<GraphRaw | null>(null);
  const [settings, setSettings] = useState<GraphSettings>(loadSettings);
  const [views, setViews] = useState<GraphViewRow[]>([]);
  const [panel, setPanel] = useState(true);
  const [mini, setMini] = useState(true);
  const [tip, setTip] = useState<Tip | null>(null);
  const [ctx, setCtx] = useState<Ctx | null>(null);
  const [webglError, setWebglError] = useState<string | null>(null);
  const [settled, setSettled] = useState(false);

  const container = useRef<HTMLDivElement>(null);
  const miniRef = useRef<HTMLCanvasElement>(null);
  const sigmaRef = useRef<Sigma | null>(null);
  const graphRef = useRef<Graph | null>(null);
  const simRef = useRef<ForceSim | null>(null);
  const positions = useRef(new Map<string, { x: number; y: number }>());
  const posLoaded = useRef<Promise<void> | null>(null);
  const hovered = useRef<string | null>(null);
  const displayRef = useRef<Display | null>(null);
  const lastSig = useRef('');
  const saveTimer = useRef<number | undefined>(undefined);
  const dragged = useRef<{ id: string; moved: boolean } | null>(null);
  const suppressClick = useRef(false);
  const settingsRef = useRef(settings);
  settingsRef.current = settings;
  const [bump, setBump] = useState(0);

  // ---- data ----------------------------------------------------------------------------------
  useEffect(() => { cmd.graphData().then(setData).catch(fail); }, [notes, fail]);
  useEffect(() => { cmd.graphViews().then(setViews).catch(() => undefined); }, []);
  useEffect(() => { try { localStorage.setItem(SETTINGS_KEY, JSON.stringify(settings)); } catch { /* storage full or blocked */ } }, [settings]);

  const display = useMemo(() => (data ? buildDisplay(data, settings, theme) : null), [data, settings, theme]);
  displayRef.current = display;
  const fac = useMemo(() => (data ? facets(data) : null), [data]);

  const patch = useCallback((p: Partial<GraphSettings>) => setSettings((s) => ({ ...s, ...p })), []);
  const setForces = useCallback((p: Partial<GraphSettings['forces']>) => setSettings((s) => ({ ...s, forces: { ...s.forces, ...p } })), []);

  // ---- positions persistence -------------------------------------------------------------------
  const scheduleSave = useCallback(() => {
    window.clearTimeout(saveTimer.current);
    saveTimer.current = window.setTimeout(() => {
      const keep = new Set(displayRef.current ? [...displayRef.current.nodes.map((n) => n.id), ...(data?.nodes.map((n) => n.id) ?? [])] : []);
      const list = [...positions.current.entries()].filter(([id]) => keep.has(id)).map(([id, p]) => ({ id, x: p.x, y: p.y }));
      cmd.saveGraphPositions(list).catch(() => undefined);
    }, 900);
  }, [data]);
  const scheduleSaveRef = useRef(scheduleSave);
  scheduleSaveRef.current = scheduleSave;

  // ---- sigma lifecycle ---------------------------------------------------------------------------
  useEffect(() => {
    const el = container.current;
    if (!el) return;
    posLoaded.current = cmd.graphPositions().then((ps) => { for (const p of ps) positions.current.set(p.id, { x: p.x, y: p.y }); }).catch(() => undefined);
    const graph = new Graph({ multi: true, type: 'directed' });
    graphRef.current = graph;
    let sigma: Sigma;
    try {
      sigma = new Sigma(graph, el, {
        nodeProgramClasses: nodePrograms, edgeProgramClasses: edgePrograms, defaultNodeType: 'circle', defaultEdgeType: 'line',
        renderEdgeLabels: false, labelSize: 12, labelWeight: '600', labelFont: 'Inter, system-ui, sans-serif', labelColor: { color: css('--text') },
        labelRenderedSizeThreshold: 7, labelDensity: 1, zIndex: true, minCameraRatio: 0.03, maxCameraRatio: 12, stagePadding: 40,
        defaultDrawNodeHover: (ctx2, d, st) => {
          const text = (d as { label?: string }).label ?? '';
          ctx2.font = `600 ${st.labelSize}px ${st.labelFont}`;
          const w = ctx2.measureText(text).width + 14, h = st.labelSize + 10, x = d.x + d.size + 4, y = d.y - h / 2;
          ctx2.fillStyle = css('--panel'); ctx2.strokeStyle = css('--border'); ctx2.lineWidth = 1;
          ctx2.beginPath(); ctx2.roundRect(x, y, w, h, 7); ctx2.fill(); ctx2.stroke();
          ctx2.fillStyle = css('--text'); ctx2.textBaseline = 'middle'; ctx2.fillText(text, x + 7, d.y + 0.5);
          ctx2.beginPath(); ctx2.arc(d.x, d.y, d.size + 2.5, 0, Math.PI * 2); ctx2.strokeStyle = css('--accent'); ctx2.lineWidth = 2; ctx2.stroke();
        },
      });
    } catch (e) {
      setWebglError(e instanceof Error ? e.message : String(e));
      return;
    }
    sigmaRef.current = sigma;

    // highlight: hovered node + neighbours, or search matches
    sigma.setSetting('nodeReducer', (id, a) => {
      const d = displayRef.current;
      if (!d) return a;
      const bg = css('--bg');
      const h = hovered.current;
      if (h && graph.hasNode(h)) {
        if (id === h || graph.areNeighbors(id, h)) return { ...a, zIndex: 2, forceLabel: id === h || a.forceLabel };
        return { ...a, color: fade(String(a.color), bg, 0.16), type: 'circle', label: null, zIndex: 0 };
      }
      if (d.hasQuery) {
        if (a.matched) return { ...a, zIndex: 2, forceLabel: true };
        return { ...a, color: fade(String(a.color), bg, 0.14), type: 'circle', label: null, zIndex: 0 };
      }
      return a;
    });
    sigma.setSetting('edgeReducer', (id, a) => {
      const d = displayRef.current;
      if (!d) return a;
      const bg = css('--bg');
      const h = hovered.current;
      if (h && graph.hasNode(h)) {
        if (graph.source(id) === h || graph.target(id) === h) return { ...a, size: Number(a.size) * 1.5, zIndex: 2 };
        return { ...a, color: fade(String(a.color), bg, 0.08) };
      }
      if (d.hasQuery && !(graph.getNodeAttribute(graph.source(id), 'matched') && graph.getNodeAttribute(graph.target(id), 'matched'))) return { ...a, color: fade(String(a.color), bg, 0.08) };
      return a;
    });

    const clientOf = (e: { x: number; y: number }) => ({ x: e.x, y: e.y });
    sigma.on('enterNode', ({ node }) => {
      hovered.current = node;
      const p = sigma.graphToViewport({ x: graph.getNodeAttribute(node, 'x'), y: graph.getNodeAttribute(node, 'y') });
      setTip({ id: node, ...clientOf(p) });
      el.style.cursor = 'pointer';
      sigma.refresh({ skipIndexation: true });
    });
    sigma.on('leaveNode', () => {
      hovered.current = null; setTip(null); el.style.cursor = '';
      sigma.refresh({ skipIndexation: true });
    });
    sigma.on('clickNode', ({ node }) => {
      if (suppressClick.current) return;
      activate(node);
    });
    sigma.on('doubleClickNode', (e) => { e.preventSigmaDefault(); togglePin(e.node); });
    sigma.on('rightClickNode', (e) => {
      e.preventSigmaDefault(); e.event.original.preventDefault();
      const o = e.event.original as MouseEvent; setCtx({ x: o.clientX, y: o.clientY, id: e.node });
    });
    sigma.on('rightClickStage', (e) => e.event.original.preventDefault());
    sigma.on('downNode', (e) => {
      dragged.current = { id: e.node, moved: false };
      setTip(null);
    });
    sigma.getMouseCaptor().on('mousemovebody', (e) => {
      const d = dragged.current;
      if (!d) return;
      const p = sigma.viewportToGraph(e);
      if (!d.moved) { d.moved = true; sim()?.fix(d.id, p.x, p.y); }
      graph.setNodeAttribute(d.id, 'x', p.x); graph.setNodeAttribute(d.id, 'y', p.y);
      positions.current.set(d.id, { x: p.x, y: p.y });
      sim()?.fix(d.id, p.x, p.y); sim()?.reheat(0.2);
      e.preventSigmaDefault(); e.original.preventDefault(); e.original.stopPropagation();
    });
    const endDrag = () => {
      const d = dragged.current;
      dragged.current = null;
      if (d?.moved) {
        suppressClick.current = true; window.setTimeout(() => { suppressClick.current = false; }, 60);
        const p = positions.current.get(d.id);
        if (p && !settingsRef.current.pinned.includes(d.id)) setSettings((s) => ({ ...s, pinned: [...s.pinned, d.id] }));
        else if (p) sim()?.pin(d.id, p.x, p.y);
        scheduleSaveRef.current();
      }
    };
    sigma.on('upNode', endDrag); sigma.on('upStage', endDrag);
    sigma.getMouseCaptor().on('mouseup', endDrag);
    sigma.on('afterRender', () => drawMini());

    const fs = new ForceSim(settingsRef.current.forces, applyPositions, () => { setSettled(true); scheduleSaveRef.current(); });
    simRef.current = fs;

    (window as unknown as { __tlGraph: unknown }).__tlGraph = {
      ids: () => graph.nodes(),
      screenPos: (id: string) => { const r = el.getBoundingClientRect(); const p = sigma.graphToViewport({ x: graph.getNodeAttribute(id, 'x'), y: graph.getNodeAttribute(id, 'y') }); return { x: p.x + r.left, y: p.y + r.top }; },
      attrs: (id: string) => ({ ...graph.getNodeAttributes(id) }),
      edgeTypes: () => graph.mapEdges((_e, a) => a.type as string),
      pinned: () => settingsRef.current.pinned,
      settled: () => !fs.running,
      renderer: () => sigma,
    };
    setBump((b) => b + 1);
    const ro = new ResizeObserver(() => sigma.refresh());
    ro.observe(el);
    return () => {
      ro.disconnect(); fs.stop(); sigma.kill();
      sigmaRef.current = null; simRef.current = null; graphRef.current = null; window.clearTimeout(saveTimer.current);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const sim = () => simRef.current;
  function applyPositions(ns: SimNode[]) {
    const g = graphRef.current;
    if (!g) return;
    for (const n of ns) {
      if (n.x === undefined || n.y === undefined) continue;
      positions.current.set(n.id, { x: n.x, y: n.y });
      if (g.hasNode(n.id)) { g.setNodeAttribute(n.id, 'x', n.x); g.setNodeAttribute(n.id, 'y', n.y); }
    }
  }

  // ---- actions -----------------------------------------------------------------------------------
  function activate(id: string) {
    const d = displayRef.current?.nodes.find((n) => n.id === id);
    if (!d) return;
    if (d.kind === 'note') openNote(id);
    else if (d.kind === 'ghost') openByTitle(d.label).catch(fail);
    else if (d.kind === 'tag') patch({ query: `tag:${id.slice(4)}` });
    else if (d.kind === 'folder') patch({ query: `folder:${id.slice(7)}` });
  }
  function togglePin(id: string) {
    const p = positions.current.get(id);
    setSettings((s) => {
      const pinned = s.pinned.includes(id) ? s.pinned.filter((x) => x !== id) : [...s.pinned, id];
      return { ...s, pinned };
    });
    if (p && settingsRef.current.pinned.includes(id)) sim()?.release(id);
  }
  const fit = useCallback(() => { sigmaRef.current?.getCamera().animatedReset({ duration: 350 }); }, []);
  const zoom = (f: number) => { const c = sigmaRef.current?.getCamera(); if (c) c.animate({ ratio: c.getState().ratio * f }, { duration: 200 }); };
  function focusMatches() {
    const s = sigmaRef.current, g = graphRef.current;
    if (!s || !g) return;
    const ids = g.filterNodes((_n, a) => !!a.matched);
    if (!ids.length) return;
    const pts = ids.map((id) => s.getNodeDisplayData(id)!).filter(Boolean);
    const xs = pts.map((p) => p.x), ys = pts.map((p) => p.y);
    const dx = Math.max(...xs) - Math.min(...xs), dy = Math.max(...ys) - Math.min(...ys);
    s.getCamera().animate({ x: (Math.max(...xs) + Math.min(...xs)) / 2, y: (Math.max(...ys) + Math.min(...ys)) / 2, ratio: Math.min(1, Math.max(0.06, Math.max(dx, dy) * 1.5)) }, { duration: 350 });
  }
  function unpinAll() {
    for (const id of settingsRef.current.pinned) sim()?.release(id);
    patch({ pinned: [] });
  }
  function resetLayout() {
    positions.current.clear();
    const d = displayRef.current, fs = sim(), g = graphRef.current;
    if (!d || !fs || !g) return;
    lastSig.current = '';
    patch({ pinned: [] });
    cmd.saveGraphPositions([]).catch(() => undefined);
    setBump((b) => b + 1);
  }
  function setLocal(on: boolean, focus?: string) {
    const f = focus ?? settings.focusId ?? selectedNoteId ?? display?.nodes.filter((n) => n.kind === 'note').sort((a, b) => b.degree - a.degree)[0]?.id ?? null;
    patch({ local: on, focusId: f });
    window.setTimeout(fit, 400);
  }

  // ---- display -> graphology ---------------------------------------------------------------------
  useEffect(() => {
    const sigma = sigmaRef.current, g = graphRef.current, fs = simRef.current;
    if (!sigma || !g || !fs || !display) return;
    let cancelled = false;
    (async () => {
      await posLoaded.current;
      if (cancelled) return;
      const first = g.order === 0;
      const sigChanged = display.signature !== lastSig.current;
      lastSig.current = display.signature;
      // current positions win over stored ones
      const pos = new Map(positions.current);
      g.forEachNode((id, a) => pos.set(id, { x: a.x, y: a.y }));
      const fresh = display.nodes.filter((n) => !pos.has(n.id)).length;
      if (sigChanged || first) {
        fs.setData(display.nodes.map((n) => ({ id: n.id, r: n.size, pinned: n.pinned })), display.edges, pos, 0);
        const n = display.nodes.length;
        if (fresh > 0 && (first || fresh > n * 0.5)) { fs.settle(n > 1500 ? 70 : n > 600 ? 140 : 300); setSettled(true); }
        else if (fresh > 0) fs.reheat(0.5);
        else setSettled(true);
        scheduleSaveRef.current();
      } else {
        // styling-only change: keep the simulation, just refresh sizes/pins
        fs.setData(display.nodes.map((n) => ({ id: n.id, r: n.size, pinned: n.pinned })), display.edges, pos, 0);
      }
      syncGraph(g, display, settings, positions.current, fs);
      sigma.setSetting('renderLabels', settings.labels !== 'never');
      sigma.setSetting('labelRenderedSizeThreshold', settings.labels === 'always' ? 0 : 7);
      sigma.setSetting('labelDensity', settings.labels === 'always' ? 4 : 1);
      sigma.setSetting('labelColor', { color: css('--text') });
      sigma.refresh();
      if (first && g.order) window.setTimeout(() => sigma.getCamera().animatedReset({ duration: 0 }), 30);
    })();
    return () => { cancelled = true; };
  }, [display, bump]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => { simRef.current?.setForces(settings.forces); }, [settings.forces]);

  // ---- minimap -------------------------------------------------------------------------------------
  const drawMini = useCallback(() => {
    const cv = miniRef.current, s = sigmaRef.current, g = graphRef.current;
    if (!cv || !s || !g || g.order === 0) return;
    const ctx2 = cv.getContext('2d')!;
    const W = cv.width, H = cv.height;
    ctx2.clearRect(0, 0, W, H);
    let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
    g.forEachNode((_i, a) => { x0 = Math.min(x0, a.x); y0 = Math.min(y0, a.y); x1 = Math.max(x1, a.x); y1 = Math.max(y1, a.y); });
    const pad = 8, sc = Math.min((W - pad * 2) / Math.max(1e-6, x1 - x0), (H - pad * 2) / Math.max(1e-6, y1 - y0));
    const ox = (W - (x1 - x0) * sc) / 2, oy = (H - (y1 - y0) * sc) / 2;
    const mx = (x: number) => ox + (x - x0) * sc, my = (y: number) => oy + (y - y0) * sc;
    g.forEachNode((_i, a) => { ctx2.fillStyle = String(a.color); ctx2.globalAlpha = 0.9; ctx2.fillRect(mx(a.x) - 1.5, my(a.y) - 1.5, 3, 3); });
    ctx2.globalAlpha = 1;
    const { width, height } = s.getDimensions();
    const a = s.viewportToGraph({ x: 0, y: 0 }), b = s.viewportToGraph({ x: width, y: height });
    ctx2.strokeStyle = css('--accent'); ctx2.lineWidth = 1.5;
    ctx2.strokeRect(mx(Math.min(a.x, b.x)), my(Math.min(a.y, b.y)), Math.abs(b.x - a.x) * sc, Math.abs(b.y - a.y) * sc);
    (cv as unknown as { _map: unknown })._map = { x0, y0, sc, ox, oy };
  }, []);
  function miniClick(e: React.MouseEvent<HTMLCanvasElement>) {
    const cv = miniRef.current, s = sigmaRef.current;
    const m = (cv as unknown as { _map?: { x0: number; y0: number; sc: number; ox: number; oy: number } } | null)?._map;
    if (!cv || !s || !m) return;
    const r = cv.getBoundingClientRect();
    const px = ((e.clientX - r.left) / r.width) * cv.width, py = ((e.clientY - r.top) / r.height) * cv.height;
    const gp = { x: (px - m.ox) / m.sc + m.x0, y: (py - m.oy) / m.sc + m.y0 };
    const f = s.viewportToFramedGraph(s.graphToViewport(gp));
    s.getCamera().animate({ x: f.x, y: f.y }, { duration: 250 });
  }
  useEffect(() => { if (mini) drawMini(); }, [mini, display, drawMini]);

  // ---- views -------------------------------------------------------------------------------------------
  async function saveView(name: string) {
    const { query, ...rest } = settings; void query;
    await cmd.saveGraphView(name, { ...rest, query: '' });
    setViews(await cmd.graphViews());
  }
  const applyView = (json: unknown) => { setSettings({ ...mergeSettings(json), query: '' }); window.setTimeout(fit, 500); };
  async function deleteView(name: string) { await cmd.deleteGraphView(name); setViews(await cmd.graphViews()); }

  // ---- render -----------------------------------------------------------------------------------------------
  const tipNode: DNode | undefined = tip ? display?.nodes.find((n) => n.id === tip.id) : undefined;
  const ctxNode = ctx ? display?.nodes.find((n) => n.id === ctx.id) : undefined;
  const ctxItems: MenuItem[] = ctxNode ? [
    ...(ctxNode.kind === 'note' || ctxNode.kind === 'ghost' ? [{ label: ctxNode.kind === 'ghost' ? 'Create this note' : 'Open note', onSelect: () => activate(ctxNode.id), testId: 'gm-open' }] : []),
    { label: ctxNode.pinned ? 'Unpin' : 'Pin in place', icon: ctxNode.pinned ? <PinOff size={14} /> : <Pin size={14} />, onSelect: () => togglePin(ctxNode.id), testId: 'gm-pin' },
    { label: `Local graph from here (${settings.depth} hops)`, icon: <Share2 size={14} />, onSelect: () => setLocal(true, ctxNode.id), testId: 'gm-local' },
  ] : [];

  return (
    <div className="graph" data-testid="graph-view" data-node-count={display?.nodes.length ?? 0} data-edge-count={display?.edges.length ?? 0} data-settled={settled ? '1' : '0'}>
      <div className="graph-canvas" ref={container} data-testid="graph-canvas" onContextMenu={(e) => e.preventDefault()} />
      {webglError && <div className="empty graph-fallback">The mind map needs WebGL, which is not available here ({webglError}).</div>}
      {data && data.nodes.length === 0 && <div className="empty">No notes to map yet.</div>}

      <div className="graph-toolbar">
        <input className="graph-search" data-testid="graph-search" value={settings.query} placeholder="Search… tag:work folder:Projects prop:status=done -word"
          onChange={(e) => patch({ query: e.target.value })} onKeyDown={(e) => { if (e.key === 'Enter') focusMatches(); if (e.key === 'Escape') patch({ query: '' }); }} />
        {display?.hasQuery && <span className="muted graph-count" data-testid="graph-match-count">{display.matchCount} match{display.matchCount === 1 ? '' : 'es'}</span>}
        <button className="ghost" title="Zoom in" onClick={() => zoom(0.6)}><ZoomIn size={16} /></button>
        <button className="ghost" title="Zoom out" onClick={() => zoom(1.6)}><ZoomOut size={16} /></button>
        <button className="ghost" title="Fit everything" data-testid="graph-fit" onClick={fit}><Maximize2 size={16} /></button>
        <button className={settings.local ? 'primary' : 'ghost'} title="Show only the neighbourhood of one note" data-testid="graph-local" onClick={() => setLocal(!settings.local)}><Crosshair size={16} /> Local</button>
        {settings.local && (
          <span className="graph-depth" data-testid="graph-depth">
            depth <button className="ghost" onClick={() => patch({ depth: Math.max(1, settings.depth - 1) })}>−</button>
            <b>{settings.depth}</b>
            <button className="ghost" onClick={() => patch({ depth: Math.min(8, settings.depth + 1) })}>+</button>
          </span>
        )}
        <span className="grow" />
        <button className="ghost" title={`${settings.pinned.length} pinned — click to unpin all`} data-testid="graph-unpin" disabled={!settings.pinned.length} onClick={unpinAll}><Pin size={16} /> {settings.pinned.length || ''}</button>
        <button className={mini ? 'ghost active' : 'ghost'} title="Minimap" onClick={() => setMini(!mini)}><MapIcon size={16} /></button>
        <button className={panel ? 'ghost active' : 'ghost'} title="Settings" data-testid="graph-settings-toggle" onClick={() => setPanel(!panel)}><SlidersHorizontal size={16} /></button>
      </div>

      {display && <Legend legend={display.legend} />}
      {display && <div className="graph-stats muted"><Eye size={12} /> {display.nodes.length} nodes · {display.edges.length} links{display.nodes.length < display.totals.nodes ? ` (filtered from ${display.totals.nodes})` : ''}</div>}
      {mini && <canvas ref={miniRef} className="graph-mini" width={320} height={220} data-testid="graph-minimap" onClick={miniClick} />}

      {tipNode && tip && <Tooltip node={tipNode} x={tip.x} y={tip.y} />}
      {ctx && <ContextMenu x={ctx.x} y={ctx.y} header={ctxNode?.label} items={ctxItems} onClose={() => setCtx(null)} />}
      {panel && data && fac && display && (
        <GraphPanel settings={settings} patch={patch} setForces={setForces} facets={fac} views={views} display={display}
          onApplyView={applyView} onSaveView={saveView} onDeleteView={deleteView} onResetLayout={resetLayout} onResetSettings={() => setSettings({ ...DEFAULT_SETTINGS, pinned: settings.pinned })} />
      )}
    </div>
  );
}

function Tooltip({ node, x, y }: { node: DNode; x: number; y: number }) {
  const raw = node.raw;
  const left = x + 18 > 540 ? x - 280 : x + 18;
  return (
    <div className="graph-tip" data-testid="graph-tooltip" style={{ left, top: Math.max(8, y - 20) }}>
      <div className="gt-title"><span className="dot" style={{ background: node.color }} />{node.label}</div>
      <div className="muted gt-sub">{kindLabel(node.kind)}{raw?.folder ? ` · ${raw.folder}` : ''} · {node.degree} link{node.degree === 1 ? '' : 's'}{node.members !== undefined ? ` · ${node.members} notes` : ''}</div>
      {raw?.image && <img className="gt-img" src={api.mediaUrl(raw.image)} alt="" />}
      {raw?.preview && <div className="gt-preview">{raw.preview}</div>}
      {raw && raw.tags.length > 0 && <div className="gt-tags">{raw.tags.slice(0, 8).map((t) => <span key={t}>#{t}</span>)}</div>}
      {raw && Object.keys(raw.props).length > 0 && <div className="gt-props">{Object.entries(raw.props).slice(0, 5).map(([k, v]) => <div key={k}><b>{k}</b> {v}</div>)}</div>}
    </div>
  );
}

function syncGraph(g: Graph, d: Display, s: GraphSettings, pos: Map<string, { x: number; y: number }>, fs: ForceSim) {
  const want = new Set(d.nodes.map((n) => n.id));
  for (const id of g.nodes()) if (!want.has(id)) g.dropNode(id);
  let images = 0;
  for (const n of d.nodes) {
    const p = pos.get(n.id) ?? { x: 0, y: 0 };
    const useImg = !!n.image && images++ < MAX_IMAGES;
    const attrs = {
      label: (n.pinned ? '📌 ' : '') + n.label, size: n.size, color: n.color, matched: n.matched, forceLabel: n.pinned,
      type: useImg ? 'thumb' : n.shape === 'circle' ? 'circle' : 'shape',
      image: useImg ? api.mediaUrl(n.image!) : n.shape === 'circle' ? undefined : shapeIcon(n.shape),
      dkind: n.kind,
    };
    if (g.hasNode(n.id)) g.mergeNodeAttributes(n.id, attrs);
    else g.addNode(n.id, { ...attrs, x: p.x, y: p.y });
  }
  g.clearEdges();
  const pairCount = new Map<string, number>();
  for (const e of d.edges) {
    const key = `${e.from}>${e.to}`;
    const k = pairCount.get(key) ?? 0;
    pairCount.set(key, k + 1);
    const arrow = s.showArrows && e.directed;
    const base = e.kind === 'tagnode' || e.kind === 'foldernode' ? 1 : 1.6;
    g.addEdgeWithKey(e.id, e.from, e.to, {
      color: e.color, size: base * s.edgeThickness, kind: e.kind,
      type: s.curved ? (arrow ? 'curvedArrow' : 'curved') : (arrow ? 'arrow' : 'line'),
      curvature: 0.22 + 0.2 * k,
    });
  }
  void fs;
}
