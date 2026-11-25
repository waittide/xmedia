import { buildEpubAssetUrl } from '../../api'

export function resolveZipRelativePath(baseFileHref: string | null, relTarget: string): string | null {
  const trimmed = relTarget.trim()
  if (
    !trimmed ||
    trimmed.startsWith('http://') ||
    trimmed.startsWith('https://') ||
    trimmed.startsWith('data:') ||
    trimmed.startsWith('blob:') ||
    trimmed.startsWith('/api/')
  ) {
    return null
  }
  const cleanRel = trimmed.split('#')[0]?.split('?')[0] ?? trimmed
  const baseParts = baseFileHref ? baseFileHref.split('/').slice(0, -1) : []
  const stack = [...baseParts.filter(Boolean)]
  for (const seg of cleanRel.split('/')) {
    if (!seg || seg === '.') continue
    if (seg === '..') {
      stack.pop()
    } else {
      stack.push(seg)
    }
  }
  return stack.join('/')
}

export const DROP_SUBTREE_TAGS = new Set([
  'script',
  'style',
  'iframe',
  'frame',
  'frameset',
  'object',
  'embed',
  'applet',
  'link',
  'meta',
  'base',
  'noscript',
  'template',
  'form',
  'input',
  'button',
  'select',
  'textarea',
  'option',
  'audio',
  'video',
  'source',
  'track',
  'foreignobject',
  'animate',
  'animatetransform',
  'animatemotion',
  'set',
  'discard',
  'handler',
  'listener',
  'use',
  'symbol',
  'marker',
  'pattern',
  'mask',
  'clippath',
  'filter',
  'feimage',
  'mpath',
  'metadata',
  'switch',
])

const SAFE_HYDRATE_TAGS = new Set([
  'p',
  'div',
  'span',
  'br',
  'hr',
  'h1',
  'h2',
  'h3',
  'h4',
  'h5',
  'h6',
  'blockquote',
  'pre',
  'code',
  'ul',
  'ol',
  'li',
  'dl',
  'dt',
  'dd',
  'strong',
  'b',
  'em',
  'i',
  'u',
  's',
  'del',
  'ins',
  'sub',
  'sup',
  'small',
  'mark',
  'abbr',
  'cite',
  'q',
  'ruby',
  'rt',
  'rp',
  'figure',
  'figcaption',
  'section',
  'article',
  'aside',
  'header',
  'footer',
  'main',
  'nav',
  'table',
  'thead',
  'tbody',
  'tfoot',
  'tr',
  'th',
  'td',
  'caption',
  'colgroup',
  'col',
  'a',
  'img',
  'svg',
  'g',
  'image',
  'path',
  'rect',
  'circle',
  'ellipse',
  'line',
  'polyline',
  'polygon',
  'defs',
  'title',
  'desc',
  'text',
  'tspan',
])

const SAFE_HYDRATE_ATTRS = new Set([
  'class',
  'id',
  'title',
  'alt',
  'dir',
  'lang',
  'xml:lang',
  'colspan',
  'rowspan',
  'width',
  'height',
  'start',
  'type',
  'viewbox',
  'preserveaspectratio',
  'xmlns',
  'xmlns:xlink',
  'xmlns:epub',
  'data-epub-src',
  'data-epub-href',
  'href',
  'src',
  'style',
  'd',
  'x',
  'y',
  'x1',
  'y1',
  'x2',
  'y2',
  'cx',
  'cy',
  'r',
  'rx',
  'ry',
  'points',
  'fill',
  'stroke',
  'stroke-width',
  'opacity',
])

export const SAFE_RASTER_DATA_URI_RE =
  /^data:image\/(?:png|jpe?g|gif|webp|avif|bmp);base64,[a-z0-9+/=\s]+$/i

export function stripControlChars(value: string): string {
  return value.replace(/[\u0000-\u001F\u007F]+/g, '').trim()
}

function decodeCssEscapes(raw: string): string {
  return raw
    .replace(/\\\r?\n/g, '')
    .replace(/\\([0-9a-fA-F]{1,6})(?:\r\n|[ \t\r\n\f])?/g, (_match, hex: string) => {
      const cp = Number.parseInt(hex, 16)
      if (!Number.isFinite(cp) || cp <= 0 || cp > 0x10ffff || (cp >= 0xd800 && cp <= 0xdfff)) {
        return '\uFFFD'
      }
      return String.fromCodePoint(cp)
    })
    .replace(/\\([^\r\n\f0-9a-fA-F])/g, '$1')
}

function isSafeImageSrc(raw: string): boolean {
  const cleaned = stripControlChars(raw)
  if (!cleaned) return false
  if (
    cleaned.startsWith('/api/v1/tool/books/epub-asset?') ||
    cleaned.startsWith('/api/tool/books/epub-asset?')
  ) {
    return true
  }
  if (cleaned.startsWith('blob:')) return true
  if (SAFE_RASTER_DATA_URI_RE.test(cleaned)) return true
  return false
}

