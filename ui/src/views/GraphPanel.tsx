import { useState, type ReactNode } from 'react';
import { Plus, RotateCcw, Save, Trash2, X } from 'lucide-react';
import { PRESETS, SHAPES, defaultLinkColor, linkKindLabel, PALETTE, type ColorBy, type Display, type GraphSettings, type LabelMode, type ShapeBy, type SizeBy, type Group } from '../lib/graphModel';
import type { GraphViewRow } from '../lib/types';
import { useStore } from '../lib/store';

type Facets = { tags: [string, number][]; folders: [string, number][]; linkKinds: string[]; propertyKeys: string[] };
interface Props {
  settings: GraphSettings; patch: (p: Partial<GraphSettings>) => void; setForces: (p: Partial<GraphSettings['forces']>) => void;
  facets: Facets; views: GraphViewRow[]; display: Display;
  onApplyView: (json: unknown) => void; onSaveView: (name: string) => Promise<void>; onDeleteView: (name: string) => Promise<void>;
  onResetLayout: () => void; onResetSettings: () => void;
}

function Section({ title, children, open = false, id }: { title: string; children: ReactNode; open?: boolean; id: string }) {
  return <details className="gp-sec" open={open} data-testid={`gp-${id}`}><summary>{title}</summary><div className="gp-body">{children}</div></details>;
}
function Row({ label, children }: { label: string; children: ReactNode }) { return <label className="gp-row"><span>{label}</span>{children}</label>; }
function Slider({ label, value, min, max, step, onChange, id }: { label: string; value: number; min: number; max: number; step: number; onChange: (v: number) => void; id: string }) {
  return (
    <label className="gp-row gp-slider"><span>{label}</span>
      <input type="range" data-testid={`gp-${id}`} min={min} max={max} step={step} value={value} onChange={(e) => onChange(Number(e.target.value))} />
      <em>{Number.isInteger(step) ? value : value.toFixed(2)}</em>
    </label>
  );
}
function Check({ label, checked, onChange, id }: { label: string; checked: boolean; onChange: (v: boolean) => void; id?: string }) {
  return <label className="gp-check"><input type="checkbox" data-testid={id} checked={checked} onChange={(e) => onChange(e.target.checked)} />{label}</label>;
}
function Chips({ items, onRemove }: { items: string[]; onRemove: (s: string) => void }) {
  return <div className="gp-chips">{items.map((t) => <span key={t} className="chip">{t}<button className="ghost" aria-label={`Remove ${t}`} onClick={() => onRemove(t)}><X size={11} /></button></span>)}</div>;
}
function Pick({ placeholder, options, onPick, id }: { placeholder: string; options: [string, number][]; onPick: (v: string) => void; id?: string }) {
  return (
    <select data-testid={id} value="" onChange={(e) => e.target.value && onPick(e.target.value)}>
      <option value="">{placeholder}</option>
      {options.map(([v, n]) => <option key={v} value={v}>{v} ({n})</option>)}
    </select>
  );
}

