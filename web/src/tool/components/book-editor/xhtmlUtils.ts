import { buildEpubAssetUrl } from '../../api'

export function escapeXml(raw: string): string {
  return raw
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/\u00A0/g, '&#160;')
}

export function plainToCleanXhtml(title: string, plain: string): string {
  const out: string[] = []
  const t = title.trim()
  if (t) {
    out.push(`  <h2>${escapeXml(t)}</h2>`)
  }
  for (const line of plain.split('\n')) {
    const trimmed = line.trim()
    if (!trimmed) continue
    out.push(`  <p>${escapeXml(trimmed)}</p>`)
  }
  if (out.length === 0) {
    out.push('  <p><br/></p>')
  }
  return out.join('\n')
}

export function countNonWhitespace(text: string): number {
  let count = 0
  for (let i = 0; i < text.length; i += 1) {
    const code = text.charCodeAt(i)
    if (code > 32 && code !== 160 && code !== 12288) {
      count += 1
    }
  }
  return count
}

export function extractPlainFromXhtml(xhtml: string, stripHeading?: string): string {
  const doc = new DOMParser().parseFromString(`<body>${xhtml}</body>`, 'text/html')
  const body = doc.body
  if (!body) return ''
  body.querySelectorAll('br').forEach((br) => br.replaceWith('\n'))
  body
    .querySelectorAll('p, div, h1, h2, h3, h4, h5, h6, li, blockquote, section, article, tr')
    .forEach((el) => {
      el.append('\n')
    })
  const paras = (body.textContent ?? '')
    .split('\n')
    .map((l) => l.replace(/\s+/g, ' ').trim())
    .filter(Boolean)

  if (stripHeading && paras[0] === stripHeading.trim()) {
    paras.shift()
  }
  return paras.join('\n')
}

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

const DROP_SUBTREE_TAGS = new Set([
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
  'handler',
  'listener',
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
  'transform',
])

const SAFE_RASTER_DATA_URI_RE =
  /^data:image\/(?:png|jpe?g|gif|webp|avif|bmp);base64,[a-z0-9+/=\s]+$/i

function stripControlChars(value: string): string {
  return value.replace(/[\u0000-\u001F\u007F]+/g, '').trim()
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
  const scheme = schemeMatch[1].toLowerCase()
  return scheme === 'http' || scheme === 'https' || scheme === 'mailto'
}

function sanitizeInlineStyle(raw: string): string {
  return raw
    .replace(/\/\*[\s\S]*?\*\//g, '')
    .replace(/expression\s*\([^)]*\)/gi, '')
    .replace(/-moz-binding\s*:[^;]*/gi, '')
    .replace(/behavior\s*:[^;]*/gi, '')
    .replace(/@import\b[^;]*/gi, '')
    .replace(/url\s*\([^)]*\)/gi, '')
    .replace(/javascript\s*:/gi, '')
    .replace(/vbscript\s*:/gi, '')
    .trim()
}

/**
 * Sanitizes untrusted EPUB CSS before injecting into a `<style>` tag inside an iframe `srcdoc`.
 * Prevents `</style>` tag breakout, `@import` / external `url(...)` tracking requests,
 * and legacy script execution vectors.
 */
