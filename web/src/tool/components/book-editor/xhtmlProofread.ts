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
