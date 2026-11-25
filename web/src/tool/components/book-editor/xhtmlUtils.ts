import { escapeXml } from './xhtmlProofread'
import {
  DROP_SUBTREE_TAGS,
  SAFE_RASTER_DATA_URI_RE,
  resolveZipRelativePath,
  sanitizeInlineStyle,
  stripControlChars,
} from './xhtmlSanitizer'

export {
  countNonWhitespace,
  escapeXml,
  extractPlainFromXhtml,
  plainToCleanXhtml,
  replaceInTextNodes,
} from './xhtmlProofread'

export {
  hydrateXhtmlForBrowser,
  resolveZipRelativePath,
  sanitizeEpubCssForPreview,
  sanitizeInlineStyle,
} from './xhtmlSanitizer'

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

  if (node.nodeType !== Node.ELEMENT_NODE) return ''

  const el = node as HTMLElement
  let tag = el.tagName.toLowerCase()
  if (DROP_SUBTREE_TAGS.has(tag)) return ''
  if (tag === 'b') tag = 'strong'
  if (tag === 'i') tag = 'em'
  if (tag === 'div') {
    const hasBlockChild = Array.from(el.children).some((c) =>
      BLOCK_TAGS.has(c.tagName.toLowerCase()),
    )
    if (!hasBlockChild) tag = 'p'
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
      if (isSafeSerializableUri(target, true)) pushAttr('src', target)
      continue
    }
    if (tag === 'image' && (lowerName === 'href' || lowerName === 'xlink:href')) {
      const target = origEpubHref || attr.value
      if (isSafeSerializableUri(target, true)) pushAttr('href', target)
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

  if (VOID_TAGS.has(tag)) return `${indent}<${tag}${attrStr}/>`

  const childMarkup = Array.from(el.childNodes)
    .map((child) => serializeNodeToValidXhtml(child, false))
    .join('')

  if (topLevel && tag === 'p' && !childMarkup.trim()) return ''

  return `${indent}<${tag}${attrStr}>${childMarkup}</${tag}>`
}

export function serializeContainerToValidXhtml(container: ParentNode): string {
  const lines: string[] = []
  for (const child of Array.from(container.childNodes)) {
    const serialized = serializeNodeToValidXhtml(child, true)
    if (serialized.trim()) lines.push(serialized)
  }
  return lines.join('\n')
}