export function sanitizeEpubCssForPreview(rawCss: string): string {
  if (!rawCss) return ''
  return rawCss
    .replace(/<\s*\/?\s*style\b[^>]*>/gi, '')
    .replace(/</g, '\\3c ')
    .replace(/@import\b[^;\n]*(?:;|$)/gi, '')
    .replace(/expression\s*\(/gi, 'blocked(')
    .replace(/behavior\s*:/gi, 'x-blocked-behavior:')
    .replace(/-moz-binding\s*:/gi, 'x-blocked-binding:')
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
    if (child.nodeType !== Node.ELEMENT_NODE) {
      continue
    }
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

    // Rewrite EPUB internal image references before attribute allowlist filtering
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

    // Filter attributes against strict allowlist
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
        if (tag !== 'img' || !isSafeImageSrc(attr.value)) {
          childEl.removeAttribute(rawName)
        }
        continue
      }
      if (lowerName === 'href') {
        if (tag === 'image') {
          if (!isSafeImageSrc(attr.value)) {
            childEl.removeAttribute(rawName)
          }
        } else if (tag === 'a') {
          if (!isSafeLinkHref(attr.value)) {
            childEl.removeAttribute(rawName)
          }
        } else {
          childEl.removeAttribute(rawName)
        }
        continue
      }
      if (lowerName === 'style') {
        const safeStyle = sanitizeInlineStyle(attr.value)
        if (safeStyle) {
          childEl.setAttribute(rawName, safeStyle)
        } else {
          childEl.removeAttribute(rawName)
        }
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

/**
 * Converts stored EPUB XHTML into safe HTML for the WYSIWYG surface,
 * sanitizing active elements/attributes/schemes and rewriting internal EPUB
 * image paths to authenticated `/books/epub-asset` URLs.
 */
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

export function computeRelativeZipPath(fromHref: string | null, targetZipPath: string): string {
  const fromDirs = (fromHref ?? '').split('/').slice(0, -1).filter(Boolean)
  const targetParts = targetZipPath.split('/').filter(Boolean)
  let common = 0
  while (
    common < fromDirs.length &&
    common < targetParts.length - 1 &&
    fromDirs[common] === targetParts[common]
  ) {
    common += 1
  }
  const ups = Array(fromDirs.length - common).fill('..')
  return [...ups, ...targetParts.slice(common)].join('/')
}

export function rewriteChapterRelativeAssetPaths(
  xhtmlBody: string,
  fromHref: string | null,
  toHref: string | null,
): string {
  if (!xhtmlBody || fromHref === toHref) return xhtmlBody
  const fromDir = (fromHref ?? '').split('/').slice(0, -1).join('/')
  const toDir = (toHref ?? '').split('/').slice(0, -1).join('/')
  if (fromDir === toDir) return xhtmlBody

  const doc = new DOMParser().parseFromString(`<body>${xhtmlBody}</body>`, 'text/html')
  if (!doc.body) return xhtmlBody

  doc.body.querySelectorAll('img[src]').forEach((el) => {
    const raw = el.getAttribute('src') ?? ''
    const resolved = resolveZipRelativePath(fromHref, raw)
    if (resolved) {
      el.setAttribute('src', computeRelativeZipPath(toHref, resolved))
    }
  })
  doc.body.querySelectorAll('image').forEach((el) => {
    const raw = el.getAttribute('href') ?? el.getAttribute('xlink:href') ?? ''
    const resolved = resolveZipRelativePath(fromHref, raw)
    if (resolved) {
      const next = computeRelativeZipPath(toHref, resolved)
      el.removeAttribute('xlink:href')
      el.setAttribute('href', next)
    }
  })
  return serializeContainerToValidXhtml(doc.body) || xhtmlBody
}

const VOID_TAGS = new Set(['br', 'hr', 'img', 'col', 'meta', 'link', 'input'])
const BLOCK_TAGS = new Set([
  'p',
  'div',
  'h1',
  'h2',
  'h3',
  'h4',
  'h5',
  'h6',
  'blockquote',
  'ul',
  'ol',
  'li',
  'dl',
  'dt',
  'dd',
  'pre',
  'hr',
  'figure',
  'figcaption',
  'table',
  'thead',
  'tbody',
  'tfoot',
  'tr',
  'th',
  'td',
  'caption',
  'section',
  'article',
  'aside',
  'header',
  'footer',
  'main',
  'nav',
])
const ALLOWED_ATTRS = new Set([
  'class',
  'id',
  'href',
  'src',
  'alt',
  'title',
  'dir',
  'lang',
  'xml:lang',
  'style',
  'rel',
  'start',
  'type',
  'colspan',
  'rowspan',
  'width',
  'height',
  'viewBox',
  'preserveAspectRatio',
  'xmlns',
  'xmlns:xlink',
  'xmlns:epub',
])

const ATTR_CANONICAL_CASE: Record<string, string> = {
  viewbox: 'viewBox',
  preserveaspectratio: 'preserveAspectRatio',
}

function isSafeSerializableUri(raw: string, allowRelative = true): boolean {
  const cleaned = stripControlChars(raw)
  if (!cleaned) return false
  if (cleaned.startsWith('#')) return true
  if (SAFE_RASTER_DATA_URI_RE.test(cleaned)) return true
  const schemeMatch = /^([a-z][a-z0-9+.-]*):/i.exec(cleaned)
  if (!schemeMatch) {
    return allowRelative && !cleaned.startsWith('//')
  }
  const scheme = schemeMatch[1]!.toLowerCase()
  return scheme === 'http' || scheme === 'https' || scheme === 'mailto'
}

export function serializeNodeToValidXhtml(node: Node, topLevel = false): string {
  if (node.nodeType === Node.TEXT_NODE) {
    const text = node.nodeValue ?? ''
    if (topLevel) {
      const trimmed = text.trim()
      return trimmed ? `  <p>${escapeXml(trimmed)}</p>` : ''
    }
    return escapeXml(text)
  }

  if (node.nodeType !== Node.ELEMENT_NODE) {
    return ''
  }

  const el = node as HTMLElement
  let tag = el.tagName.toLowerCase()
  if (DROP_SUBTREE_TAGS.has(tag)) return ''
  if (tag === 'b') tag = 'strong'
  if (tag === 'i') tag = 'em'
  // Normalize browser contenteditable `<div>` paragraphs into semantic `<p>` unless it wraps blocks
  if (tag === 'div') {
    const hasBlockChild = Array.from(el.children).some((c) =>
      BLOCK_TAGS.has(c.tagName.toLowerCase()),
    )
    if (!hasBlockChild) {
      tag = 'p'
    }
  }

  const attrs: string[] = []
  const emittedAttrNames = new Set<string>()
  const origEpubSrc = el.getAttribute('data-epub-src')
  const origEpubHref = el.getAttribute('data-epub-href')

  const pushAttr = (name: string, value: string): void => {
    const key = name.toLowerCase()
    if (emittedAttrNames.has(key)) return
    emittedAttrNames.add(key)
    attrs.push(`${name}="${escapeXml(value)}"`)
  }

  for (const attr of Array.from(el.attributes)) {
    const rawName = attr.name
    const lowerName = rawName.toLowerCase()
    if (lowerName === 'data-epub-src' || lowerName === 'data-epub-href') continue
    if (lowerName === 'contenteditable' || lowerName === 'draggable') continue
    if (lowerName.startsWith('on')) continue

    if (tag === 'img' && lowerName === 'src') {
      const target = origEpubSrc || attr.value
      if (isSafeSerializableUri(target, true)) {
        pushAttr('src', target)
      }
      continue
    }
    if (tag === 'image' && (lowerName === 'href' || lowerName === 'xlink:href')) {
      const target = origEpubHref || attr.value
      if (isSafeSerializableUri(target, true)) {
        pushAttr('href', target)
      }
      continue
    }

    const canonicalName = ATTR_CANONICAL_CASE[lowerName] ?? rawName
    if (!ALLOWED_ATTRS.has(canonicalName) && !lowerName.startsWith('epub:')) continue

    if (lowerName === 'href' || lowerName === 'src') {
      if (!isSafeSerializableUri(attr.value, true)) continue
    }
    if (lowerName === 'style') {
      const safeStyle = sanitizeInlineStyle(attr.value)
      if (!safeStyle) continue
      pushAttr('style', safeStyle)
      continue
    }
    pushAttr(canonicalName, attr.value)
  }
  if (tag === 'img' && origEpubSrc && !emittedAttrNames.has('src') && isSafeSerializableUri(origEpubSrc, true)) {
    pushAttr('src', origEpubSrc)
  }
  if (tag === 'image' && origEpubHref && !emittedAttrNames.has('href') && isSafeSerializableUri(origEpubHref, true)) {
    pushAttr('href', origEpubHref)
  }

  const attrStr = attrs.length > 0 ? ` ${attrs.join(' ')}` : ''
  const indent = topLevel ? '  ' : ''

  if (VOID_TAGS.has(tag)) {
    return `${indent}<${tag}${attrStr}/>`
  }

  const childMarkup = Array.from(el.childNodes)
    .map((child) => serializeNodeToValidXhtml(child, false))
    .join('')

  if (topLevel && tag === 'p' && !childMarkup.trim()) {
    return ''
  }

  return `${indent}<${tag}${attrStr}>${childMarkup}</${tag}>`
}

export function serializeContainerToValidXhtml(container: ParentNode): string {
  const lines: string[] = []
  for (const child of Array.from(container.childNodes)) {
    const serialized = serializeNodeToValidXhtml(child, true)
    if (serialized.trim()) {
      lines.push(serialized)
    }
  }
  return lines.join('\n')
}

export function replaceInTextNodes(
  root: Node,
  re: RegExp,
  replacement: string,
  limitOne: boolean,
  literal = true,
): number {
  let replaced = 0
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT)
  const textNodes: Text[] = []
  let cur = walker.nextNode()
  while (cur) {
    textNodes.push(cur as Text)
    cur = walker.nextNode()
  }
  const literalReplacer = () => replacement
  for (const tn of textNodes) {
    const val = tn.nodeValue ?? ''
    re.lastIndex = 0
    if (!re.test(val)) continue
    re.lastIndex = 0
    if (limitOne) {
      const singleFlags = re.flags.replace('g', '')
      const singleRe = new RegExp(re.source, singleFlags)
      tn.nodeValue = literal ? val.replace(singleRe, literalReplacer) : val.replace(singleRe, replacement)
      return 1
    }
    const matches = Array.from(val.matchAll(re)).length
    replaced += matches
    tn.nodeValue = literal ? val.replace(re, literalReplacer) : val.replace(re, replacement)
  }
  return replaced
}

