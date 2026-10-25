import { useEffect, useMemo, useRef } from 'react';
import { Maximize2 } from 'lucide-react';
import { Markmap } from 'markmap-view';
import { outlineIsEmpty, parseOutline, type OutlineNode } from '../lib/outline';

interface Props { markdown: string; title: string; onJump(line: number): void }

/** Markmap-style mind map of ONE note: headings and bullets as a collapsible tree. Click a branch to jump to it in the editor. */
export function MarkmapPanel({ markdown, title, onJump }: Props) {
  const svg = useRef<SVGSVGElement>(null);
  const mm = useRef<Markmap | null>(null);
  const first = useRef(true);
  const touched = useRef(false);
  const root = useMemo(() => parseOutline(markdown, title), [markdown, title]);
  const empty = outlineIsEmpty(root);

  useEffect(() => {
    const el = svg.current;
    if (!el) return;
    if (!mm.current) mm.current = Markmap.create(el, { duration: 250, maxWidth: 260, initialExpandLevel: 4, autoFit: false, zoom: true, pan: true });
    const data = root as unknown as Parameters<Markmap['setData']>[0];
    void mm.current.setData(data).then(() => { if (first.current) { first.current = false; void mm.current?.fit(); setTimeout(() => { if (!touched.current) void mm.current?.fit(); }, 400); } });
  }, [root]);
  useEffect(() => () => { mm.current?.destroy(); mm.current = null; first.current = true; }, []);
  // keep the whole map in view while the panel is laid out / resized, until the user pans or zooms it themselves
  useEffect(() => {
    const el = svg.current?.parentElement;
    if (!el) return;
    let t: ReturnType<typeof setTimeout> | undefined;
    const ro = new ResizeObserver(() => { clearTimeout(t); t = setTimeout(() => { if (!touched.current) void mm.current?.fit(); }, 80); });
    ro.observe(el);
    return () => { ro.disconnect(); clearTimeout(t); };
  }, []);

  const click = (e: React.MouseEvent) => {
    const t = e.target as Element;
    if (t.closest('circle')) return; // the fold / unfold dot
    const g = t.closest('g.markmap-node') as (Element & { __data__?: OutlineNode }) | null;
    const payload = g?.__data__?.payload as OutlineNode['payload'] | undefined;
    if (payload) onJump(payload.line);
  };

  return (
    <div className="markmap-panel" data-testid="markmap-panel" data-nodes={empty ? 0 : count(root)}>
      <button className="ghost fit" title="Fit to panel" onClick={() => { touched.current = false; void mm.current?.fit(); }}><Maximize2 size={14} /></button>
      {empty && <p className="muted markmap-empty">Add headings (<code>## Topic</code>) or bullets (<code>- point</code>) and they appear here as a mind map.</p>}
      <svg ref={svg} onClick={click} onWheel={() => { touched.current = true; }} onPointerDown={() => { touched.current = true; }} data-testid="markmap-svg" />
    </div>
  );
}

const count = (n: OutlineNode): number => 1 + n.children.reduce((a, c) => a + count(c), 0);
