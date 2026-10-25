# Decisions

Principle: reuse proven MIT/Apache/BSD libraries for everything except Timeline's unique value —
the zoomable Month/Week/Day cluster/zoom logic (custom, with a Rust↔TS golden-fixture parity test).

## Architecture
| Decision | Why |
|---|---|
| **Rust core (`timeline-core`) + one `dispatch(name, json)` command surface** | One tested engine for every platform; the UI is a thin client. Mobile/desktop ports reuse it unchanged. |
| **Tauri 2** shell (`src-tauri`) | Same Rust + web UI on macOS/Windows/Linux, and Android/iOS via `tauri android/ios init`. Small binaries, system webview. Rejected: Electron (heavy, no mobile), Swift (Apple only), Flutter/RN (rewrite UI). |
| **`dev-bridge` (tiny_http)** | Serves the same core over HTTP so Playwright can e2e-test the real Rust core + real UI on a headless Linux box without a desktop webview. Dev/test only, not shipped. |
| Zoom math in Rust *and* TS | Rust is canonical + unit-tested; TS port runs per animation frame (IPC per frame would be too slow). `fixtures/timeline_golden.json` (regen with `UPDATE_GOLDEN=1 cargo test`) is verified by vitest. |
| Media via custom `tlmedia` URI scheme with Range support | Lets `<audio>`/wavesurfer seek without base64 or temp files. Windows/Android use the `http://tlmedia.localhost` form. |

## Notes (core pillar) — adopted
| Choice | Why |
|---|---|
| **CodeMirror 6 + `@lezer/markdown` (GFM)** | Proven editor; not hand-rolled. Live preview = our decorations on the Lezer syntax tree: markup hides unless the cursor touches it; tables render as widgets, and show source when edited. Tiptap/Milkdown rejected: they own a rich-text model, so Markdown round-trip is lossy; CM6 keeps plain Markdown as the source of truth. |
| `@codemirror/autocomplete` | `[[wiki-link]]`, `#tag` and `/slash` sources (custom ranking; "Create …" last). |
| `@codemirror/language-data` | Lazy syntax highlighting for fenced code. |
| Plain Markdown in SQLite; `.md` export | Portable, greppable; export writes files to the data dir. |
| `cmdk` | Ctrl/⌘+K quick switcher. |
| Backlinks, tags, paste-image | Implemented in core (link/tag extraction at save; images stored as media, referenced as `media:<id>`). |

## Other adopted libraries
- **@xyflow/react (React Flow)** for the mind map; **layout is custom Rust Fruchterman–Reingold** (deterministic, tested) with "ghost" nodes for unresolved `[[links]]`. Custom floating straight edges; MiniMap removed (rendered blank).
- **wavesurfer.js** waveform + playback. **lofty** (duration/tags), **hound** (WAV), **png** (image dims).
- **zustand** (tiny store), **date-fns**, **lucide-react**, **React + Vite**, **Vitest + Playwright**.
- **rusqlite (bundled) + rusqlite_migration**, **chrono**, **uuid**, **sha2**, **serde**.
- Tauri plugins: **dialog** (native save), **window-state** (desktop only).

