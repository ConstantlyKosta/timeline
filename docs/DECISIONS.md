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
- *(superseded in v0.2, see "Mind map" below)* **@xyflow/react (React Flow)** for the mind map; **layout was custom Rust Fruchterman–Reingold** (deterministic, tested) with "ghost" nodes for unresolved `[[links]]`. Custom floating straight edges; MiniMap removed (rendered blank).
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


## v0.2 — Mind map (port of Obsidian "Extended Graph" features)

- **Licence hygiene.** Extended Graph (ElsaTam/obsidian-extended-graph) is **GPL-3.0**. Timeline is not GPL, so nothing from its source was copied or adapted; its README/feature list was the *spec only*, and everything here was written from scratch against that list. (`THIRD_PARTY_NOTICES` records this.)
- **Renderer: Sigma.js v3.0.3 + graphology** (both MIT; licences checked in the npm packages before adoption). Why: WebGL rendering handles thousands of nodes/edges smoothly where the previous React Flow (DOM nodes) did not, and Sigma has first-party programs for exactly what the spec needs: node images (`@sigma/node-image`: photos as node fills and one-colour pictograms for shapes) and curved/arrow edges (`@sigma/edge-curve`). graphology is Sigma's native graph model (multigraph, so a note can have a text link and a property link to the same note).
- **Sigma v3, not v4.** v4 is very new, written with LLM assistance, has an unfamiliar API, and its docs were unreachable while building; v3.0.3 is the documented, widely used release. Easy to revisit.
- **Layout: d3-force (ISC), run in the browser** (`ui/src/lib/forceSim.ts`) instead of the earlier Rust layout. Reason: the forces panel needs *live* sliders (centre, repel, link strength, link distance, collision) and dragging with re-heating; a client simulation does that without an IPC round trip per tick. The first layout is computed synchronously (tick budget shrinks for big graphs), then the simulation runs live only when something changes. The Rust side supplies the rich graph (`graph_data`) and persists state.
- **Persistence.** Node positions are stored in `node_position` and named views in `graph_view` (both in migration 2); current settings also live in `localStorage`. Pinned nodes (drag = pin, double-click toggles) are stored in the settings/view. Opening the map again shows the same picture, not a re-layout.
- **Shapes** are drawn by Sigma's image program as one-colour SVG pictograms (circle uses the native circle program) because WebGL node programs per shape would mean writing GLSL. Pictogram tinting works in headless Chrome; the real Tauri webviews are unverified.
- **Filtering/colouring logic is pure TypeScript** (`ui/src/lib/graphModel.ts`, 23 unit tests): query language (`tag:`, `#tag`, `folder:`, `prop:k=v`, `kind:`, `-term`), groups, PageRank/degree/length/recency sizing, local graph (BFS depth N), link types (text link, property link, attachment embed, tag/folder membership).
- Fixture: `fixtures/gen-big-vault.mjs` generates a deterministic ~110 note vault (folders, tags, properties, images, ghosts) used by `ui/e2e/zgraph.spec.ts` and the screenshots `30`–`35`.
- **Not ported** from Extended Graph (spec = its README and wiki pages on tags/properties/node names): export to SVG (and PNG); multi-level coloured **arcs/rings** around a node for several tags/properties at once (we use one fill colour per node plus shape, and colour *groups*); per-tag/per-link-type images or icons; the full node-name options (character limit, "use a property as label", background plate, dynamic vertical label offset — we offer always/when-zoomed/never and show neighbour labels on hover); styling of the "currently open note" and reflecting open tabs in the graph; its larger menu of statistical node/link size functions (we have uniform, degree, in-degree, out-degree, word count, recency and PageRank for nodes, and no link-size function); folder pseudo-nodes are offered as hub nodes instead of its arc-style folders; Dataview inline properties; its exact Obsidian look and plugin integration (it patches Obsidian's native graph, this is a separate viewer). Touch drag/long-press is not wired.


## v0.2 — Speech-to-text (whisper.cpp, local)

- **Engine: whisper.cpp through `whisper-rs` 0.16** (MIT / Unlicense), fully offline after the model download. Chosen over cloud APIs (privacy is the product's premise) and over candle/burn-based ports (slower, less mature). It is C++, compiled by `cmake` during `cargo build`, so CI needs cmake + libclang (Ubuntu: apt; Windows: preinstalled LLVM via `LIBCLANG_PATH`; macOS: Xcode toolchain).
- **Separate crate `timeline-asr`, behind a trait in core.** `timeline-core` only knows the `Asr` trait (`models`, `download`, `transcribe`), so core tests stay fast and C++-free; `dev-bridge` and `src-tauri` plug in `WhisperAsr`. `TIMELINE_ASR=fake` swaps in a deterministic `FakeAsr` so the UI e2e can run the whole flow without a 150 MB download.
- **Models are never bundled.** Catalog: tiny, **base.en (default, 148 MB, English)**, base, small.en, small, large-v3-turbo-q5_0. Downloaded on first use from `huggingface.co/ggerganov/whisper.cpp` (override with `TIMELINE_ASR_MIRROR`) into `<data dir>/models/`, streamed to a `.part` file, **SHA-256 verified** before it is renamed, with progress + cancel. Offline gives a plain error ("could not reach Hugging Face…") and no half-written model. The multilingual models are offered because the user mixes Russian and English; language is *auto-detect* by default and can be pinned.
- **Decoding:** `symphonia` (MPL-2.0, used unmodified) decodes MP3 / M4A(AAC) / WAV / FLAC / OGG and the audio track of MP4, then mono-mixes and resamples to 16 kHz.
- **Jobs:** transcription and download run as background jobs (same `Jobs` mechanism as import) with progress, cancel and a `subject` (media id / model id) so the UI can re-attach after navigating. Segments are written to the existing `transcript_segment` table in one transaction at the end — a cancelled or failed run never destroys an existing transcript — so the synced transcript player, search and the mind map need no changes. Photos are rejected.
- **Settings** (`meta` table): `asr.model`, `asr.language`, `asr.auto`. *Auto-transcribe on import* is **off by default**; when on it runs from the UI after an import and does nothing (but says so) if the model is not downloaded.
- **whisper-rs 0.16 bug worked around:** `set_abort_callback_safe` has a mismatched closure type and segfaults when called. We register the raw `set_abort_callback` + `set_abort_callback_user_data` pointing at our own cancel closure instead. (Progress callback is the safe one and works.)
- **Verified (Linux x86-64, 8 cores, CPU only):** `jfk.wav`, `.mp3`, `.m4a`, `.mp4` and a 44.1 kHz stereo WAV all transcribe correctly with base.en at real-time factor 0.07–0.11. A Russian TTS clip is poor on `base` (language mis-detected) and good on `large-v3-turbo-q5_0`, but that model runs at RTF 2.8 on this CPU — so it is offered, not the default.
- **Limits:** language is detected once per file (first 30 s), so a recording that switches language midway is transcribed in the first language; the whole decoded PCM is held in memory (≈230 MB per hour of audio); CPU only — the `gpu` cargo feature enables Metal but is untested; never run on real macOS/Windows hardware (only compiled in CI).

## v0.2 — Per-note mind map (Markmap-style outline)
- Each note has a **Mind map** toggle (Notes view) that shows the note's headings and bullets (and task items as ☐/☑) as a collapsible tree beside the editor. The toggle is remembered (`localStorage`).
- **Drawing:** `markmap-view` 0.18 (MIT) renders the tree; pan/zoom/fold come from it. **Parsing:** a small own parser (`ui/src/lib/outline.ts`) instead of `markmap-lib`, because `markmap-lib` does not give a source line per node, which click-to-jump needs. A single top-level `# Title` becomes the root; otherwise the note title is the root.
- **Click a branch** → the editor jumps to that line. The map follows typing (250 ms debounce). The map re-fits until you pan/zoom it yourself; the corner button re-fits.
- **Limits:** paragraphs and setext (`===`) headings are not nodes; images/links inside nodes are shown as plain text; no export of the map.
