import { useMemo } from 'react';
import { Background, Controls, MarkerType, ReactFlow, Position, getStraightPath, useInternalNode, type Edge, type EdgeProps, type InternalNode, type Node } from '@xyflow/react';
import '@xyflow/react/dist/style.css';
import { useEffect, useState } from 'react';
import { cmd } from '../lib/api';
import { useStore } from '../lib/store';
import type { Graph } from '../lib/types';

// Floating edge: a straight line between the node borders, pointing at the other node's centre
// (pattern from the React Flow docs examples, MIT).
function borderPoint(a: InternalNode, b: InternalNode) {
  const aw = (a.measured.width ?? 100) / 2, ah = (a.measured.height ?? 30) / 2;
  const ac = { x: a.internals.positionAbsolute.x + aw, y: a.internals.positionAbsolute.y + ah };
  const bc = { x: b.internals.positionAbsolute.x + (b.measured.width ?? 100) / 2, y: b.internals.positionAbsolute.y + (b.measured.height ?? 30) / 2 };
  const dx = bc.x - ac.x, dy = bc.y - ac.y;
  if (dx === 0 && dy === 0) return ac;
  const k = 1 / Math.max(Math.abs(dx) / aw, Math.abs(dy) / ah);
  return { x: ac.x + dx * k, y: ac.y + dy * k };
}
function FloatingEdge({ id, source, target, markerEnd, style, data }: EdgeProps) {
  const s = useInternalNode(source), t = useInternalNode(target);
  if (!s || !t || !s.measured.width || !t.measured.width) return null;
  const p1 = borderPoint(s, t), p2 = borderPoint(t, s);
  const [path] = getStraightPath({ sourceX: p1.x, sourceY: p1.y, targetX: p2.x, targetY: p2.y });
  return <path id={id} className={`react-flow__edge-path ${(data as { kind?: string } | undefined)?.kind ?? ''}`} d={path} markerEnd={markerEnd as string | undefined} style={style} />;
}
const edgeTypes = { floating: FloatingEdge };
void Position;

export function GraphView() {
  const { openNote, openByTitle, fail, notes, theme } = useStore();
  const [graph, setGraph] = useState<Graph | null>(null);
  useEffect(() => { cmd.graph().then(setGraph).catch(fail); }, [notes, fail]);

  const { nodes, edges } = useMemo(() => {
    if (!graph) return { nodes: [] as Node[], edges: [] as Edge[] };
    const nodes: Node[] = graph.nodes.map((n) => {
      const size = Math.min(150, 90 + n.degree * 8);
      return {
        id: n.id, position: { x: n.x * 1.2, y: n.y * 1.2 }, data: { label: n.title },
        className: `gnode ${n.isGhost ? 'ghost' : ''}`,
        style: { width: size },
      };
    });
    const edges: Edge[] = graph.edges.map((e, i) => ({
      id: `${e.from}-${e.to}-${i}`, source: e.from, target: e.to, type: 'floating', data: { kind: e.kind }, className: `gedge ${e.kind}`,
      markerEnd: e.kind === 'link' ? { type: MarkerType.ArrowClosed } : undefined,
      style: e.kind === 'tag' ? { strokeDasharray: '4 4', opacity: 0.45 } : undefined,
    }));
    return { nodes, edges };
  }, [graph]);

  return (
    <div className="graph" data-testid="graph-view" data-node-count={nodes.length} data-edge-count={edges.length}>
      <div className="graph-legend">
        <span><i className="solid" /> wiki link</span><span><i className="dashed" /> shared tag</span><span><i className="ghostdot" /> not created yet</span>
        <span className="muted">Click a node to open it</span>
      </div>
      {nodes.length === 0 ? <div className="empty">No notes to map yet.</div> : (
        <ReactFlow
          nodes={nodes} edges={edges} edgeTypes={edgeTypes} fitView fitViewOptions={{ padding: 0.15 }} minZoom={0.1} maxZoom={2.5}
          nodesConnectable={false} colorMode={theme} proOptions={{ hideAttribution: true }}
          onNodeClick={(_e, n) => (n.id.startsWith('ghost:') ? openByTitle(String(n.data.label)).catch(fail) : openNote(n.id))}
        >
          <Background gap={24} />
          <Controls showInteractive={false} />
        </ReactFlow>
      )}
    </div>
  );
}
