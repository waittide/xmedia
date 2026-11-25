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
import { useAuthStore } from '@/stores/auth'
import { useDomainStore } from '@/stores/domain'
import type { MediaKind } from '@/types/api'
import { ALL_DOMAIN_KINDS, getDomainPlugin } from './domainRegistry'
import { libraryRoute } from './routes'
import type { OnlineQuickAction, SearchGroup } from './types'

export const SEARCH_RECENT_STORAGE_KEY = 'xmedia.search.recent'
const MAX_RECENT_SEARCHES = 8
const DEFAULT_LIMIT_PER_GROUP = 5
const DEFAULT_DEBOUNCE_MS = 250

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
    localStorage.setItem(
      SEARCH_RECENT_STORAGE_KEY,
      JSON.stringify(list.slice(0, MAX_RECENT_SEARCHES)),
    )
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

  const preferredDomain = computed<MediaKind>(() =>
    options.preferDomain !== undefined ? toValue(options.preferDomain) : domainStore.activeDomain,
  )

  const orderedEnabledDomains = computed<MediaKind[]>(() => {
    const enabled = authStore.enabledModules
    const prefer = preferredDomain.value
    const ordered: MediaKind[] = []
    if (enabled.includes(prefer)) ordered.push(prefer)
    for (const d of ALL_DOMAIN_KINDS) {
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

  const onlineActions = computed<OnlineQuickAction[]>(() => {
    const q = query.value.trim()
    if (!q) return []
    const actions: OnlineQuickAction[] = []
    for (const domain of ALL_DOMAIN_KINDS) {
      if (!authStore.hasModule(domain)) continue
      const meta = getDomainPlugin(domain).onlineSearchI18n
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
          const res = await getDomainPlugin(domain).searchLocal({
            query: trimmed,
            limit: limitPerGroup,
            signal,
            t: (key, named) => (named ? t(key, named) : t(key)),
          })
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
