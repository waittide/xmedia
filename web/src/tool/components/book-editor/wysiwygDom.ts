const INLINE_FORMAT_SELECTOR = 'strong, b, em, i, u, s, strike, del, code, span'
const BLOCK_ELEMENT_TAGS = new Set([
  'P',
  'DIV',
  'H1',
  'H2',
  'H3',
  'H4',
  'H5',
  'H6',
  'BLOCKQUOTE',
  'LI',
  'PRE',
])

function getRangeInContainer(container: HTMLElement): Range | null {
  const sel = window.getSelection()
  if (!sel || sel.rangeCount === 0) return null
  const range = sel.getRangeAt(0)
  if (!container.contains(range.commonAncestorContainer)) return null
  return range
}

function setCollapsedCaretAfter(node: Node): void {
  const sel = window.getSelection()
  if (!sel) return
  const range = document.createRange()
  range.setStartAfter(node)
  range.collapse(true)
  sel.removeAllRanges()
  sel.addRange(range)
}

function setCaretInsideStart(el: HTMLElement): void {
  const sel = window.getSelection()
  if (!sel) return
  const range = document.createRange()
  range.selectNodeContents(el)
  range.collapse(true)
  sel.removeAllRanges()
  sel.addRange(range)
}

export function insertFragmentAtSelection(
  container: HTMLElement,
  fragment: DocumentFragment,
): void {
  const lastInserted = fragment.lastChild
  const range = getRangeInContainer(container)
  if (range) {
    range.deleteContents()
    range.insertNode(fragment)
  } else {
    container.appendChild(fragment)
  }
  if (lastInserted) {
    setCollapsedCaretAfter(lastInserted)
  }
}

export function insertHtmlAtSelection(container: HTMLElement, html: string): void {
  const template = document.createElement('template')
  template.innerHTML = html
  insertFragmentAtSelection(container, template.content)
}

function findMatchingInlineAncestor(
  container: HTMLElement,
  node: Node | null,
  tagNames: string[],
): HTMLElement | null {
  let cur: Node | null = node
  while (cur && cur !== container) {
    if (cur.nodeType === Node.ELEMENT_NODE) {
      const el = cur as HTMLElement
      if (tagNames.includes(el.tagName.toLowerCase())) {
        return el
      }
    }
    cur = cur.parentNode
  }
  return null
}

function unwrapElement(el: HTMLElement): Node | null {
  const parent = el.parentNode
  if (!parent) return null
  let lastMoved: Node | null = null
  while (el.firstChild) {
    lastMoved = el.firstChild
    parent.insertBefore(el.firstChild, el)
  }
  parent.removeChild(el)
  return lastMoved
}

export function toggleInlineTagAtSelection(
  container: HTMLElement,
  tag: 'strong' | 'em' | 'u' | 'code',
): boolean {
  const range = getRangeInContainer(container)
  if (!range || range.collapsed) return false

  const aliases: Record<'strong' | 'em' | 'u' | 'code', string[]> = {
    strong: ['strong', 'b'],
    em: ['em', 'i'],
    u: ['u'],
    code: ['code'],
  }

  const existing = findMatchingInlineAncestor(
    container,
    range.commonAncestorContainer,
    aliases[tag],
  )
  if (existing && (existing.textContent ?? '') === range.toString()) {
    const last = unwrapElement(existing)
    if (last) setCollapsedCaretAfter(last)
    return true
  }

  const wrapper = document.createElement(tag)
  const extracted = range.extractContents()
  wrapper.appendChild(extracted)
  range.insertNode(wrapper)

  const sel = window.getSelection()
  if (sel) {
    const nextRange = document.createRange()
    nextRange.selectNodeContents(wrapper)
    sel.removeAllRanges()
    sel.addRange(nextRange)
  }
  return true
}

export function removeInlineFormatAtSelection(container: HTMLElement): boolean {
  const range = getRangeInContainer(container)
  if (!range || range.collapsed) return false

  const existing = findMatchingInlineAncestor(
    container,
    range.commonAncestorContainer,
    ['strong', 'b', 'em', 'i', 'u', 's', 'strike', 'del', 'code', 'span'],
  )
  if (existing && (existing.textContent ?? '') === range.toString()) {
    const last = unwrapElement(existing)
    if (last) setCollapsedCaretAfter(last)
    return true
  }

  const fragment = range.extractContents()
  fragment.querySelectorAll(INLINE_FORMAT_SELECTOR).forEach((el) => {
    unwrapElement(el as HTMLElement)
  })
  const plainText = fragment.textContent ?? ''
  const textNode = document.createTextNode(plainText)
  range.insertNode(textNode)
  setCollapsedCaretAfter(textNode)
  return true
}

function findDirectBlockAncestor(container: HTMLElement, node: Node | null): HTMLElement | null {
  let cur: Node | null = node
  while (cur && cur !== container) {
    if (
      cur.nodeType === Node.ELEMENT_NODE &&
      cur.parentNode === container &&
      BLOCK_ELEMENT_TAGS.has((cur as HTMLElement).tagName)
    ) {
      return cur as HTMLElement
    }
    cur = cur.parentNode
  }
  return null
}

export function formatSelectionBlock(
  container: HTMLElement,
  tag: 'p' | 'h1' | 'h2' | 'h3' | 'blockquote',
): boolean {
  const range = getRangeInContainer(container)
  const targetNode = range?.startContainer ?? container.firstChild
  const existingBlock = findDirectBlockAncestor(container, targetNode)

  const replacement = document.createElement(tag)
  if (existingBlock) {
    if (existingBlock.tagName.toLowerCase() === tag) return false
    while (existingBlock.firstChild) {
      replacement.appendChild(existingBlock.firstChild)
    }
    existingBlock.replaceWith(replacement)
  } else if (range && !range.collapsed) {
    replacement.appendChild(range.extractContents())
    range.insertNode(replacement)
  } else {
    replacement.appendChild(document.createElement('br'))
    container.appendChild(replacement)
  }
  setCaretInsideStart(replacement)
  return true
}

export function insertHorizontalRuleAtSelection(container: HTMLElement): void {
  const range = getRangeInContainer(container)
  const hr = document.createElement('hr')
  const nextPara = document.createElement('p')
  nextPara.appendChild(document.createElement('br'))

  const existingBlock = findDirectBlockAncestor(container, range?.startContainer ?? null)
  if (existingBlock && existingBlock.parentNode === container) {
    existingBlock.after(hr, nextPara)
  } else if (range) {
    range.deleteContents()
    const frag = document.createDocumentFragment()
    frag.append(hr, nextPara)
    range.insertNode(frag)
  } else {
    container.append(hr, nextPara)
  }
  setCaretInsideStart(nextPara)
}
