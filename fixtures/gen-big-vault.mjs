// Generates a larger, deterministic demo vault (about 130 notes, tags, properties, wikilinks, ghosts and pictures)
// used for the mind-map screenshots and e2e tests:  node fixtures/gen-big-vault.mjs <outDir>
import fs from 'node:fs';
import path from 'node:path';
import zlib from 'node:zlib';

const out = process.argv[2] ?? 'big-vault';
let seed = 42;
const rnd = () => ((seed = (seed * 1664525 + 1013904223) % 4294967296) / 4294967296);
const pick = (a) => a[Math.floor(rnd() * a.length)];
const picks = (a, n) => [...a].sort(() => rnd() - 0.5).slice(0, n);

// tiny PNG writer (solid colour with a gradient) so no image library is needed
const crcT = new Int32Array(256).map((_, n) => { let c = n; for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; return c; });
const crc = (b) => { let c = -1; for (const x of b) c = crcT[(c ^ x) & 255] ^ (c >>> 8); return (c ^ -1) >>> 0; };
const chunk = (t, d) => { const l = Buffer.alloc(4); l.writeUInt32BE(d.length); const td = Buffer.concat([Buffer.from(t), d]); const c = Buffer.alloc(4); c.writeUInt32BE(crc(td)); return Buffer.concat([l, td, c]); };
function png(r, g, b, n) {
  const W = 96, H = 96, raw = Buffer.alloc((W * 3 + 1) * H);
  for (let y = 0; y < H; y++) { raw[y * (W * 3 + 1)] = 0; for (let x = 0; x < W; x++) { const o = y * (W * 3 + 1) + 1 + x * 3, t = (x + y + n * 7) % 48 < 24 ? 1 : 0.78; raw[o] = r * t; raw[o + 1] = g * t; raw[o + 2] = b * t; } }
  const ih = Buffer.alloc(13); ih.writeUInt32BE(W, 0); ih.writeUInt32BE(H, 4); ih[8] = 8; ih[9] = 2;
  return Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), chunk('IHDR', ih), chunk('IDAT', zlib.deflateSync(raw)), chunk('IEND', Buffer.alloc(0))]);
}
const write = (rel, text) => { const p = path.join(out, rel); fs.mkdirSync(path.dirname(p), { recursive: true }); fs.writeFileSync(p, text); };

fs.rmSync(out, { recursive: true, force: true });
const colors = [[230, 120, 70], [80, 140, 230], [70, 180, 130], [190, 90, 210], [230, 190, 60], [60, 180, 200]];
for (let i = 0; i < 10; i++) { fs.mkdirSync(path.join(out, 'assets'), { recursive: true }); fs.writeFileSync(path.join(out, 'assets', `pic-${i}.png`), png(...colors[i % colors.length], i)); }

const projects = ['Aurora', 'Beacon', 'Compass', 'Delta'];
const people = ['Alice Chen', 'Boris Ivanov', 'Carla Gomez', 'Dmitri Volkov', 'Elena Rossi', 'Farid Khan', 'Grace Lee', 'Hugo Martin', 'Ines Silva', 'Jonas Berg', 'Katya Orlova', 'Liam Walsh'];
const books = ['Deep Work', 'Thinking in Systems', 'The Pragmatic Programmer', 'Gödel, Escher, Bach', 'Range', 'Atomic Habits', 'Four Thousand Weeks', 'The Mythical Man-Month', 'Designing Data-Intensive Applications', 'How to Take Smart Notes', 'Staff Engineer', 'The Art of Doing Science and Engineering'];
const ideas = ['Local-first sync', 'Spaced repetition', 'Zettelkasten habits', 'Weekly review ritual', 'Offline speech to text', 'Graph layouts', 'Timeline as a UI', 'Attention budget', 'Writing in public', 'Small tools', 'Reading backlog', 'Habit stacking', 'Energy tracking', 'Trip planning checklist', 'Cooking experiments'];
const notes = new Map();
const add = (folder, title, tags, props, body) => notes.set(title, { folder, title, tags, props, body });
const ghosts = ['Quarterly goals', 'Mentor intro', 'Conference talk', 'Budget 2027', 'Home server', 'Sourdough'];

