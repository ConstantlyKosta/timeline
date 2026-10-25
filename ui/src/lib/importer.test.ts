import { describe, expect, it } from 'vitest';
import { classifyPaths, emptySummary, extOf, mergeSummary } from './importer';

describe('importer helpers', () => {
  it('extOf handles dots in folders', () => {
    expect(extOf('/a.b/c')).toBe('');
    expect(extOf('C:\\x.y\\n.MD')).toBe('md');
    expect(extOf('a.tar.gz')).toBe('gz');
  });
  it('classifies dropped native paths', () => {
    const c = classifyPaths(['/v/Notes', '/v/a.md', '/v/b.MARKDOWN', '/v/r.m4a', '/v/p.jpg', '/v/x.pdf']);
    expect(c.notes).toEqual(['/v/Notes', '/v/a.md', '/v/b.MARKDOWN']);
    expect(c.media).toEqual(['/v/r.m4a', '/v/p.jpg']);
    expect(c.other).toEqual(['/v/x.pdf']);
  });
  it('merges summaries', () => {
    const a = { ...emptySummary(), imported: 2, vaults: ['v'], firstNoteId: 'a', folders: 3 };
    const b = { ...emptySummary(), imported: 5, vaults: ['v', 'w'], firstNoteId: 'b', folders: 2, errorCount: 1, errors: [{ path: 'x', reason: 'y' }] };
    const m = mergeSummary(a, b);
    expect(m).toMatchObject({ imported: 7, vaults: ['v', 'w'], firstNoteId: 'a', folders: 3, errorCount: 1 });
  });
});
