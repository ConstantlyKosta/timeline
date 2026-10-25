import { useEffect, useRef, useState, type ReactNode } from 'react';

/** Fixed-row-height windowed list: only the rows in view (plus overscan) are in the DOM, so thousands of notes stay smooth. */
export function visibleRange(scrollTop: number, viewport: number, rowH: number, count: number, overscan = 6) {
  const first = Math.max(0, Math.floor(scrollTop / rowH) - overscan);
  const last = Math.min(count, Math.ceil((scrollTop + viewport) / rowH) + overscan);
  return { first, last };
}

export function VirtualList<T>({ items, rowHeight, render, keyOf, className, testId }: { items: T[]; rowHeight: number; render: (item: T, index: number) => ReactNode; keyOf: (item: T) => string; className?: string; testId?: string }) {
  const ref = useRef<HTMLDivElement>(null);
  const [st, setSt] = useState({ top: 0, h: 600 });
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const measure = () => setSt({ top: el.scrollTop, h: el.clientHeight || 600 });
    measure();
    const ro = typeof ResizeObserver !== 'undefined' ? new ResizeObserver(measure) : null;
    ro?.observe(el);
    return () => ro?.disconnect();
  }, []);
  const { first, last } = visibleRange(st.top, st.h, rowHeight, items.length);
  return (
    <div ref={ref} className={`vlist ${className ?? ''}`} data-testid={testId} data-total={items.length} onScroll={(e) => setSt({ top: e.currentTarget.scrollTop, h: e.currentTarget.clientHeight || 600 })}>
      <div style={{ height: items.length * rowHeight, position: 'relative' }}>
        {items.slice(first, last).map((it, i) => (
          <div key={keyOf(it)} style={{ position: 'absolute', top: (first + i) * rowHeight, left: 0, right: 0, height: rowHeight }}>{render(it, first + i)}</div>
        ))}
      </div>
    </div>
  );
}
