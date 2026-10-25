// Live force simulation for the mind map (d3-force, ISC). Owns its own node/link copies and reports
// positions through `onTick`; pinned nodes are held with fx/fy.
import { forceCenter, forceCollide, forceLink, forceManyBody, forceSimulation, forceX, forceY, type Simulation, type SimulationLinkDatum, type SimulationNodeDatum } from 'd3-force';
import type { Forces } from './graphModel';

export interface SimNode extends SimulationNodeDatum { id: string; r: number; pinned: boolean }
type SimLink = SimulationLinkDatum<SimNode>;

export class ForceSim {
  private sim: Simulation<SimNode, SimLink>;
  private nodes: SimNode[] = [];
  private byId = new Map<string, SimNode>();
  private forces: Forces;
  constructor(forces: Forces, private onTick: (nodes: SimNode[]) => void, private onEnd: (nodes: SimNode[]) => void) {
    this.forces = forces;
    this.sim = forceSimulation<SimNode>([]).alphaDecay(0.035).velocityDecay(0.4).stop();
    this.sim
      .force('link', forceLink<SimNode, SimLink>([]).id((d) => d.id))
      .force('charge', forceManyBody<SimNode>().distanceMax(600))
      .force('x', forceX<SimNode>(0))
      .force('y', forceY<SimNode>(0))
      .force('collide', forceCollide<SimNode>())
      .force('center', forceCenter(0, 0).strength(0.02));
    this.apply();
    this.sim.on('tick', () => this.onTick(this.nodes));
    this.sim.on('end', () => this.onEnd(this.nodes));
  }
  private apply() {
    const f = this.forces;
    const link = this.sim.force('link') as ReturnType<typeof forceLink<SimNode, SimLink>> | undefined;
    link?.distance(f.distance).strength(f.link);
    (this.sim.force('charge') as ReturnType<typeof forceManyBody<SimNode>>).strength(-f.repel);
    (this.sim.force('x') as ReturnType<typeof forceX<SimNode>>).strength(f.center);
    (this.sim.force('y') as ReturnType<typeof forceY<SimNode>>).strength(f.center);
    (this.sim.force('collide') as ReturnType<typeof forceCollide<SimNode>>).radius((d) => d.r * 0.5 * f.collide);
  }
  /** Replace the simulated graph. `pos` supplies known positions; nodes without one are seeded. */
  setData(nodes: { id: string; r: number; pinned: boolean }[], links: { from: string; to: string }[], pos: Map<string, { x: number; y: number }>, reheat: number) {
    const old = this.byId;
    this.nodes = nodes.map((n, i) => {
      const prev = old.get(n.id);
      const p = pos.get(n.id) ?? (prev && prev.x !== undefined ? { x: prev.x!, y: prev.y! } : undefined);
      const a = i * 2.399963; // golden angle spiral for fresh nodes
      const rad = 14 * Math.sqrt(i + 1);
      const s: SimNode = { id: n.id, r: n.r, pinned: n.pinned, x: p ? p.x : Math.cos(a) * rad, y: p ? p.y : Math.sin(a) * rad };
      if (n.pinned && p) { s.fx = p.x; s.fy = p.y; }
      return s;
    });
    this.byId = new Map(this.nodes.map((n) => [n.id, n]));
    const ls: SimLink[] = links.filter((l) => this.byId.has(l.from) && this.byId.has(l.to)).map((l) => ({ source: l.from, target: l.to }));
    this.sim.nodes(this.nodes);
    this.sim.force('link', forceLink<SimNode, SimLink>(ls).id((d) => d.id));
    this.apply();
  }
  /** Run `n` ticks synchronously (initial layout); the simulation stays stopped. */
  settle(n: number) {
    this.sim.alpha(1);
    for (let i = 0; i < n; i++) this.sim.tick();
    this.sim.alpha(0);
    this.onTick(this.nodes);
  }
  setForces(f: Forces) { this.forces = f; this.apply(); this.reheat(0.5); }
  reheat(alpha = 0.4) { if (this.nodes.length) this.sim.alpha(alpha).restart(); }
  fix(id: string, x: number, y: number) { const n = this.byId.get(id); if (n) { n.fx = x; n.fy = y; n.x = x; n.y = y; } }
  release(id: string) { const n = this.byId.get(id); if (n) { n.fx = null; n.fy = null; } }
  pin(id: string, x: number, y: number) { const n = this.byId.get(id); if (n) { n.pinned = true; n.fx = x; n.fy = y; } }
  stop() { this.sim.stop(); }
  get running() { return this.sim.alpha() > this.sim.alphaMin(); }
}