projects.forEach((p, pi) => {
  const subs = ['Kickoff', 'Requirements', 'Design', 'Retro', 'Risks', 'Launch plan', 'Budget'].map((s) => `${p} ${s}`);
  add('Projects', p, ['project', 'work'], { status: pick(['active', 'active', 'someday']), type: 'project', owner: pick(people) }, `# ${p}\n\nHub for the ${p} project.\n\n${subs.map((s) => `- [[${s}]]`).join('\n')}\n\nLead: [[${pick(people)}]]\n\n![[pic-${pi}.png]]`);
  subs.forEach((s, i) => add(`Projects/${p}`, s, ['project', i % 2 ? 'meeting' : 'planning'], { status: pick(['active', 'done', 'done']), type: 'doc' }, `# ${s}\n\nPart of [[${p}]]. ${i ? `Follows [[${subs[i - 1]}]].` : ''}\n\nAttendees: ${picks(people, 2).map((x) => `[[${x}]]`).join(', ')}\n\n- [ ] follow up on ${pick(ideas).toLowerCase()}\n${rnd() < 0.3 ? `\nAlso see [[${pick(ghosts)}]].\n` : ''}`));
});
people.forEach((n, i) => add('People', n, ['person'], { type: 'person', company: pick(['Acme', 'Globex', 'Initech', 'Umbrella']) }, `# ${n}\n\nWorks on [[${pick(projects)}]]. Also knows [[${pick(people.filter((x) => x !== n))}]].\n${i < 4 ? `\n![[pic-${(i + 4) % 10}.png]]\n` : ''}`));
books.forEach((b, i) => add('Reading', b, ['book', pick(['learning', 'craft', 'systems'])], { type: 'book', rating: String(3 + Math.floor(rnd() * 3)), status: pick(['done', 'reading', 'done']) }, `# ${b}\n\nNotes on *${b}*. Inspired [[${pick(ideas)}]] and [[${pick(ideas)}]].\n${i % 4 === 0 ? `\n![[pic-${i % 10}.png]]\n` : ''}`));
ideas.forEach((n, i) => add('Ideas', n, ['idea', pick(['tools', 'habits', 'writing', 'tools'])], { type: 'idea', status: pick(['seed', 'growing', 'seed']) }, `# ${n}\n\nSee also [[${pick(ideas.filter((x) => x !== n))}]]. Related book: [[${pick(books)}]].\n${rnd() < 0.35 ? `\nBlocked on [[${pick(ghosts)}]].\n` : ''}`));
for (let d = 1; d <= 36; d++) {
  const day = `2026-09-${String(d % 28 + 1).padStart(2, '0')}`;
  const title = `${day} Journal${d > 28 ? ' (evening)' : ''}`;
  add('Journal', title, ['journal', pick(['health', 'travel', 'work', 'family', 'work'])], { type: 'journal', mood: pick(['good', 'ok', 'tired']), date: day },
    `# ${title}\n\nWorked on [[${pick(projects)}]] with [[${pick(people)}]]. Thought about [[${pick(ideas)}]].\n${rnd() < 0.4 ? `\nRead a bit of [[${pick(books)}]].\n` : ''}${rnd() < 0.15 ? `\n![[pic-${d % 10}.png]]\n` : ''}`);
}
// a few orphans and a map of content
add('', 'Index', ['moc'], { type: 'moc' }, `# Index\n\n${['Projects', 'People', 'Reading', 'Ideas'].map((x) => `## ${x}\n${(x === 'Projects' ? projects : x === 'People' ? people.slice(0, 4) : x === 'Reading' ? books.slice(0, 4) : ideas.slice(0, 4)).map((t) => `- [[${t}]]`).join('\n')}`).join('\n\n')}\n`);
['Shopping list', 'Passwords reminder', 'Random thought'].forEach((t) => add('Inbox', t, [], {}, `# ${t}\n\nNothing links here.`));

for (const n of notes.values()) {
  const fm = `---\ntags: [${n.tags.join(', ')}]\n${Object.entries(n.props).map(([k, v]) => `${k}: ${v}`).join('\n')}\n---\n\n`;
  write(path.join(n.folder, `${n.title}.md`), (n.tags.length || Object.keys(n.props).length ? fm : '') + n.body + (n.tags.length ? `\n\n${n.tags.map((t) => `#${t}`).join(' ')}\n` : '\n'));
}
console.log(`wrote ${notes.size} notes to ${out}`);