function isSafeLinkHref(raw: string): boolean {
  const cleaned = stripControlChars(raw)
  if (!cleaned) return false
  if (cleaned.startsWith('#')) return true
  const schemeMatch = /^([a-z][a-z0-9+.-]*):/i.exec(cleaned)
  if (!schemeMatch) {
    return !cleaned.startsWith('//')
  }
  const scheme = schemeMatch[1]!.toLowerCase()
  return scheme === 'http' || scheme === 'https' || scheme === 'mailto'
}

const SAFE_BOX_SPACING_RE =
  /^(?:auto|0|\d+(?:\.\d+)?(?:em|rem|px|%))(?:\s+(?:auto|0|\d+(?:\.\d+)?(?:em|rem|px|%))){0,3}$/i
const SAFE_COLOR_VALUE_RE =
  /^(?:#[0-9a-f]{3,8}|rgb\(\s*\d{1,3}\s*,\s*\d{1,3}\s*,\s*\d{1,3}\s*\)|rgba\(\s*\d{1,3}\s*,\s*\d{1,3}\s*,\s*\d{1,3}\s*,\s*(?:0|1|0?\.\d+)\s*\)|transparent|inherit|currentcolor)$/i

const SAFE_STYLE_PROP_VALIDATORS: Record<string, RegExp> = {
  'text-align': /^(?:left|right|center|justify|start|end)$/i,
  'text-indent': /^(?:0|\d+(?:\.\d+)?(?:em|rem|px|%))$/i,
  'font-style': /^(?:normal|italic|oblique)$/i,
  'font-weight': /^(?:normal|bold|bolder|lighter|[1-9]00)$/i,
  'font-size':
    /^(?:xx-small|x-small|small|medium|large|x-large|xx-large|smaller|larger|\d+(?:\.\d+)?(?:em|rem|px|%))$/i,
  'line-height': /^(?:normal|\d+(?:\.\d+)?(?:em|rem|px|%)?)$/i,
  'letter-spacing': /^(?:normal|0|\d+(?:\.\d+)?(?:em|rem|px))$/i,
  'text-decoration':
    /^(?:none|underline|line-through|overline)(?:\s+(?:underline|line-through|overline))*$/i,
  'text-decoration-line':
    /^(?:none|underline|line-through|overline)(?:\s+(?:underline|line-through|overline))*$/i,
  'vertical-align': /^(?:baseline|sub|super|top|text-top|middle|bottom|text-bottom)$/i,
  color: SAFE_COLOR_VALUE_RE,
  'background-color': SAFE_COLOR_VALUE_RE,
  margin: SAFE_BOX_SPACING_RE,
  'margin-top': SAFE_BOX_SPACING_RE,
  'margin-right': SAFE_BOX_SPACING_RE,
  'margin-bottom': SAFE_BOX_SPACING_RE,
  'margin-left': SAFE_BOX_SPACING_RE,
  padding: SAFE_BOX_SPACING_RE,
  'padding-top': SAFE_BOX_SPACING_RE,
  'padding-right': SAFE_BOX_SPACING_RE,
  'padding-bottom': SAFE_BOX_SPACING_RE,
  'padding-left': SAFE_BOX_SPACING_RE,
}

export function sanitizeInlineStyle(raw: string): string {
  if (!raw) return ''
  const decoded = stripControlChars(decodeCssEscapes(raw.replace(/\/\*[\s\S]*?\*\//g, '')))
  if (!decoded) return ''

  const kept: string[] = []
  for (const decl of decoded.split(';')) {
    const colonIdx = decl.indexOf(':')
    if (colonIdx <= 0) continue
    const prop = decl.slice(0, colonIdx).trim().toLowerCase()
    const val = decl
      .slice(colonIdx + 1)
      .replace(/!important\s*$/i, '')
      .replace(/\s+/g, ' ')
      .trim()
    if (!prop || !val) continue
    if (/[<>\\"'`]|\burl\s*\(|\bexpression\s*\(|@import\b|\bvar\s*\(/i.test(val)) continue
    const validator = SAFE_STYLE_PROP_VALIDATORS[prop]
    if (!validator || !validator.test(val)) continue
    kept.push(`${prop}: ${val}`)
  }
  return kept.join('; ')
}

function isSafeSvgPaintValue(raw: string): boolean {
  const decoded = stripControlChars(decodeCssEscapes(raw))
  if (!decoded) return false
  if (decoded.toLowerCase() === 'none') return true
  if (/\burl\s*\(|[<>\\"'`]/i.test(decoded)) return false
  return SAFE_COLOR_VALUE_RE.test(decoded)
}

export function sanitizeEpubCssForPreview(rawCss: string): string {
  if (!rawCss) return ''
  const decoded = decodeCssEscapes(rawCss.replace(/\/\*[\s\S]*?\*\//g, ''))
  return decoded
    .replace(/<\s*\/?\s*style\b[^>]*>/gi, '')
    .replace(/</g, '\\3c ')
    .replace(/@import\b[^;\n]*(?:;|$)/gi, '')
    .replace(/expression\s*\(/gi, 'blocked(')
    .replace(/image-set\s*\(/gi, 'blocked-image-set(')
    .replace(/behavior\s*:/gi, 'x-blocked-behavior:')
    .replace(/-moz-binding\s*:/gi, 'x-blocked-binding:')
    .replace(/position\s*:\s*(?:fixed|sticky)\b/gi, 'position: static')
    .replace(/url\s*\(\s*(['"]?)([^)'"]*)\1\s*\)/gi, (_match, _q: string, inner: string) => {
      const target = stripControlChars(inner)
      if (SAFE_RASTER_DATA_URI_RE.test(target)) {
        return `url("${target}")`
      }
      return 'none'
    })
}

function sanitizeHydratedElement(
  el: Element,
  sourceHref: string | null,
  epubContext?: { rootId: string; relPath: string } | null,
): void {
  const children = Array.from(el.childNodes)
  for (const child of children) {
    if (
      child.nodeType === Node.COMMENT_NODE ||
      child.nodeType === Node.PROCESSING_INSTRUCTION_NODE
    ) {
      child.remove()
      continue
    }
    if (child.nodeType !== Node.ELEMENT_NODE) continue
    const childEl = child as Element
    const tag = childEl.tagName.toLowerCase()

    if (DROP_SUBTREE_TAGS.has(tag)) {
      childEl.remove()
      continue
    }

    if (!SAFE_HYDRATE_TAGS.has(tag)) {
      sanitizeHydratedElement(childEl, sourceHref, epubContext)
      while (childEl.firstChild) {
        childEl.parentNode?.insertBefore(childEl.firstChild, childEl)
      }
      childEl.remove()
      continue
    }

    if (tag === 'img') {
      const origSrc = childEl.getAttribute('src')
      if (origSrc && epubContext) {
        const resolved = resolveZipRelativePath(sourceHref, origSrc)
        if (resolved) {
          childEl.setAttribute('data-epub-src', origSrc)
          childEl.setAttribute(
            'src',
            buildEpubAssetUrl(epubContext.rootId, epubContext.relPath, resolved),
          )
        }
      }
    } else if (tag === 'image') {
      const origHref = childEl.getAttribute('href') || childEl.getAttribute('xlink:href')
      childEl.removeAttribute('xlink:href')
      if (origHref && epubContext) {
        const resolved = resolveZipRelativePath(sourceHref, origHref)
        if (resolved) {
          const apiUrl = buildEpubAssetUrl(epubContext.rootId, epubContext.relPath, resolved)
          childEl.setAttribute('data-epub-href', origHref)
          childEl.setAttribute('href', apiUrl)
        } else if (isSafeImageSrc(origHref)) {
          childEl.setAttribute('href', stripControlChars(origHref))
        } else {
          childEl.removeAttribute('href')
        }
      } else if (origHref && isSafeImageSrc(origHref)) {
        childEl.setAttribute('href', stripControlChars(origHref))
      } else {
        childEl.removeAttribute('href')
      }
    }

    for (const attr of Array.from(childEl.attributes)) {
      const rawName = attr.name
      const lowerName = rawName.toLowerCase()
      if (lowerName.startsWith('on')) {
        childEl.removeAttribute(rawName)
        continue
      }
      if (!SAFE_HYDRATE_ATTRS.has(lowerName) && !lowerName.startsWith('epub:')) {
        childEl.removeAttribute(rawName)
        continue
      }
      if (lowerName === 'src') {
        if (tag !== 'img' || !isSafeImageSrc(attr.value)) childEl.removeAttribute(rawName)
        continue
      }
      if (lowerName === 'href') {
        if (tag === 'image') {
          if (!isSafeImageSrc(attr.value)) childEl.removeAttribute(rawName)
        } else if (tag === 'a' && !childEl.closest('svg')) {
          if (!isSafeLinkHref(attr.value)) childEl.removeAttribute(rawName)
        } else {
          childEl.removeAttribute(rawName)
        }
        continue
      }
      if (lowerName === 'fill' || lowerName === 'stroke') {
        if (!isSafeSvgPaintValue(attr.value)) childEl.removeAttribute(rawName)
        continue
      }
      if (lowerName === 'style') {
        const safeStyle = sanitizeInlineStyle(attr.value)
        if (safeStyle) childEl.setAttribute(rawName, safeStyle)
        else childEl.removeAttribute(rawName)
      }
    }

    if (tag === 'a' && childEl.hasAttribute('href')) {
      const href = childEl.getAttribute('href') ?? ''
      if (/^https?:\/\//i.test(href)) {
        childEl.setAttribute('rel', 'noopener noreferrer')
      }
    }

    sanitizeHydratedElement(childEl, sourceHref, epubContext)
  }
}

export function hydrateXhtmlForBrowser(
  xhtmlBody: string,
  sourceHref: string | null,
  epubContext?: { rootId: string; relPath: string } | null,
): string {
  const raw = xhtmlBody.trim() || '<p><br/></p>'
  const doc = new DOMParser().parseFromString(`<body>${raw}</body>`, 'text/html')
  const body = doc.body
  if (!body) return '<p><br></p>'
  sanitizeHydratedElement(body, sourceHref, epubContext)
  return body.innerHTML || '<p><br></p>'
}
