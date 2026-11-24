# `xmedia` Opaque-Box E2E Test Harness (`e2e/`)

Self-contained, zero-dependency **Node.js `v26.13.0` ESM** E2E test infrastructure for `xmedia` online data sources (`comic`, `music`, `video`), legacy/local non-regression (`book`, OPDS, OpenSubsonic), SSRF security hardening, and headless Chromium Web UI verification.

## Prerequisites

- **Node.js** `>= 26` (uses built-in `fetch`, `WebSocket`, `node:test`, `node:zlib`, `node:http`)
- **Chromium** at `/usr/sbin/chromium` (overridable via `XMEDIA_CHROMIUM_BIN`)
- **`xmedia-server` binary** (`cargo build --bin xmedia-server`, or `--xmedia-bin <path>` / `XMEDIA_BIN`)
- **Built Web UI** (`cd web && pnpm run build` -> `web/dist`, or `--web-dir <path>` / `XMEDIA_WEB_DIR`)

Zero `npm install` or third-party packages are needed in `e2e/`.

## Running Tests

```bash
# 1. Harness & Mock Origin self-tests (no xmedia binary needed; includes real Chromium CDP smoke)
node e2e/run.mjs --self-test

# 2. Full offline 4-tier E2E suite (spawns isolated xmedia + Mock Origin Server)
node e2e/run.mjs

# 3. Scoped tier / domain runs
node e2e/run.mjs --tier 1
node e2e/run.mjs --tier 1,2 --domain comic
node e2e/run.mjs --base-url http://127.0.0.1:8090 --tier 1

# 4. Opt-in live external source verification (requires public internet)
node e2e/run.mjs --live
node e2e/run.mjs --live --live-report e2e

# Or via the reproducible verification script (writes e2e/live_verification_report.{md,json})
./scripts/verify_live_sources.sh

# 5. Machine-readable JSON output
node e2e/run.mjs --self-test --json
```

## Directory Layout

| Path | Purpose |
|---|---|
| `e2e/run.mjs` | CLI runner (`--self-test`, `--live`, `--live-report`, `--tier`, `--domain`, `--base-url`, `--xmedia-bin`, `--web-dir`, `--keep-tmp`, `--json`) |
| `e2e/lib/fixtures.mjs` | Deterministic in-memory binary fixtures (`MOCK_JPEG_BYTES`, `MOCK_PNG_BYTES`, `MOCK_MP3_BYTES`, `MOCK_TS_SEGMENT_{1,2}`, `MOCK_MP4_BYTES`, `MOCK_FMP4_{INIT,SEG}`, `MOCK_AES_KEY_BYTES`, `MOCK_GBK_HTML_BYTES`) & magic-byte/header/M3U8 assertions |
| `e2e/lib/mock_origin.mjs` | Deterministic local HTTP/1.1 Mock Origin Server (`127.0.0.1:0`) simulating MangaDex, WEBTOON (`Referer` gate + `data-url`), XKCD, GDStudio (`206` Range + LRC), Audius (`200` non-Range), AppleCMS v10 (nested HLS + AES key + fMP4 map + `0x47` TS), PeerTube (`206` MP4), and `/edge/*` security/error/GBK/cookie endpoints |
| `e2e/lib/client.mjs` | Free TCP port allocator, `XMediaServerManager` (`mock` vs `hardened` SSRF modes, isolated `TempDir` DB, `restart()`), `ApiClient` (Cookie, Bearer, `x-api-key`, GET-only `?token=`, `failLoudlyRequest` throwing `SutEndpointMissingError` on `404`/`405`) |
| `e2e/lib/cdp.mjs` | Zero-dependency `/usr/sbin/chromium` CDP WebSocket driver (`goto`, `evalJs`, `waitForSelector`, `waitForFn`, `click`, `type`, `setCookie`, `pressKey`, exception/console/HTTP error capture with `/_app/health` allowlist, guaranteed process & profile cleanup) |
| `e2e/fixtures/mock_sources.mjs` | Factory functions `buildMockComicSources`, `buildMockMusicSources`, `buildMockVideoSources`, `buildMockBookSources`, and `buildEdgeMockSources` |
| `e2e/tests/self_test.mjs` | `ST-01`..`ST-10` + `ST-CDP` harness & Mock Origin self-tests |
| `e2e/tests/tier{1,2,3,4}.mjs` | 4-tier opaque-box E2E suites |
| `e2e/tests/live.mjs` | Opt-in `--live` external source verification suite |

## Dual-Mode SUT Topology & Fail-Loudly Contract

- **Mock Mode (`mode: 'mock'`)**: Spawns `xmedia --bind 127.0.0.1:<port>` with `XMEDIA_ALLOW_PRIVATE_IPS_FOR_TESTING=1` so opaque-box HTTP and Chromium flows can exercise the local `MockOriginServer`.
- **Hardened Mode (`mode: 'hardened'`)**: Spawns `xmedia` **without** `XMEDIA_ALLOW_PRIVATE_IPS_FOR_TESTING` (`allow_private_ips = false`) for `F2` SSRF blocking checks and `--live` verification.
- **Fail-Loudly Guarantee**: `ApiClient.failLoudlyRequest` throws `SUT_ENDPOINT_MISSING` immediately if any expected route returns `404` or `405`, and `e2e/run.mjs` exits non-zero if any requested suite file is missing or reports 0 tests.

## Suite Export Contract

Each `e2e/tests/tier{N}.mjs` exports `async function runTier{N}(context)` (or `runSuite(context)`), and `e2e/tests/live.mjs` exports `async function runLiveTests(context)`, returning:

```js
{ suite: string, total: number, passed: number, failed: number, cases: Array<{ id, title, status, durationMs, error? }> }
```
