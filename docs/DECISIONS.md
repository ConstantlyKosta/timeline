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
- **Tauri updater / deep-link / drag-drop plugins**: not needed yet. Window drag-drop is disabled so HTML5 file drop works.
- **symphonia**: lofty is sufficient for metadata; wavesurfer decodes in the webview.

## UX-only survey (no code taken; several are AGPL/GPL)
Logseq/SiYuan/Zettlr (wiki-links, backlinks, live preview, quick switcher), Joplin/Notesnook (plain Markdown + export), AppFlowy (slash menu), Memos (low-friction capture), Journey/Diaro (journal-by-date timeline), Immich (zoomable time scrubbing), Whishper/Buzz (transcript-synced player).

## Known limitations
- Notes that start with a table get a pipe-text title/preview.
- Month view has unused vertical space.
- Transcript export in Tauri uses the native save dialog; on mobile it falls back to a browser download (unverified).
