try {
  var p = JSON.parse(localStorage.getItem('xmedia.preferences') || '{}')
  var m = p.theme_mode || 'system'
  var d = m === 'dark' || (m !== 'light' && matchMedia('(prefers-color-scheme:dark)').matches)
  var cached = JSON.parse(localStorage.getItem('xmedia.theme_color_cache') || '{}')
  if (d) {
    document.documentElement.dataset.theme = 'dark'
  }
  var mt = document.querySelector('meta[name="theme-color"]')
  if (mt) {
    var color = d ? cached.dark : cached.light
    if (typeof color === 'string' && /^#[0-9a-f]{6}$/i.test(color)) {
      mt.content = color
    } else if (d) {
      mt.content = '#1d2023'
    }
  }
  var icon = typeof p.app_icon === 'string' ? p.app_icon.trim().toLowerCase() : ''
  if (icon === 'porcelain' || icon === 'obsidian' || icon === 'monolith' || icon === 'cobalt') {
    var fav = document.getElementById('app-favicon')
    if (fav) {
      fav.setAttribute('href', '/icons/' + icon + '.svg')
    }
  }
} catch (e) {
  // ignore storage/media errors
}