## Rejected / deferred (and why)
- **Tailwind / shadcn / Radix**: plain CSS variables are enough for ~6 screens; avoids a build pipeline and dependency churn. Revisit if UI grows.
- **TanStack Query**: all data is local and in-process; zustand + direct dispatch is simpler.
- **SQLite FTS5 and fastembed-rs**: *deferred*. Search today is in-memory lexical + hashed TF-IDF (1024-d) behind an `EmbeddingProvider` trait, which passes "meaning" tests on the sample data but is much weaker than a real embedding model. Swapping fastembed-rs (ONNX; large model download) in behind the trait is the planned upgrade — not done, not verified.
- **whisper-rs / transcription**: *not implemented*; transcripts are imported (SRT/VTT/JSON/text via a small custom parser, not a subtitle crate — formats are trivial and it avoids a dependency). Local Whisper is a follow-up.
- **Tauri updater / deep-link plugins**: not needed yet. (v0.2: Tauri's native drag-drop is now *enabled* — see "Markdown import" below.)
- **symphonia**: lofty is sufficient for metadata; wavesurfer decodes in the webview.

## UX-only survey (no code taken; several are AGPL/GPL)
Logseq/SiYuan/Zettlr (wiki-links, backlinks, live preview, quick switcher), Joplin/Notesnook (plain Markdown + export), AppFlowy (slash menu), Memos (low-friction capture), Journey/Diaro (journal-by-date timeline), Immich (zoomable time scrubbing), Whishper/Buzz (transcript-synced player).

## Known limitations
- Notes that start with a table get a pipe-text title/preview.
- Month view has unused vertical space.
- Transcript export in Tauri uses the native save dialog; on mobile it falls back to a browser download (unverified).


## v0.2 decisions

### Markdown import (notes and whole folders)
- **Native path import on desktop, text upload in the browser.** In the Tauri app, the native dialog (`@tauri-apps/plugin-dialog`, already in use) and Tauri's `onDragDropEvent` give real file paths; Rust walks folders itself (`walkdir`), which is fast and can resolve relative image paths. Because of that, `dragDropEnabled` is now `true` (HTML5 file drop no longer fires in the webview; one global handler routes dropped `.md`/folders to the importer and audio/video/photos to the media importer). In the browser/e2e build there are no paths, so the UI reads files as text (folder picker `webkitdirectory` / `webkitGetAsEntry`) and uploads them in chunks to `import_inline`; images are *not* imported in that mode (documented in the summary).
- **Background job + polling, no new event plumbing.** `import_start` returns a job id; the UI polls `job_status` (progress 0..1, message, result) and `job_cancel` flips a flag checked per file. Notes are inserted in transactions of 200 and the search index is updated per batch, so 3000 notes import in well under a second in the test (see `tests/import_vault.rs`) and the UI never blocks. The notes list is virtualised (fixed-height windowing) so thousands of rows are fine.
- **Dedupe**: by SHA-256 of the file bytes *and* by canonical source path (stored on the note) so re-importing a vault is idempotent; edited files at the same path are reported as skipped, never overwritten.
- **Dates**: front matter `date` / `created` / `created_at` / `datetime` / `updated`…; else a leading `YYYY-MM-DD` in the file name; else min(created, modified) file time; else now. A date without a time lands at 12:00 local so it does not drift across days with timezones.
- **Titles**: first `# H1`, else front-matter `title`, else file name. Folder structure is stored as `note.folder` (relative to the chosen root). Front matter is kept as `properties` (JSON) and re-emitted on export; `tags:` merge with inline `#tags`.
- **Wiki links**: `[[a]]`, `[[a|alias]]`, `[[a#heading]]`, `[[folder/a]]` resolve by case/diacritic-folded name against note title, file name (slug) and `aliases:` — after the whole batch is in, so order does not matter. Unresolved links stay as ghost nodes in the graph.
- **Attachments**: `![alt](rel/path.png)` and `![[image.png]]` are copied into the content-addressed media store as hidden rows (`media_item.attachment = 1`, not shown in the library) and the body is rewritten to `![alt](media:ID)`, the same form pasted images use. Remote URLs and code blocks are untouched.
- **Obsidian**: a folder containing `.obsidian/` is flagged as a vault; all dot-folders (`.obsidian`, `.git`, `.trash`) and `node_modules` are skipped.
- No YAML crate: the front-matter subset notes actually use (scalars, inline/block lists, `|`/`>` text; nested maps kept as raw text) is ~150 lines and avoids an unmaintained dependency (`serde_yaml` is archived).

## v0.2 — Timeline previews and context menu

- **Preview cards at Week and Month zoom** (`ui/src/lib/preview.ts`, TS-only, covered by `preview.test.ts`; the Rust↔TS golden layout is untouched and still drives Day). Every entry starts at the richest card its zoom allows (rich: title + 4 lines + thumbnail, medium: title + 2 lines, title chip, dot). Card width scales with points-per-day (clamped), thumbnails appear only when the rich card is ≥ 200 px wide. Cards that collide horizontally form a *crowd*; a crowd is stacked into lanes, and if it needs more height than the viewport has, its richest cards are demoted one tier and the layout re-solved until it fits. Touching dots merge into a count badge. Notes with nothing to preview start as title chips. Only the on-screen window is rendered (virtualised) and layout of 5 000 notes is asserted < 500 ms in the unit tests.
- Thumbnails use the original media file (no thumbnail cache yet) — fine for dozens on screen; a resized-thumbnail cache is a later optimisation.
- **Right-click on the timeline**: empty space → "New note here" (dated at the time under the cursor, opens in the editor); an entry → Open / New note here / Delete… (confirmation dialog); a cluster → Zoom in. Touch long-press is not wired (mobile browsers differ); the "Note here" button remains.
- `window.__tl.setTl` is a tiny test hook so Playwright can jump the timeline to a date.