export function GraphPanel({ settings: s, patch, setForces, facets, views, display, onApplyView, onSaveView, onDeleteView, onResetLayout, onResetSettings }: Props) {
  const theme = useStore((st) => st.theme);
  const [viewName, setViewName] = useState('');
  const [busy, setBusy] = useState(false);
  const add = (key: 'tagsInclude' | 'tagsExclude' | 'folders', v: string) => { if (!s[key].includes(v)) patch({ [key]: [...s[key], v] }); };
  const drop = (key: 'tagsInclude' | 'tagsExclude' | 'folders', v: string) => patch({ [key]: s[key].filter((x) => x !== v) });
  const newGroup = () => patch({ groups: [...s.groups, { id: `g${Date.now()}`, name: `Group ${s.groups.length + 1}`, query: '', color: PALETTE[(s.groups.length + 3) % PALETTE.length] }] });
  const setGroup = (id: string, p: Partial<Group>) => patch({ groups: s.groups.map((g) => (g.id === id ? { ...g, ...p } : g)) });
  const doSave = async () => { const n = viewName.trim(); if (!n) return; setBusy(true); try { await onSaveView(n); setViewName(''); } finally { setBusy(false); } };

  return (
    <aside className="graph-panel" data-testid="graph-panel">
      <Section id="views" title="Views" open>
        <div className="gp-presets">
          {PRESETS.map((p) => <button key={p.name} className="ghost" data-testid={`preset-${p.name}`} onClick={() => onApplyView({ ...p.settings, pinned: s.pinned })}>{p.name}</button>)}
        </div>
        {views.length > 0 && <div className="gp-saved">
          {views.map((v) => (
            <div key={v.name} className="gp-saved-row">
              <button className="link" data-testid={`view-${v.name}`} onClick={() => onApplyView(v.json)}>{v.name}</button>
              <button className="ghost" aria-label={`Delete view ${v.name}`} onClick={() => onDeleteView(v.name)}><Trash2 size={13} /></button>
            </div>
          ))}
        </div>}
        <div className="gp-save">
          <input data-testid="view-name" placeholder="Name this view…" value={viewName} onChange={(e) => setViewName(e.target.value)} onKeyDown={(e) => e.key === 'Enter' && doSave()} />
          <button className="primary" data-testid="view-save" disabled={!viewName.trim() || busy} onClick={doSave}><Save size={14} /> Save</button>
        </div>
      </Section>

      <Section id="filters" title="Filters">
        <Row label="Only tags"><Pick id="gp-tag-in" placeholder="add tag…" options={facets.tags} onPick={(v) => add('tagsInclude', v)} /></Row>
        <Chips items={s.tagsInclude} onRemove={(v) => drop('tagsInclude', v)} />
        <Row label="Not tags"><Pick placeholder="exclude tag…" options={facets.tags} onPick={(v) => add('tagsExclude', v)} /></Row>
        <Chips items={s.tagsExclude} onRemove={(v) => drop('tagsExclude', v)} />
        <Row label="Folders"><Pick id="gp-folder" placeholder="add folder…" options={facets.folders} onPick={(v) => add('folders', v)} /></Row>
        <Chips items={s.folders} onRemove={(v) => drop('folders', v)} />
        <Row label="Property">
          <span className="gp-pair">
            <select value={s.propKey} onChange={(e) => patch({ propKey: e.target.value })}><option value="">any</option>{facets.propertyKeys.map((k) => <option key={k}>{k}</option>)}</select>
            <input placeholder="value" value={s.propValue} disabled={!s.propKey} onChange={(e) => patch({ propValue: e.target.value })} />
          </span>
        </Row>
        <div className="gp-checks">
          <Check label="Orphans" checked={s.showOrphans} onChange={(v) => patch({ showOrphans: v })} id="gp-orphans" />
          <Check label="Unresolved" checked={s.showUnresolved} onChange={(v) => patch({ showUnresolved: v })} id="gp-unresolved" />
          <Check label="Attachments" checked={s.showAttachments} onChange={(v) => patch({ showAttachments: v })} id="gp-attachments" />
          <Check label="Tag nodes" checked={s.showTagNodes} onChange={(v) => patch({ showTagNodes: v })} id="gp-tagnodes" />
          <Check label="Folder nodes" checked={s.showFolderNodes} onChange={(v) => patch({ showFolderNodes: v })} id="gp-foldernodes" />
        </div>
      </Section>

      <Section id="display" title="Appearance">
        <Row label="Colour by">
          <select data-testid="gp-colorby" value={s.colorBy} onChange={(e) => patch({ colorBy: e.target.value as ColorBy })}>
            <option value="kind">Type</option><option value="tag">First tag</option><option value="folder">Folder</option><option value="property">Property</option><option value="none">None</option>
          </select>
        </Row>
        {s.colorBy === 'property' && <Row label="Property"><select value={s.colorProp} onChange={(e) => patch({ colorProp: e.target.value })}><option value="">—</option>{facets.propertyKeys.map((k) => <option key={k}>{k}</option>)}</select></Row>}
        <Row label="Shape by">
          <select data-testid="gp-shapeby" value={s.shapeBy} onChange={(e) => patch({ shapeBy: e.target.value as ShapeBy })}>
            <option value="kind">Type</option><option value="tag">First tag</option><option value="folder">Folder</option><option value="none">Circles</option>
          </select>
        </Row>
        <Row label="Size by">
          <select data-testid="gp-sizeby" value={s.sizeBy} onChange={(e) => patch({ sizeBy: e.target.value as SizeBy })}>
            <option value="degree">Links</option><option value="in">Incoming links</option><option value="out">Outgoing links</option><option value="pagerank">Importance (PageRank)</option>
            <option value="words">Length</option><option value="recent">Recency</option><option value="uniform">Same size</option>
          </select>
        </Row>
        <Slider id="size" label="Node size" value={s.sizeScale} min={0.4} max={3} step={0.1} onChange={(v) => patch({ sizeScale: v })} />
        <Slider id="edge" label="Link width" value={s.edgeThickness} min={0.4} max={4} step={0.1} onChange={(v) => patch({ edgeThickness: v })} />
        <Row label="Labels"><select value={s.labels} onChange={(e) => patch({ labels: e.target.value as LabelMode })}><option value="auto">When zoomed</option><option value="always">Always</option><option value="never">Never</option></select></Row>
        <div className="gp-checks">
          <Check label="Arrows" checked={s.showArrows} onChange={(v) => patch({ showArrows: v })} id="gp-arrows" />
          <Check label="Curved links" checked={s.curved} onChange={(v) => patch({ curved: v })} id="gp-curved" />
          <Check label="Images" checked={s.showImages} onChange={(v) => patch({ showImages: v })} id="gp-images" />
        </div>
      </Section>

      <Section id="links" title="Link types">
        {display.legend.links.length === 0 && <div className="muted">No links on screen.</div>}
        {facets.linkKinds.map((k) => {
          const hidden = s.hiddenLinkKinds.includes(k);
          const color = s.linkColors[k] ?? defaultLinkColor(k, theme);
          return (
            <div key={k} className="gp-link-row">
              <Check label={linkKindLabel(k)} checked={!hidden} id={`gp-link-${k}`} onChange={(v) => patch({ hiddenLinkKinds: v ? s.hiddenLinkKinds.filter((x) => x !== k) : [...s.hiddenLinkKinds, k] })} />
              <input type="color" value={color.length === 7 ? color : '#888888'} aria-label={`${k} colour`} onChange={(e) => patch({ linkColors: { ...s.linkColors, [k]: e.target.value } })} />
            </div>
          );
        })}
      </Section>

      <Section id="groups" title="Groups">
        <div className="muted gp-hint">Highlight notes matching a search with a colour. Same syntax as the search box.</div>
        {s.groups.map((g) => (
          <div key={g.id} className="gp-group">
            <input type="color" value={g.color} aria-label="Group colour" onChange={(e) => setGroup(g.id, { color: e.target.value })} />
            <input className="grow" placeholder="tag:work folder:Projects…" value={g.query} data-testid="gp-group-query" onChange={(e) => setGroup(g.id, { query: e.target.value })} />
            <input className="gname" placeholder="name" value={g.name} onChange={(e) => setGroup(g.id, { name: e.target.value })} />
            <button className="ghost" aria-label="Remove group" onClick={() => patch({ groups: s.groups.filter((x) => x.id !== g.id) })}><X size={13} /></button>
          </div>
        ))}
        <button className="ghost" data-testid="gp-add-group" onClick={newGroup}><Plus size={14} /> Add group</button>
      </Section>

      <Section id="forces" title="Forces" open>
        <Slider id="center" label="Centre" value={s.forces.center} min={0} max={0.3} step={0.01} onChange={(v) => setForces({ center: v })} />
        <Slider id="repel" label="Repel" value={s.forces.repel} min={0} max={300} step={5} onChange={(v) => setForces({ repel: v })} />
        <Slider id="link" label="Link force" value={s.forces.link} min={0} max={1} step={0.05} onChange={(v) => setForces({ link: v })} />
        <Slider id="distance" label="Link distance" value={s.forces.distance} min={5} max={200} step={5} onChange={(v) => setForces({ distance: v })} />
        <Slider id="collide" label="No overlap" value={s.forces.collide} min={0} max={3} step={0.1} onChange={(v) => setForces({ collide: v })} />
        <div className="gp-actions">
          <button className="ghost" data-testid="gp-relayout" onClick={onResetLayout}><RotateCcw size={13} /> Re-layout</button>
          <button className="ghost" onClick={onResetSettings}>Reset settings</button>
        </div>
      </Section>
      <div className="muted gp-foot">{SHAPES.length} shapes · drag to pin · double-click to toggle pin · right-click for more</div>
    </aside>
  );
}
