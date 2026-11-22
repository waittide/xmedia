# xmedia Live Source Verification Report

- Generated at: `2026-11-22T15:49:38.752Z`
- Mode: `hardened (allow_private_ips=false)`
- Domain gate: **PASS** (Comic=3, Music=3, Video=3)

## Case Summary

| ID | Domain | Status | Duration (ms) | Detail / Error |
|---|---|---|---|---|
| LIVE-01 | comic | PASS | 27685 | {"source":"MangaDex (EN)","keyword":"Yotsuba","mime":"image/jpeg","bytes":52815,"title":"Yotsuba&!"} |
| LIVE-02 | comic | PASS | 17563 | {"source":"WEBTOON (EN)","keyword":"Tower of God","mime":"image/jpeg","bytes":54414,"title":"Tower of God: Urek Mazino"} |
| LIVE-03 | comic | PASS | 4446 | {"source":"XKCD","mime":"image/png","bytes":100654,"title":"#3327 Plate Maintenance"} |
| LIVE-04 | music | PASS | 7655 | {"source":"GDStudio Music API (网易云)","status":206,"audioBytes":4096,"coverMime":"image/jpeg","lyricLines":38} |
| LIVE-05 | music | PASS | 20171 | {"source":"Audius Public API","status":200,"bytesRead":8192,"title":"Xsetra - Fragments"} |
| LIVE-06 | music | PASS | 20987 | {"archive":{"source":"Internet Archive Audio","status":200,"bytesRead":4096,"title":"Klasika U Pokretu"},"itunes":{"source":"iTunes Search + LRCLIB","status":200,"bytesRead":4096,"lyricLines":65,"hasPlainLyrics":true,"title":"Bohemian Rhapsody"}} |
| LIVE-07 | video | PASS | 14669 | {"source":"爱坤资源 (ikunzy)","title":"拜托了冰箱2","tsBytes":1880,"encrypted":false,"nodeWarnings":[]} |
| LIVE-08 | video | PASS | 9972 | {"source":"PeerTube (Framatube)","title":"Fondation Blender - Le jeu Yo Frankie","streamType":"hls","status":206,"bytes":4096} |
| LIVE-08B | video | PASS | 1923 | {"source":"Internet Archive Video","title":"Big Buck Bunny","streamType":"mp4","status":206,"bytes":4096} |
| LIVE-09 | ui | PASS | 60791 | {"comic":"Yotsuba&!","music":"Mojito (Live)","video":"拜托了冰箱2","externalThumbnailWarnings":["http://127.0.0.1:45841/api/v1/music/online/cover-proxy?url=https%3A%2F%2Fcn2.mainnet.audiusindex.org%2Fcontent%2FQmd5zV99L86gqhJe6uXDMtJ6vs4orV948RjtjywR34WLUt%2F480x480.jpg&source_id=01a203f2-e544-756d-b847-89e4d848cb31"]} |
| LIVE-10 | core | PASS | 0 | {"jsonPath":"/data/code/github/waittide/xmedia/e2e/live_verification_report.json","mdPath":"/data/code/github/waittide/xmedia/e2e/live_verification_report.md"} |
