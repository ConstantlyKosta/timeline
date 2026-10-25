import { describe, expect, it } from 'vitest';
import { visibleRange } from './VirtualList';

describe('visibleRange', () => {
  it('windows with overscan and clamps', () => {
    expect(visibleRange(0, 400, 80, 1000, 2)).toEqual({ first: 0, last: 7 });
    expect(visibleRange(8000, 400, 80, 1000, 2)).toEqual({ first: 98, last: 107 });
    expect(visibleRange(79000, 400, 80, 1000)).toEqual({ first: 981, last: 999 });
    expect(visibleRange(0, 400, 80, 3)).toEqual({ first: 0, last: 3 });
  });
});
