import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'react';

export interface MenuItem { label: string; icon?: ReactNode; danger?: boolean; testId?: string; onSelect: () => void }

/** Small right-click menu positioned at the pointer, kept inside the window; closes on outside click, Escape, scroll or blur. */
export function ContextMenu({ x, y, items, header, onClose }: { x: number; y: number; items: MenuItem[]; header?: string; onClose: () => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ left: x, top: y });
  useLayoutEffect(() => {
    const r = ref.current?.getBoundingClientRect();
    if (r) setPos({ left: Math.max(4, Math.min(x, window.innerWidth - r.width - 4)), top: Math.max(4, Math.min(y, window.innerHeight - r.height - 4)) });
  }, [x, y]);
  useEffect(() => {
    const away = (e: Event) => { if (!ref.current?.contains(e.target as Node)) onClose(); };
    const key = (e: KeyboardEvent) => { if (e.key === 'Escape') onClose(); };
    window.addEventListener('pointerdown', away, true);
    window.addEventListener('keydown', key);
    window.addEventListener('blur', onClose);
    window.addEventListener('wheel', onClose, { passive: true });
    return () => {
      window.removeEventListener('pointerdown', away, true); window.removeEventListener('keydown', key);
      window.removeEventListener('blur', onClose); window.removeEventListener('wheel', onClose);
    };
  }, [onClose]);
  return (
    <div ref={ref} className="ctx-menu" role="menu" style={pos} data-testid="context-menu" onContextMenu={(e) => e.preventDefault()}>
      {header && <div className="ctx-head">{header}</div>}
      {items.map((it) => (
        <button key={it.label} role="menuitem" className={it.danger ? 'danger' : ''} data-testid={it.testId} onClick={() => { onClose(); it.onSelect(); }}>{it.icon}{it.label}</button>
      ))}
    </div>
  );
}
