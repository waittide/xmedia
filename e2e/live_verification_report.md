# xmedia Live Source Verification Report

- Generated at: `2026-11-22T16:01:38.933Z`
- Mode: `hardened (allow_private_ips=false)`
- Domain gate: **PASS** (Comic=3, Music=3, Video=3)

## Case Summary

| ID | Domain | Status | Duration (ms) | Detail / Error |
|---|---|---|---|---|
| LIVE-01 | comic | PASS | 9030 | {"source":"MangaDex (EN)","keyword":"Yotsuba","mime":"image/jpeg","bytes":52815,"title":"Yotsuba&!"} |
| LIVE-02 | comic | PASS | 34787 | {"source":"WEBTOON (EN)","keyword":"Tower of God","mime":"image/jpeg","bytes":54414,"title":"Tower of God: Urek Mazino"} |
| LIVE-03 | comic | PASS | 19321 | {"source":"XKCD","mime":"image/png","bytes":100654,"title":"#3327 Plate Maintenance"} |
| LIVE-04 | music | PASS | 15943 | {"source":"GDStudio Music API (网易云)","status":206,"audioBytes":4096,"coverMime":"image/jpeg","lyricLines":38} |
| LIVE-05 | music | PASS | 10145 | {"source":"Audius Public API","status":200,"bytesRead":8192,"title":"Xsetra - Fragments"} |
| LIVE-06 | music | PASS | 47313 | {"archive":{"source":"Internet Archive Audio","status":200,"bytesRead":4096,"title":"Klasika U Pokretu"},"itunes":{"source":"iTunes Search + LRCLIB","status":200,"bytesRead":4096,"lyricLines":65,"hasPlainLyrics":true,"title":"Bohemian Rhapsody"}} |
| LIVE-07 | video | PASS | 4736 | {"source":"爱坤资源 (ikunzy)","title":"拜托了冰箱2","tsBytes":1880,"encrypted":false,"nodeWarnings":[]} |
| LIVE-08 | video | PASS | 19694 | {"source":"PeerTube (Framatube)","title":"Fondation Blender - Le jeu Yo Frankie","streamType":"hls","status":206,"bytes":4096} |
| LIVE-08B | video | PASS | 23491 | {"source":"Internet Archive Video","title":"Big Buck Bunny","streamType":"mp4","status":206,"bytes":4096} |
| LIVE-09 | ui | PASS | 38602 | {"comic":"Yotsuba&!","music":"Mojito (Live)","video":"拜托了冰箱2"} |
| LIVE-10 | core | PASS | 0 | {"jsonPath":"/data/code/github/waittide/xmedia/e2e/live_verification_report.json","mdPath":"/data/code/github/waittide/xmedia/e2e/live_verification_report.md"} |
