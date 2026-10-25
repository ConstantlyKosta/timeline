# Timeline (Tauri rebuild)

Privacy-first, local-first autobiographical memory: Markdown notes (live preview, `[[wiki-links]]`, `#tags`, backlinks, tables, tasks, code), a mind map of your notes, audio recordings with transcripts, and a zoomable **Month / Week / Day** timeline. Everything stays on your machine in one SQLite file plus a `media/` folder. Hybrid search (keyword + hashed-TF-IDF "meaning").

Layout: `crates/timeline-core` (Rust engine, 78 tests) · `crates/dev-bridge` (HTTP server for tests/dev) · `src-tauri` (desktop/mobile shell) · `ui` (React + TypeScript + Vite) · `docs/` (decisions, screenshots).

## Run it on a Mac (exact steps)
```bash
xcode-select --install                                  # Command Line Tools (skip if present)
curl https://sh.rustup.rs -sSf | sh && . ~/.cargo/env  # Rust (stable)
# Node 20+: https://nodejs.org or `brew install node`
cargo install tauri-cli --version "^2" --locked        # provides `cargo tauri`

cd timeline-tauri
npm --prefix ui ci
cargo tauri dev            # run the app with hot reload
cargo tauri build          # produce .app and .dmg in target/release/bundle/
```
First run seeds a small sample library. Data lives in the OS app-data dir (macOS: `~/Library/Application Support/app.timeline.local`); override with `TIMELINE_DATA_DIR=/some/dir`. Unsigned builds: right-click → Open the first time (Gatekeeper).

## Develop & test
```bash
cargo test --workspace                 # Rust: core + bridge + shell (needs recent stable Rust; run `rustup update stable`)
npm --prefix ui test                   # vitest: TS zoom port vs Rust golden fixture
npm --prefix ui run build              # typecheck + production UI
npm --prefix ui run e2e                # Playwright (system Chrome) vs real Rust core via dev-bridge, port 8923
# browser-only dev without Tauri:
cargo run -p dev-bridge -- --data ./dev-data --port 8923 --static ui/dist
```
Regenerate the Rust↔TS golden fixture after changing zoom math: `UPDATE_GOLDEN=1 cargo test -p timeline-core`.
Screenshots from the e2e run are written to `docs/screenshots/`.

## Package
- Desktop: `cargo tauri build` (use `--no-bundle` for just the binary). CI: `.github/workflows/build.yml` (tauri-action for macOS arm64/x64, Windows, Linux — **never run**).
- Windows: install Rust (MSVC), Node, WebView2 (preinstalled on Win 11); same commands.
- Android: install Android Studio + NDK, set `ANDROID_HOME`/`NDK_HOME`, then `cargo tauri android init && cargo tauri android dev`.
- iOS: Xcode + `rustup target add aarch64-apple-ios`, then `cargo tauri ios init && cargo tauri ios dev`.
  Mobile builds are **untested** — the UI has a phone layout (tested at 390px in desktop Chrome only).

## Status — honest
Verified on this Linux box: `cargo test --workspace` green (78 core + bridge + shell tests); vitest 17/17; Playwright 10/10 (Chrome, real Rust core) with screenshots; `cargo tauri build --no-bundle` compiles; the built binary launched under Xvfb and rendered the seeded timeline (`docs/screenshots/20-tauri-native-linux.png`).
**Not verified:** macOS/Windows/iOS/Android builds; packaged `.dmg/.msi/.AppImage`; WKWebView behaviour (touch pinch, `tlmedia` Range audio seeking, native save dialog); the GitHub workflow; real embedding model / Whisper (not implemented); Tauri IPC paths beyond the Linux smoke launch (e2e drives the HTTP bridge, not IPC).

See `docs/DECISIONS.md` for library choices and `THIRD_PARTY_NOTICES`.
