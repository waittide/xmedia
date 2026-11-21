import type { BookJsContextInput, OnlineChapterItem } from '../../onlineApi'

export interface ReaderContext {
  shelfId?: string
  sourceId: string
  sourceName: string
  name: string
  author: string
  bookUrl: string
  tocUrl: string
  coverUrl?: string | null
  intro?: string | null
  currentChapterIndex: number
}

export interface LoadedOnlineChapter {
  pos: number
  chapterIndex: number
  title: string
  paragraphs: string[]
  cached: boolean
}

const CN_DIGITS: Record<string, number> = {
  零: 0,
  〇: 0,
  一: 1,
  二: 2,
  两: 2,
  三: 3,
  四: 4,
  五: 5,
  六: 6,
  七: 7,
  八: 8,
  九: 9,
}

const CN_UNITS: Record<string, number> = {
  十: 10,
  百: 100,
  千: 1000,
  万: 10000,
}

export function parseChineseInt(token: string): number | null {
  if (!token) return null
  if (/^\d+$/.test(token)) return Number.parseInt(token, 10)
  let total = 0
  let section = 0
  let number = 0
  let matched = false
  for (const ch of token) {
    if (ch in CN_DIGITS) {
      number = CN_DIGITS[ch]!
      matched = true
    } else if (ch in CN_UNITS) {
      const unit = CN_UNITS[ch]!
      matched = true
      if (unit === 10000) {
        section = (section + number) * unit
        total += section
        section = 0
        number = 0
      } else {
        if (number === 0 && unit === 10) number = 1
        section += number * unit
        number = 0
      }
    } else {
      return null
    }
  }
  return matched ? total + section + number : null
}

export function normalizeChapterTitle(raw: string): string {
  return raw
    .normalize('NFKC')
    .toLowerCase()
    .replace(/[\s\p{P}\p{S}_-]+/gu, '')
}

export function extractChapterSignature(raw: string): {
  num: number | null
  body: string
  norm: string
} {
  const cleaned = raw.normalize('NFKC').trim()
  const norm = normalizeChapterTitle(cleaned)
  const m = cleaned.match(
    /^(?:第\s*)?([0-9零〇一二两三四五六七八九十百千万]+)\s*(?:[章回节话卷集幕篇]|[\s.:：、\-]+)(.*)$/i,
  )
  if (m) {
    const num = parseChineseInt(m[1] ?? '')
    const body = normalizeChapterTitle(m[2] ?? '')
    return { num, body, norm }
  }
  return { num: null, body: norm, norm }
}

export function diceSimilarity(a: string, b: string): number {
  if (!a || !b) return 0
  if (a === b) return 1
  if (a.length < 2 || b.length < 2) {
    return a.includes(b) || b.includes(a) ? 0.6 : 0
  }
  const bigrams = new Map<string, number>()
  for (let i = 0; i < a.length - 1; i += 1) {
    const bg = a.slice(i, i + 2)
    bigrams.set(bg, (bigrams.get(bg) ?? 0) + 1)
  }
  let overlap = 0
  for (let i = 0; i < b.length - 1; i += 1) {
    const bg = b.slice(i, i + 2)
    const cnt = bigrams.get(bg) ?? 0
    if (cnt > 0) {
      overlap += 1
      bigrams.set(bg, cnt - 1)
    }
  }
  return (2 * overlap) / (a.length - 1 + (b.length - 1))
}

export function matchChapterIndexByTitle(
  targetTitle: string,
  list: OnlineChapterItem[],
  fallbackIdx: number,
): number {
  const srcSig = extractChapterSignature(targetTitle)
  if (!srcSig.norm) return fallbackIdx

  let bestIdx = -1
  let bestScore = 0

  for (let i = 0; i < list.length; i += 1) {
    const ch = list[i]
    if (!ch || ch.is_volume || !ch.url) continue
    const candSig = extractChapterSignature(ch.title)
    if (!candSig.norm) continue

    let score = 0
    if (candSig.norm === srcSig.norm) {
      score = 1.0
    } else if (srcSig.body && candSig.body && srcSig.body === candSig.body) {
      score = srcSig.num !== null && candSig.num === srcSig.num ? 0.98 : 0.9
    } else if (
      srcSig.norm.length >= 3 &&
      candSig.norm.length >= 3 &&
      (candSig.norm.includes(srcSig.norm) || srcSig.norm.includes(candSig.norm))
    ) {
      const ratio =
        Math.min(srcSig.norm.length, candSig.norm.length) /
        Math.max(srcSig.norm.length, candSig.norm.length)
      score = 0.72 + ratio * 0.2
    } else {
      const simFull = diceSimilarity(srcSig.norm, candSig.norm)
      const simBody = srcSig.body && candSig.body ? diceSimilarity(srcSig.body, candSig.body) : 0
      score = Math.max(simFull, simBody)
      if (srcSig.num !== null && candSig.num !== null && srcSig.num === candSig.num) {
        score = Math.max(score, 0.7 + simBody * 0.25)
      }
    }

    const dist = Math.abs(i - fallbackIdx)
    const tieBreaker = Math.max(0, 0.02 - dist * 0.0001)
    const finalScore = score + tieBreaker

    if (finalScore > bestScore) {
      bestScore = finalScore
      bestIdx = i
    }
  }

  return bestIdx >= 0 && bestScore >= 0.45 ? bestIdx : fallbackIdx
}

export function buildBookJsCtxFrom(
  c: ReaderContext,
  idx: number,
  title: string,
): BookJsContextInput {
  return {
    name: c.name,
    author: c.author,
    book_url: c.bookUrl,
    toc_url: c.tocUrl,
    cover_url: c.coverUrl ?? null,
    intro: c.intro ?? null,
    dur_chapter_index: idx,
    dur_chapter_title: title,
  }
}

export function resolveChapterPos(list: OnlineChapterItem[], rawIdx: number): number {
  if (list.length === 0) return 0
  const byIndex = list.findIndex((c) => c.index === rawIdx)
  if (byIndex >= 0) return byIndex
  return Math.max(0, Math.min(rawIdx, list.length - 1))
}
