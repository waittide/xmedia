import type { MediaKind } from '@/types/api'
import type { ToolCompletenessFilter } from '../../completeness'
import type { ToolSettingsTab } from '../ToolSettingsView.vue'

export type PrimaryNavKey = 'home' | 'items' | 'settings'
export type BrowseViewMode = 'flat' | 'folders'
export type SortField = 'name' | 'title' | 'incomplete' | 'size' | 'modified'
export type SortDir = 'asc' | 'desc'
export type CompletenessFilter = ToolCompletenessFilter

export const VALID_DOMAINS: MediaKind[] = ['book', 'comic', 'music', 'video']
export const PAGE_CHUNK_SIZE = 100

export interface ParsedToolHash {
  domain?: MediaKind
  nav?: PrimaryNavKey
  rootId?: string
  relPath?: string
  settingsTab?: ToolSettingsTab
  workbench?: boolean
  item?: string
}

export interface BuildToolHashInput {
  domain: MediaKind
  nav: PrimaryNavKey
  rootId?: string
  relPath?: string
  settingsTab?: ToolSettingsTab
  workbench?: boolean
  item?: string | null
}

export function parseToolHash(): ParsedToolHash {
  if (typeof window === 'undefined') return {}
  const raw = window.location.hash.replace(/^#\/?/, '')
  if (!raw) return {}
  const [pathPart, queryPart = ''] = raw.split('?')
  const segs = (pathPart ?? '').split('/').filter(Boolean)
  const out: ParsedToolHash = {}
  if (segs[0] && VALID_DOMAINS.includes(segs[0] as MediaKind)) {
    out.domain = segs[0] as MediaKind
  }
  if (segs[1] === 'home' || segs[1] === 'items' || segs[1] === 'settings') {
    out.nav = segs[1]
  }
  const params = new URLSearchParams(queryPart)
  const root = params.get('root')
  if (root) out.rootId = root
  const path = params.get('path')
  if (path !== null) out.relPath = path
  const tab = params.get('tab')
  if (tab === 'appearance' || tab === 'scrapers' || tab === 'roots' || tab === 'templates') {
    out.settingsTab = tab
  }
  if (params.get('wb') === '1') {
    out.workbench = true
  }
  const item = params.get('item')
  if (item) {
    out.item = item
  }
  return out
}

export function buildToolHash(state: BuildToolHashInput): string {
  const params = new URLSearchParams()
  if (state.nav === 'settings') {
    params.set('tab', state.settingsTab ?? 'appearance')
  } else {
    if (state.rootId) params.set('root', state.rootId)
    if (state.relPath) params.set('path', state.relPath)
    if (state.nav === 'items' && state.workbench) {
      params.set('wb', '1')
    }
    if ((state.nav === 'items' || state.nav === 'home') && state.item) {
      params.set('item', state.item)
    }
  }
  const qs = params.toString()
  return `#/${state.domain}/${state.nav}${qs ? `?${qs}` : ''}`
}

export function loadSavedDomain(initialHash: ParsedToolHash): MediaKind {
  if (initialHash.domain) return initialHash.domain
  const saved = localStorage.getItem('xmedia_tool.active_domain') as MediaKind | null
  if (saved && VALID_DOMAINS.includes(saved)) {
    return saved
  }
  return 'book'
}

export function loadSavedNav(initialHash: ParsedToolHash): PrimaryNavKey {
  if (initialHash.nav) return initialHash.nav
  if (initialHash.workbench) return 'items'
  const saved = localStorage.getItem('xmedia_tool.active_nav')
  if (saved === 'home' || saved === 'items') return saved
  return 'home'
}

export function loadSavedSortField(): SortField {
  const saved = localStorage.getItem('xmedia_tool.sort_field')
  if (
    saved === 'name' ||
    saved === 'title' ||
    saved === 'incomplete' ||
    saved === 'size' ||
    saved === 'modified'
  ) {
    return saved
  }
  return 'name'
}

export function loadSavedViewMode(): BrowseViewMode {
  const saved = localStorage.getItem('xmedia_tool.view_mode')
  if (saved === 'folders') return 'folders'
  return 'flat'
}

export function parentRelOf(relPath: string): string {
  const idx = relPath.lastIndexOf('/')
  return idx === -1 ? '' : relPath.slice(0, idx)
}

export function isTypingTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false
  const tag = target.tagName
  return (
    tag === 'INPUT' ||
    tag === 'TEXTAREA' ||
    tag === 'SELECT' ||
    target.isContentEditable
  )
}
