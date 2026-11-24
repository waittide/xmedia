# xmedia

Unified self-hosted multi-user media server & web workspace built in Rust — bringing **Books**, **Comics**, **Music**, and **Videos** together into a single binary, single database, and unified Web SPA while preserving first-class third-party client protocols.

## Workspace Architecture

```text
xmedia/
├── crates/
│   ├── xmedia-core/    # Shared auth (JWT/Cookie/Basic/ApiKey/Subsonic), libraries, online items, event bus, Range/WebP/FFmpeg
│   ├── xmedia-source/  # Shared Legado HTTP/SSRF/Cookie/template engine, CSS/JSONPath rules, QuickJS sandbox, concurrency & CRUD
│   ├── xmedia-book/    # EPUB/PDF/TXT parsers, book_* persistence, /api/v1/book REST + OPDS 1.2 & 2.0
│   ├── xmedia-comic/   # CBZ/ZIP/EPUB/folder scanner, ComicInfo.xml, comic_* persistence, ETag/304 + OPDS PSE 1.1
│   ├── xmedia-music/   # Lofty tags/ReplayGain, SYLT/LRC multi-encoding lyrics, FFmpeg audio pipe, OpenSubsonic 1.16.1
│   ├── xmedia-video/   # FFprobe multi-audio/subtitle probing, fMP4 smart remux/transcode, SRT/ASS→WebVTT, Jellyfin API
│   ├── xmedia-tool/    # Standalone metadata scraper, inspector, EPUB content editor, and batch organizer backend
│   └── xmedia/         # Feature-gated unified HTTP/WebSocket gateway, rust-embed SPA host, CLI server binary
├── apps/
│   └── xmedia-app/     # Tauri v2 cross-platform desktop/mobile client with embedded local engine & remote OPDS/Subsonic gateway
├── web/                # Vue 3 + TypeScript + Tailwind v4 SPAs (main workspace SPA + xmedia-tool workbench SPA, Material Design 3)
└── e2e/                # Dependency-free Node.js API + CDP browser E2E verification suite
```

## Build & Run

```bash
# 1. Build unified frontend SPAs (main workspace + xmedia-tool workbench)
cd web
pnpm install
pnpm run build
pnpm run build:tool
cd ..

# 2. Build all-in-one server & tool binaries (embed web/dist & web/dist-tool via rust-embed)
cargo build --release -p xmedia -p xmedia-tool

# Optional: build a tailored subset of media domains via Cargo features
cargo build --release -p xmedia --no-default-features --features book,music

# 3. Start server (default http://0.0.0.0:8090, SQLite in ./data/xmedia.db)
./target/release/xmedia
```

## Configuration (CLI / Environment Variables)

| Flag | Env | Default | Description |
| --- | --- | --- | --- |
| `--bind` | `XMEDIA_BIND` | `0.0.0.0:8090` | HTTP listen address |
| `--data-dir` | `XMEDIA_DATA_DIR` | `./data` | Managed uploads, covers/thumbnails, and default SQLite directory |
| `--database-url` | `XMEDIA_DATABASE_URL` | `sqlite:<data_dir>/xmedia.db` | SQLite by default; `postgresql://...` / `mysql://...` require `--features db-postgres` / `db-mysql` |
| `--secret` | `XMEDIA_SECRET` | *(auto-generated)* | Master secret for JWT/Subsonic encryption (auto-generated and persisted to `<data_dir>/.secret` if omitted) |
| `--jwt-ttl-secs` | `XMEDIA_JWT_TTL_SECS` | `604800` | Session token validity in seconds |
| `--ffmpeg-bin` | `XMEDIA_FFMPEG_BIN` | `ffmpeg` | Path to `ffmpeg` (auto-discovers sibling `ffprobe`) |
| `--web-dir` | `XMEDIA_WEB_DIR` | *(none)* | Optional external `web/dist` override |
| `--cors-allow-origin` | `XMEDIA_CORS_ALLOW_ORIGINS` | *(same-origin)* | Optional allowed CORS origin(s); repeatable CLI flag or comma-separated env var |

## Open Client Protocols

- **Books OPDS 1.2 (Atom)**: `http://<host>:8090/opds/book/v1.2`
- **Books OPDS 2.0 (JSON)**: `http://<host>:8090/opds/book/v2`
- **Comics OPDS 1.2 + PSE 1.1 Page Streaming**: `http://<host>:8090/opds/comic/v1.2`
- **Music OpenSubsonic 1.16.1**: `http://<host>:8090` (mounts `/rest/*` for Symfonium, Feishin, Supersonic, Amperfy, etc.)
- **Video Jellyfin-Compatible API**: `http://<host>:8090/jellyfin` (for Infuse, SenPlayer, Swiftfin, etc.)
