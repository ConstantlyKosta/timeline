import { shapeSvg } from '../lib/shapeIcons';
import type { Legend } from '../lib/graphModel';

/** Key for colours, shapes, link types and groups currently on screen. */
export function Legend({ legend }: { legend: Legend }) {
  const empty = !legend.colors.length && !legend.shapes.length && !legend.links.length && !legend.groups.length;
  if (empty) return null;
  return (
    <div className="graph-legend" data-testid="graph-legend">
      {legend.colors.map((c) => {
        const sh = legend.shapes.find((s) => s.label === c.label); // same label: show the shape in the colour
        return <span key={c.label} title={`${c.count} nodes`}>{sh && sh.shape !== 'circle'
          ? <i className="shp" dangerouslySetInnerHTML={{ __html: shapeSvg(sh.shape, c.color, 12) }} />
          : <i className="dot" style={{ background: c.color }} />}{c.label}</span>;
      })}
      {legend.shapes.filter((s) => !legend.colors.some((c) => c.label === s.label)).map((s) => <span key={s.label}><i className="shp" dangerouslySetInnerHTML={{ __html: shapeSvg(s.shape, 'currentColor', 12) }} />{s.label}</span>)}
      {legend.groups.map((g) => <span key={g.label} title={`${g.count} nodes`}><i className="ring" style={{ borderColor: g.color }} />{g.label}</span>)}
      {legend.links.map((l) => <span key={l.kind} title={`${l.count} links`}><i className="line" style={{ background: l.color }} />{l.label}</span>)}
    </div>
  );
}
