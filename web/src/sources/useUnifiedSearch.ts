import {
  computed,
  onScopeDispose,
  ref,
  toValue,
  watch,
  type ComputedRef,
  type MaybeRefOrGetter,
  type Ref,
} from 'vue'
import { useI18n } from 'vue-i18n'
import { requestJson, withAuthToken } from '@/api/client'
import { api as bookApi } from '@/domains/book/api'
import type { BookSummaryDto, PagedResult as BookPagedResult } from '@/domains/book/types'
import type { SeriesSummaryDto } from '@/domains/comic/types'
import { buildCoverUrl as buildMusicCoverUrl } from '@/domains/music/api'
import type { SearchResultDto as MusicSearchResultDto } from '@/domains/music/types'
import { buildPosterUrl as buildVideoPosterUrl } from '@/domains/video/api'
import type { VideoSummaryDto } from '@/domains/video/types'
import { useAuthStore } from '@/stores/auth'
import { useDomainStore } from '@/stores/domain'
import type { MediaKind } from '@/types/api'
import { itemRoute, libraryRoute, playRoute } from './routes'
import type { MediaItemVM, OnlineQuickAction, SearchGroup } from './types'

export const SEARCH_RECENT_STORAGE_KEY = 'xmedia.search.recent'
const MAX_RECENT_SEARCHES = 8
const DEFAULT_LIMIT_PER_GROUP = 5
const DEFAULT_DEBOUNCE_MS = 250
const ALL_DOMAINS: readonly MediaKind[] = ['book', 'comic', 'music', 'video']

const ONLINE_SEARCH_I18N: Record<MediaKind, { titleKey: string; subtitleKey: string }> = {
  book: {
    titleKey: 'globalSearch.onlineBookSearchTitle',
    subtitleKey: 'globalSearch.onlineBookSearchSubtitle',
  },
  comic: {
    titleKey: 'globalSearch.onlineComicSearchTitle',
    subtitleKey: 'globalSearch.onlineComicSearchSubtitle',
  },
  music: {
    titleKey: 'globalSearch.onlineMusicSearchTitle',
    subtitleKey: 'globalSearch.onlineMusicSearchSubtitle',
  },
  video: {
    titleKey: 'globalSearch.onlineVideoSearchTitle',
    subtitleKey: 'globalSearch.onlineVideoSearchSubtitle',
  },
}

function readStoredRecent(): string[] {
  try {
    const raw = localStorage.getItem(SEARCH_RECENT_STORAGE_KEY)
    if (!raw) return []
    const parsed: unknown = JSON.parse(raw)
    if (!Array.isArray(parsed)) return []
    return parsed
      .filter((v): v is string => typeof v === 'string' && v.trim().length > 0)
      .map((v) => v.trim())
      .slice(0, MAX_RECENT_SEARCHES)
  } catch {
    return []
  }
}

function writeStoredRecent(list: readonly string[]): void {
  try {
    localStorage.setItem(SEARCH_RECENT_STORAGE_KEY, JSON.stringify(list.slice(0, MAX_RECENT_SEARCHES)))
  } catch {
    // ignore storage quota / private mode errors
  }
}

function isAbortError(err: unknown): boolean {
  return err instanceof DOMException
    ? err.name === 'AbortError'
    : err instanceof Error && err.name === 'AbortError'
}

export interface UseUnifiedSearchOptions {
  /** Domain placed first in result groups (defaults to `domainStore.activeDomain`). */
  preferDomain?: MaybeRefOrGetter<MediaKind>
  /** Max items per local domain group. Default: `5`. */
  limitPerGroup?: number
  /** Debounce delay in milliseconds when `query` changes. Default: `250`. */
  debounceMs?: number
}

export interface UseUnifiedSearchReturn {
  query: Ref<string>
  /** Local search groups across enabled domains (online is never auto-fetched). */
  groups: ComputedRef<SearchGroup[]>
  /** 4-domain online quick actions; strictly `[]` when `query.trim()` is empty (T2-F13-03). */
  onlineActions: ComputedRef<OnlineQuickAction[]>
  /** Recent search terms persisted in `localStorage` (`xmedia.search.recent`, max 8). */
  recent: Ref<string[]>
  loading: ComputedRef<boolean>
  hasQuery: ComputedRef<boolean>
  error: ComputedRef<string | null>
  run: (q?: string) => void
  cancel: () => void
  addRecent: (term: string) => void
  removeRecent: (term: string) => void
  clearRecent: () => void
}

