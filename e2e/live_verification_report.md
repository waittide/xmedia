# xmedia Live Source Verification Report

- Generated at: `2026-11-22T05:24:17.569Z`
- Mode: `hardened (allow_private_ips=false)`
- Domain gate: **PASS** (Comic=3, Music=3, Video=3)

## Case Summary

| ID | Domain | Status | Duration (ms) | Detail / Error |
|---|---|---|---|---|
| LIVE-01 | comic | PASS | 3134 | {"source":"MangaDex (EN)","keyword":"Yotsuba","mime":"image/jpeg","bytes":52815,"title":"Yotsuba&!"} |
| LIVE-02 | comic | PASS | 4973 | {"source":"WEBTOON (EN)","keyword":"Tower of God","mime":"image/jpeg","bytes":54414,"title":"Tower of God: Urek Mazino"} |
| LIVE-03 | comic | PASS | 2292 | {"source":"XKCD","mime":"image/png","bytes":100654,"title":"#3327 Plate Maintenance"} |
| LIVE-04 | music | PASS | 1981 | {"source":"GDStudio Music API (网易云)","status":206,"audioBytes":4096,"coverMime":"image/jpeg","lyricLines":38} |
| LIVE-05 | music | PASS | 4168 | {"source":"Audius Public API","status":200,"bytesRead":8192,"title":"Xsetra - Fragments"} |
| LIVE-06 | music | PASS | 5861 | {"archive":{"source":"Internet Archive Audio","status":200,"bytesRead":4096,"title":"Whiz M.F. Quiz - Sick Puppies interview"},"itunes":{"source":"iTunes Search + LRCLIB","status":200,"bytesRead":4096,"lyricLines":65,"hasPlainLyrics":true,"title":"Bohemian Rhapsody"}} |
| LIVE-07 | video | PASS | 12205 | {"source":"爱坤资源 (ikunzy)","title":"群英会2026","tsBytes":711204,"encrypted":false,"nodeWarnings":[]} |
| LIVE-08 | video | PASS | 4170 | {"source":"PeerTube (Framatube)","title":"Fondation Blender - Le jeu Yo Frankie","streamType":"hls","status":206,"bytes":4096} |
| LIVE-08B | video | PASS | 1589 | {"source":"Internet Archive Video","title":"Big Buck Bunny","streamType":"mp4","status":206,"bytes":4096} |
| LIVE-09 | ui | PASS | 20863 | {"comic":"Yotsuba&!","music":"Mojito (Live)","video":"群英会2026"} |
| LIVE-10 | core | PASS | 0 | {"jsonPath":"/data/code/github/waittide/xmedia/e2e/live_verification_report.json","mdPath":"/data/code/github/waittide/xmedia/e2e/live_verification_report.md"} |
