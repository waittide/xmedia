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

/**
 * Converts stored EPUB XHTML into safe HTML for the WYSIWYG surface,
 * rewriting internal EPUB image paths to authenticated `/books/epub-asset` URLs.
 */
export function hydrateXhtmlForBrowser(
  xhtmlBody: string,
  sourceHref: string | null,
  epubContext?: { rootId: string; relPath: string } | null,
): string {
  const raw = xhtmlBody.trim() || '<p><br/></p>'
  const doc = new DOMParser().parseFromString(`<body>${raw}</body>`, 'text/html')
  const body = doc.body
  if (!body) return raw

  if (epubContext) {
    body.querySelectorAll('img').forEach((img) => {
      const origSrc = img.getAttribute('src')
      if (!origSrc) return
      const resolved = resolveZipRelativePath(sourceHref, origSrc)
      if (resolved) {
        img.setAttribute('data-epub-src', origSrc)
        img.setAttribute('src', buildEpubAssetUrl(epubContext.rootId, epubContext.relPath, resolved))
      }
    })
    body.querySelectorAll('image').forEach((svgImg) => {
      const origHref = svgImg.getAttribute('href') || svgImg.getAttribute('xlink:href')
      if (!origHref) return
      const resolved = resolveZipRelativePath(sourceHref, origHref)
      if (resolved) {
        const apiUrl = buildEpubAssetUrl(epubContext.rootId, epubContext.relPath, resolved)
        svgImg.setAttribute('data-epub-href', origHref)
        svgImg.setAttribute('href', apiUrl)
      }
    })
  }

  return body.innerHTML || '<p><br></p>'
}

const VOID_TAGS = new Set(['br', 'hr', 'img', 'col', 'meta', 'link', 'input'])
const BLOCK_TAGS = new Set([
  'p',
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
  'pre',
  'hr',
  'figure',
  'figcaption',
  'table',
  'thead',
  'tbody',
  'tr',
  'th',
  'td',
  'section',
  'article',
])
const ALLOWED_ATTRS = new Set([
  'class',
  'id',
  'href',
  'src',
  'alt',
  'title',
  'colspan',
  'rowspan',
  'width',
  'height',
  'viewBox',
  'xmlns',
])

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
  if (tag === 'script' || tag === 'style') return ''
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
  const origEpubSrc = el.getAttribute('data-epub-src')
  const origEpubHref = el.getAttribute('data-epub-href')

  for (const attr of Array.from(el.attributes)) {
    const name = attr.name
    if (name === 'data-epub-src' || name === 'data-epub-href') continue
    if (name === 'contenteditable' || name === 'draggable') continue
    if (tag === 'img' && name === 'src' && origEpubSrc) {
      attrs.push(`src="${escapeXml(origEpubSrc)}"`)
      continue
    }
    if (tag === 'image' && (name === 'href' || name === 'xlink:href') && origEpubHref) {
      attrs.push(`href="${escapeXml(origEpubHref)}"`)
      continue
    }
    if (ALLOWED_ATTRS.has(name) || name.startsWith('epub:')) {
      attrs.push(`${name}="${escapeXml(attr.value)}"`)
    }
  }
  if (tag === 'img' && origEpubSrc && !attrs.some((a) => a.startsWith('src='))) {
    attrs.push(`src="${escapeXml(origEpubSrc)}"`)
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


export function replaceInTextNodes(root: Node, re: RegExp, replacement: string, limitOne: boolean): number {
  let replaced = 0
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT)
  const textNodes: Text[] = []
  let cur = walker.nextNode()
  while (cur) {
    textNodes.push(cur as Text)
    cur = walker.nextNode()
  }
  for (const tn of textNodes) {
    const val = tn.nodeValue ?? ''
    re.lastIndex = 0
    if (!re.test(val)) continue
    re.lastIndex = 0
    if (limitOne) {
      const singleFlags = re.flags.replace('g', '')
      const singleRe = new RegExp(re.source, singleFlags)
      tn.nodeValue = val.replace(singleRe, replacement)
      return 1
    }
    const matches = Array.from(val.matchAll(re)).length
    replaced += matches
    tn.nodeValue = val.replace(re, replacement)
  }
  return replaced
}

