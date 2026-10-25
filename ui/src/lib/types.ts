// Wire types. These mirror the serde (camelCase) structs in crates/timeline-core.
export type Ms = number;
export type EntryKind = 'recording' | 'note' | 'photo' | 'video';
export type MediaKind = 'audio' | 'video' | 'photo';

export interface TimelineEntry {
  id: string;
  kind: EntryKind;
  start: Ms;
  duration: number; // seconds
  title: string;
  snippet: string;
  tags: string[];
}

export interface Note {
  id: string;
  title: string;
  body: string;
  date: Ms;
  createdAt: Ms;
  updatedAt: Ms;
}
export interface NoteSummary {
  id: string;
  title: string;
  date: Ms;
  updatedAt: Ms;
  snippet: string;
  tags: string[];
  taskCount: number;
  doneCount: number;
}
export interface NoteRef { id: string; title: string }
export interface TagCount { tag: string; count: number }

export interface MediaItem {
  id: string;
  kind: MediaKind;
  title: string;
  recordedAt: Ms;
  duration: number | null;
  sha256: string;
  fileSize: number;
  importedAt: Ms;
  isMissing: boolean;
  hasTranscript: boolean;
  mime: string;
  originalName: string;
}
export interface TranscriptSegment { id: number; start: number; end: number; text: string; speaker: string | null }
export interface Transcript { segments: TranscriptSegment[]; language: string | null; hasSynthesizedTiming: boolean }

export interface SearchHit {
  id: string;
  ownerId: string;
  kind: 'note' | 'recording' | 'transcriptSegment';
  title: string;
  score: number;
  textScore: number;
  semanticScore: number;
  snippet: string;
  date: Ms;
  time: number | null;
}
export interface Backlink { noteId: string; title: string; snippet: string; line: number }
export interface OutgoingLink { target: string; alias: string | null; noteId: string | null; line: number }
export interface TaskItem { text: string; isDone: boolean; line: number }
export interface Heading { level: number; text: string; line: number }
export interface NoteContext {
  note: Note;
  backlinks: Backlink[];
  outgoing: OutgoingLink[];
  related: SearchHit[];
  tags: string[];
  tasks: TaskItem[];
  headings: Heading[];
}
export interface GraphNode { id: string; title: string; isGhost: boolean; tags: string[]; degree: number; x: number; y: number }
export interface GraphEdge { from: string; to: string; kind: 'link' | 'tag' }
export interface Graph { nodes: GraphNode[]; edges: GraphEdge[]; width: number; height: number }
export interface ImportOutcome { item: MediaItem; duplicate: boolean }
export interface Stats {
  notes: number; recordings: number; videos: number; photos: number; transcripts: number;
  indexedDocuments: number; embedding: string;
}
export interface ExportResult { dir: string; count: number }
