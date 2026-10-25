import { useState } from 'react';
import { Command } from 'cmdk';
import { AudioLines, CalendarClock, FileText, Moon, Network, Plus, Search as SearchIcon, StickyNote, Sun, Tag } from 'lucide-react';
import { useStore } from '../lib/store';

export function QuickSwitcher() {
  const { switcherOpen, setSwitcher, notes, tags, media, openNote, openMedia, newNote, setView, setSearchQuery, theme, setTheme, fail } = useStore();
  const [q, setQ] = useState('');
  const close = () => { setSwitcher(false); setQ(''); };
  const run = (fn: () => void) => () => { close(); fn(); };
  const trimmed = q.trim();
  const exact = notes.some((n) => n.title.toLowerCase() === trimmed.toLowerCase());

  return (
    <Command.Dialog open={switcherOpen} onOpenChange={(o) => (o ? setSwitcher(true) : close())} label="Quick switcher" className="switcher" overlayClassName="switcher-overlay" contentClassName="switcher-content">
      <Command.Input value={q} onValueChange={setQ} placeholder="Jump to a note, recording, tag or action…" data-testid="switcher-input" />
      <Command.List>
        <Command.Empty>Nothing found.</Command.Empty>
        {trimmed && !exact && (
          <Command.Group heading="Create">
            <Command.Item value={`create ${trimmed}`} onSelect={run(() => newNote(`# ${trimmed}\n\n`).catch(fail))}><Plus size={15} /> Create note “{trimmed}”</Command.Item>
            <Command.Item value={`search ${trimmed}`} onSelect={run(() => { setSearchQuery(trimmed); setView('search'); })}><SearchIcon size={15} /> Search everything for “{trimmed}”</Command.Item>
          </Command.Group>
        )}
        <Command.Group heading="Notes">
          {notes.slice(0, 200).map((n) => (
            <Command.Item key={n.id} value={`${n.title} ${n.snippet} ${n.id}`} onSelect={run(() => openNote(n.id))}><FileText size={15} /> {n.title || 'Untitled'}</Command.Item>
          ))}
        </Command.Group>
        <Command.Group heading="Recordings & media">
          {media.slice(0, 100).map((m) => (
            <Command.Item key={m.id} value={`${m.title} media ${m.id}`} onSelect={run(() => openMedia(m.id))}><AudioLines size={15} /> {m.title}</Command.Item>
          ))}
        </Command.Group>
        {tags.length > 0 && (
          <Command.Group heading="Tags">
            {tags.map((t) => <Command.Item key={t.tag} value={`tag #${t.tag}`} onSelect={run(() => { setSearchQuery('#' + t.tag); setView('search'); })}><Tag size={15} /> #{t.tag} <small>{t.count}</small></Command.Item>)}
          </Command.Group>
        )}
        <Command.Group heading="Go to">
          <Command.Item value="go timeline" onSelect={run(() => setView('timeline'))}><CalendarClock size={15} /> Timeline</Command.Item>
          <Command.Item value="go notes" onSelect={run(() => setView('notes'))}><StickyNote size={15} /> Notes</Command.Item>
          <Command.Item value="go mind map graph" onSelect={run(() => setView('graph'))}><Network size={15} /> Mind map</Command.Item>
          <Command.Item value="go search" onSelect={run(() => setView('search'))}><SearchIcon size={15} /> Search</Command.Item>
          <Command.Item value="go library recordings" onSelect={run(() => setView('library'))}><AudioLines size={15} /> Library</Command.Item>
          <Command.Item value="toggle theme dark light" onSelect={run(() => setTheme(theme === 'dark' ? 'light' : 'dark'))}>{theme === 'dark' ? <Sun size={15} /> : <Moon size={15} />} Toggle theme</Command.Item>
        </Command.Group>
      </Command.List>
    </Command.Dialog>
  );
}
