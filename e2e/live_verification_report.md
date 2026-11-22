# xmedia Live Source Verification Report

- Generated at: `2026-11-22T16:34:21.730Z`
- Mode: `hardened (allow_private_ips=false)`
- Domain gate: **PASS** (Comic=3, Music=3, Video=3)

## Case Summary

| ID | Domain | Status | Duration (ms) | Detail / Error |
|---|---|---|---|---|
| LIVE-01 | comic | PASS | 25983 | {"source":"MangaDex (EN)","keyword":"Yotsuba","mime":"image/jpeg","bytes":52815,"title":"Yotsuba&!"} |
| LIVE-02 | comic | PASS | 17058 | {"source":"WEBTOON (EN)","keyword":"Tower of God","mime":"image/jpeg","bytes":54414,"title":"Tower of God: Urek Mazino"} |
| LIVE-03 | comic | PASS | 4304 | {"source":"XKCD","mime":"image/png","bytes":100654,"title":"#3327 Plate Maintenance"} |
| LIVE-04 | music | PASS | 12002 | {"source":"GDStudio Music API (网易云)","status":206,"audioBytes":4096,"coverMime":"image/jpeg","lyricLines":38} |
| LIVE-05 | music | PASS | 4119 | {"source":"Audius Public API","status":200,"bytesRead":8192,"title":"Xsetra - Fragments"} |
| LIVE-06 | music | PASS | 16916 | {"archive":{"source":"Internet Archive Audio","status":200,"bytesRead":4096,"title":"PFITZNER Violinkonzert h-moll op. 34"},"itunes":{"source":"iTunes Search + LRCLIB","status":200,"bytesRead":4096,"lyricLines":65,"hasPlainLyrics":true,"title":"Bohemian Rhapsody"}} |
| LIVE-07 | video | PASS | 12493 | {"source":"爱坤资源 (ikunzy)","title":"拜托了冰箱2","tsBytes":1880,"encrypted":false,"nodeWarnings":[]} |
| LIVE-08 | video | PASS | 4243 | {"source":"PeerTube (Framatube)","title":"Fondation Blender - Le jeu Yo Frankie","streamType":"hls","status":206,"bytes":4096} |
| LIVE-08B | video | PASS | 1934 | {"source":"Internet Archive Video","title":"Big Buck Bunny","streamType":"mp4","status":206,"bytes":4096} |
| LIVE-09 | ui | PASS | 50930 | {"comic":"Yotsuba&!","music":"Mojito (Live)","video":"拜托了冰箱2","externalThumbnailWarnings":["http://127.0.0.1:38711/api/v1/music/online/cover-proxy?url=https%3A%2F%2Faudius.rickyrombo.com%2Fcontent%2FQmWJcqx3VZQzeKHZ2Ltz3wrzSuycEUdQMyfovK5WuocAs4%2F480x480.jpg&source_id=01a2041b-d591-74d6-b2b0-ec14d1ddeb7d","http://127.0.0.1:38711/api/v1/music/online/cover-proxy?url=https%3A%2F%2Faudius.rickyrombo.com%2Fcontent%2FQmUW3xZY7HZ6MUEbrtv9EoA8CW3MJP2EH4PPTy6ve2soAK%2F480x480.jpg&source_id=01a2041b-d591-74d6-b2b0-ec14d1ddeb7d","http://127.0.0.1:38711/api/v1/music/online/cover-proxy?url=https%3A%2F%2Faudius-nodes.com%2Fcontent%2F01K5152VB2ZJBYNV7GHQMR9A18%2F480x480.jpg&source_id=01a2041b-d591-74d6-b2b0-ec14d1ddeb7d","http://127.0.0.1:38711/api/v1/music/online/cover-proxy?url=https%3A%2F%2Faudius-nodes.com%2Fcontent%2Fbaeaaaiqseddkzxkju6ztsl65lhjcajvw7xrhqbr6mtwfx6k3lzxspsrejf64e%2F480x480.jpg&source_id=01a2041b-d591-74d6-b2b0-ec14d1ddeb7d"]} |
| LIVE-10 | core | PASS | 0 | {"jsonPath":"/data/code/github/waittide/xmedia/e2e/live_verification_report.json","mdPath":"/data/code/github/waittide/xmedia/e2e/live_verification_report.md"} |
