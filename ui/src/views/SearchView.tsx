import { useEffect, useState } from 'react';
import { AudioLines, FileText, MessageSquareText, Search } from 'lucide-react';
import { format } from 'date-fns';
import { cmd } from '../lib/api';
import { useStore } from '../lib/store';
import type { SearchHit } from '../lib/types';

const fmtTime = (s: number) => `${Math.floor(s / 60)}:${String(Math.floor(s % 60)).padStart(2, '0')}`;

export function SearchView() {
  const { searchQuery, setSearchQuery, openNote, openMedia, fail, stats } = useStore();
  const [hits, setHits] = useState<SearchHit[]>([]);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    const q = searchQuery.trim();
    if (!q) { setHits([]); return; }
    setBusy(true);
    const h = setTimeout(() => cmd.search(q, 40).then((r) => { setHits(r); setBusy(false); }).catch((e) => { setBusy(false); fail(e); }), 150);
    return () => clearTimeout(h);
  }, [searchQuery, fail]);

  return (
    <div className="search-view" data-testid="search-view">
      <div className="search-box">
        <Search size={18} />
        <input autoFocus value={searchQuery} onChange={(e) => setSearchQuery(e.target.value)} placeholder="Search by meaning or words — notes, recordings, transcripts" data-testid="search-input" />
      </div>
      <p className="muted small">Hybrid ranking: keyword match + semantic similarity ({stats?.embedding ?? '…'}). Try “feeling anxious before presentations”.</p>
      <ul className="hits">
        {hits.map((h) => (
          <li key={h.id}>
            <button className="hit" data-testid="search-hit" onClick={() => (h.kind === 'note' ? openNote(h.ownerId) : openMedia(h.ownerId, h.time))}>
              <span className="hit-ico">{h.kind === 'note' ? <FileText size={16} /> : h.kind === 'recording' ? <AudioLines size={16} /> : <MessageSquareText size={16} />}</span>
              <span className="hit-main">
                <b>{h.title}{h.time != null && <em> @ {fmtTime(h.time)}</em>}</b>
                <span className="snip">{h.snippet}</span>
                <small>{format(new Date(h.date), 'd MMM yyyy')} · {h.kind === 'transcriptSegment' ? 'transcript' : h.kind}</small>
              </span>
              <span className="score" title={`keyword ${h.textScore.toFixed(2)} · semantic ${h.semanticScore.toFixed(2)}`}>
                <i style={{ width: `${Math.min(100, Math.round(h.score * 100))}%` }} />
                <small>{Math.round(h.score * 100)}%</small>
              </span>
            </button>
          </li>
        ))}
        {searchQuery.trim() && !busy && hits.length === 0 && <li className="empty-small">No results.</li>}
      </ul>
    </div>
  );
}