export function useUnifiedSearch(
  options: UseUnifiedSearchOptions = {},
): UseUnifiedSearchReturn {
  const limitPerGroup = options.limitPerGroup ?? DEFAULT_LIMIT_PER_GROUP
  const debounceMs = options.debounceMs ?? DEFAULT_DEBOUNCE_MS

  const { t } = useI18n()
  const authStore = useAuthStore()
  const domainStore = useDomainStore()

  const query = ref('')
  const recent = ref<string[]>(readStoredRecent())
  const groupMap = ref<Partial<Record<MediaKind, SearchGroup>>>({})
  const pendingDebounce = ref(false)

  let debounceTimer: ReturnType<typeof setTimeout> | null = null
  let activeController: AbortController | null = null
  let searchSeq = 0

  const preferredDomain = computed<MediaKind>(
    () => (options.preferDomain !== undefined ? toValue(options.preferDomain) : domainStore.activeDomain),
  )

  const orderedEnabledDomains = computed<MediaKind[]>(() => {
    const enabled = authStore.enabledModules
    const prefer = preferredDomain.value
    const ordered: MediaKind[] = []
    if (enabled.includes(prefer)) ordered.push(prefer)
    for (const d of ALL_DOMAINS) {
      if (enabled.includes(d) && !ordered.includes(d)) {
        ordered.push(d)
      }
    }
    return ordered
  })

  const hasQuery = computed(() => query.value.trim().length > 0)

  const groups = computed<SearchGroup[]>(() => {
    const out: SearchGroup[] = []
    for (const domain of orderedEnabledDomains.value) {
      const g = groupMap.value[domain]
      if (g) out.push(g)
    }
    return out
  })

  const loading = computed(
    () => pendingDebounce.value || groups.value.some((g) => g.status === 'loading'),
  )

  const error = computed<string | null>(() => {
    const failed = groups.value.find((g) => g.status === 'error' && g.error)
    return failed?.error ?? null
  })

  /**
   * Online search quick actions for all enabled domains.
   * Returns `[]` when trimmed query is empty (satisfies E2E T1-F13-04 & T2-F13-03).
   */
  const onlineActions = computed<OnlineQuickAction[]>(() => {
    const q = query.value.trim()
    if (!q) return []
    const actions: OnlineQuickAction[] = []
    for (const domain of ALL_DOMAINS) {
      if (!authStore.hasModule(domain)) continue
      const meta = ONLINE_SEARCH_I18N[domain]
      actions.push({
        domain,
        label: t(meta.titleKey, { query: q }),
        subtitle: t(meta.subtitleKey),
        to: libraryRoute(domain, { scope: 'online', q }),
      })
    }
    return actions
  })

  function clearDebounce(): void {
    if (debounceTimer !== null) {
      clearTimeout(debounceTimer)
      debounceTimer = null
    }
    pendingDebounce.value = false
  }

  function cancel(): void {
    clearDebounce()
    if (activeController) {
      activeController.abort()
      activeController = null
    }
    searchSeq += 1
  }

  function addRecent(term: string): void {
    const trimmed = term.trim()
    if (!trimmed) return
    const next = [trimmed, ...recent.value.filter((item) => item !== trimmed)].slice(
      0,
      MAX_RECENT_SEARCHES,
    )
    recent.value = next
    writeStoredRecent(next)
  }

  function removeRecent(term: string): void {
    const next = recent.value.filter((item) => item !== term)
    recent.value = next
    writeStoredRecent(next)
  }

  function clearRecent(): void {
    recent.value = []
    writeStoredRecent([])
  }

  async function searchLocalDomain(
    domain: MediaKind,
    q: string,
    signal: AbortSignal,
  ): Promise<{ items: MediaItemVM[]; total?: number }> {
    if (domain === 'book') {
      const sp = new URLSearchParams({
        keyword: q,
        page: '1',
        page_size: String(limitPerGroup),
      })
      const res = await requestJson<BookPagedResult<BookSummaryDto>>(`/book/books?${sp.toString()}`, {
        signal,
      })
      const items: MediaItemVM[] = res.items.slice(0, limitPerGroup).map((b) => {
        const ref = { scope: 'local' as const, domain: 'book' as const, id: b.id }
        return {
          ref,
          title: b.metadata.title || b.id,
          subtitle: b.metadata.authors.join(', ') || b.format.toUpperCase(),
          cover: b.has_cover ? bookApi.coverUrl(b.id, b.updated_at) : undefined,
          badge: { sourceKind: b.online_tracking ? 'online' : 'local' },
          to: itemRoute(ref),
          playTo: playRoute(ref),
        }
      })
      return { items, total: res.total }
    }

    if (domain === 'comic') {
      const sp = new URLSearchParams({
        search: q,
        limit: String(limitPerGroup),
      })
      const res = await requestJson<SeriesSummaryDto[]>(`/comic/series?${sp.toString()}`, {
        signal,
      })
      const items: MediaItemVM[] = res.slice(0, limitPerGroup).map((s) => {
        const ref = { scope: 'local' as const, domain: 'comic' as const, id: s.id }
        return {
          ref,
          title: s.title,
          subtitle: s.writers?.join(', ') || `${s.chapter_count} ch`,
          cover: s.cover_url ? withAuthToken(s.cover_url) : undefined,
          badge: { sourceKind: 'local' },
          to: itemRoute(ref),
        }
      })
      return { items, total: res.length }
    }

    if (domain === 'music') {
      const sp = new URLSearchParams({
        q,
        limit: String(limitPerGroup),
      })
      const res = await requestJson<MusicSearchResultDto>(`/music/search?${sp.toString()}`, {
        signal,
      })
      const items: MediaItemVM[] = []
      const albumCap = Math.min(2, limitPerGroup)
      for (const alb of res.albums.slice(0, albumCap)) {
        const ref = { scope: 'local' as const, domain: 'music' as const, id: alb.id }
        items.push({
          ref,
          title: alb.title,
          subtitle: alb.album_artist_name || t('app.domains.music'),
          cover: buildMusicCoverUrl(alb.cover_key) ?? undefined,
          badge: { sourceKind: 'local' },
          to: itemRoute(ref),
        })
      }
      const trackCap = Math.max(1, limitPerGroup - items.length)
      for (const tr of res.tracks.slice(0, trackCap)) {
        const ref = { scope: 'local' as const, domain: 'music' as const, id: tr.id }
        items.push({
          ref,
          title: tr.title,
          subtitle: [tr.artist_name, tr.album_title].filter(Boolean).join(' · ') || undefined,
          cover: buildMusicCoverUrl(tr.cover_key) ?? undefined,
          badge: { sourceKind: 'local' },
          to: tr.album_id
            ? itemRoute({ scope: 'local', domain: 'music', id: tr.album_id })
            : libraryRoute('music', { view: 'tracks', q }),
        })
      }
      return { items, total: res.albums.length + res.tracks.length }
    }

    // video
    const sp = new URLSearchParams({
      search: q,
      limit: String(limitPerGroup),
    })
    const res = await requestJson<VideoSummaryDto[]>(`/video/videos?${sp.toString()}`, { signal })
    const items: MediaItemVM[] = res.slice(0, limitPerGroup).map((v) => {
      const ref = { scope: 'local' as const, domain: 'video' as const, id: v.id }
      const to = v.show_id
        ? itemRoute({ scope: 'local', domain: 'video', id: v.show_id })
        : playRoute(ref)
      return {
        ref,
        title: v.title,
        subtitle: v.year ? String(v.year) : t('app.domains.video'),
        cover: buildVideoPosterUrl(v.poster_key) ?? undefined,
        badge: { sourceKind: 'local' },
        to,
        playTo: playRoute(ref),
      }
    })
    return { items, total: res.length }
  }

  function executeSearch(q: string): void {
    cancel()
    const trimmed = q.trim()
    if (!trimmed) {
      groupMap.value = {}
      return
    }

    const seq = ++searchSeq
    const controller = new AbortController()
    activeController = controller
    const { signal } = controller

    const domains = orderedEnabledDomains.value
    const initial: Partial<Record<MediaKind, SearchGroup>> = {}
    for (const domain of domains) {
      initial[domain] = {
        key: `local:${domain}`,
        domain,
        scope: 'local',
        title: t(`app.domains.${domain}`),
        status: 'loading',
        items: [],
        seeAll: libraryRoute(domain, { q: trimmed }),
      }
    }
    groupMap.value = initial

    void Promise.all(
      domains.map(async (domain) => {
        try {
          const res = await searchLocalDomain(domain, trimmed, signal)
          if (signal.aborted || seq !== searchSeq) return
          groupMap.value = {
            ...groupMap.value,
            [domain]: {
              key: `local:${domain}`,
              domain,
              scope: 'local',
              title: t(`app.domains.${domain}`),
              status: 'ok',
              items: res.items,
              total: res.total,
              seeAll: libraryRoute(domain, { q: trimmed }),
            },
          }
        } catch (err) {
          if (signal.aborted || isAbortError(err) || seq !== searchSeq) return
          groupMap.value = {
            ...groupMap.value,
            [domain]: {
              key: `local:${domain}`,
              domain,
              scope: 'local',
              title: t(`app.domains.${domain}`),
              status: 'error',
              error: err instanceof Error ? err.message : String(err),
              items: [],
              seeAll: libraryRoute(domain, { q: trimmed }),
            },
          }
        }
      }),
    ).finally(() => {
      if (activeController === controller) {
        activeController = null
      }
    })
  }

  function run(explicitQuery?: string): void {
    if (explicitQuery !== undefined) {
      query.value = explicitQuery
    }
    clearDebounce()
    executeSearch(query.value)
  }

  watch(query, (val) => {
    clearDebounce()
    const trimmed = val.trim()
    if (!trimmed) {
      cancel()
      groupMap.value = {}
      return
    }
    pendingDebounce.value = true
    debounceTimer = setTimeout(() => {
      debounceTimer = null
      pendingDebounce.value = false
      executeSearch(trimmed)
    }, debounceMs)
  })

  onScopeDispose(() => {
    cancel()
  })

  return {
    query,
    groups,
    onlineActions,
    recent,
    loading,
    hasQuery,
    error,
    run,
    cancel,
    addRecent,
    removeRecent,
    clearRecent,
  }
}
