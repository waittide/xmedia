try {
  var p = JSON.parse(localStorage.getItem('xmedia.preferences') || '{}')
  var m = p.theme_mode || 'system'
  var d = m === 'dark' || (m !== 'light' && matchMedia('(prefers-color-scheme:dark)').matches)
  if (d) {
    document.documentElement.dataset.theme = 'dark'
    var mt = document.querySelector('meta[name="theme-color"]')
    if (mt) mt.content = '#1d2023'
  }
} catch (e) {
  // ignore storage/media errors
}
