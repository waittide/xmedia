import {
  computed,
  reactive,
  ref,
  toValue,
  watch,
  type ComputedRef,
  type MaybeRefOrGetter,
  type Ref,
} from 'vue'
import { useI18n } from 'vue-i18n'
import { useRoute, useRouter, type LocationQueryRaw } from 'vue-router'
import { requestJson } from '@/api/client'
import type { OnlineSourceSummaryDto } from '@/components/online-sources/types'
import { useAppClientStore } from '@/stores/appServers'
import type { MediaKind } from '@/types/api'
import { parseSourceFilter } from './routes'
import type { SourceDescriptor, SourceFilter, SourceHealth, SourceScope } from './types'

interface DomainSourceCacheEntry {
  loaded: boolean
  sources: OnlineSourceSummaryDto[]
}

const domainSourceCache = reactive<Record<MediaKind, DomainSourceCacheEntry>>({
  book: { loaded: false, sources: [] },
  comic: { loaded: false, sources: [] },
  music: { loaded: false, sources: [] },
  video: { loaded: false, sources: [] },
})

const inflightRequests = new Map<MediaKind, Promise<OnlineSourceSummaryDto[]>>()

const LOCAL_ONLY_QUERY_KEYS = new Set([
  'library',
  'library_id',
  'collection',
  'shelf',
  'shelf_id',
  'view',
  'author',
  'tag',
  'format',
  'missing',
  'writer',
  'penciller',
  'genre',
  'artist',
  'kind',
  'sort',
  'sort_by',
  'order',
  'sort_order',
])

export function clearSourceFilterCache(domain?: MediaKind): void {
  const targets: MediaKind[] = domain ? [domain] : ['book', 'comic', 'music', 'video']
  for (const d of targets) {
    domainSourceCache[d] = { loaded: false, sources: [] }
    inflightRequests.delete(d)
  }
}

function mapSourceHealth(src: OnlineSourceSummaryDto): SourceHealth {
  if (src.last_test_ok === true) return 'ok'
  if (src.last_test_ok === false) return 'failing'
  return 'untested'
}

/**
 * Fetches current user's isolated online plugins from `GET /api/v1/plugins?domain={domain}`.
 */
async function fetchDomainOnlineSources(
  domain: MediaKind,
  force = false,
): Promise<OnlineSourceSummaryDto[]> {
  if (!force && domainSourceCache[domain].loaded) {
    return domainSourceCache[domain].sources
  }
  const existing = inflightRequests.get(domain)
  if (!force && existing) {
    return existing
  }

  const req = requestJson<OnlineSourceSummaryDto[]>(
    `/plugins?domain=${encodeURIComponent(domain)}`,
  )
    .then((list) => {
      domainSourceCache[domain] = { loaded: true, sources: list }
      return list
    })
    .finally(() => {
      if (inflightRequests.get(domain) === req) {
        inflightRequests.delete(domain)
      }
    })

  inflightRequests.set(domain, req)
  return req
}

export interface UseSourceFilterOptions {
  /** Automatically fetch online/remote sources when domain changes. Default: `true`. */
  autoLoad?: boolean
}

export interface UseSourceFilterReturn {
  /** URL-backed read-only source filter (`scope` + `sourceId`). */
  filter: ComputedRef<SourceFilter>
  /** Combined descriptors: local + enabled online sources + remote sources (standalone only). */
  sources: ComputedRef<SourceDescriptor[]>
  /** Enabled online source descriptors for the active domain. */
  onlineSources: ComputedRef<SourceDescriptor[]>
  /** Remote OPDS/Subsonic source descriptors for the active domain (standalone app only). */
  remoteSources: ComputedRef<SourceDescriptor[]>
  /** Active source descriptor, or `null` when browsing all sources within online/remote scope. */
  current: ComputedRef<SourceDescriptor | null>
  hasOnlineSources: ComputedRef<boolean>
  hasRemoteSources: ComputedRef<boolean>
  loading: Ref<boolean>
  error: Ref<string | null>
  setScope: (scope: SourceScope) => Promise<void>
  setSource: (id: string | null) => Promise<void>
  refresh: (force?: boolean) => Promise<void>
}

export function useSourceFilter(
  domain: MaybeRefOrGetter<MediaKind>,
  options: UseSourceFilterOptions = {},
): UseSourceFilterReturn {
  const autoLoad = options.autoLoad ?? true
  const route = useRoute()
  const router = useRouter()
  const appClientStore = useAppClientStore()
  const { locale } = useI18n()

  const loading = ref(false)
  const error = ref<string | null>(null)
  let loadSeq = 0

  const activeDomain = computed<MediaKind>(() => toValue(domain))
  const filter = computed<SourceFilter>(() => parseSourceFilter(route.query))

  const localDescriptor = computed<SourceDescriptor>(() => ({
    scope: 'local',
    id: 'local',
    domain: activeDomain.value,
    name: locale.value === 'zh-CN' ? '本机资源' : 'Local Resources',
    enabled: true,
    health: 'ok',
    capabilities: {
      search: true,
      explore: false,
      track: true,
      import: false,
    },
  }))

  const onlineSources = computed<SourceDescriptor[]>(() => {
    const d = activeDomain.value
    return domainSourceCache[d].sources
      .filter((src) => src.enabled)
      .map((src) => ({
        scope: 'online' as const,
        id: src.id,
        domain: d,
        name: src.name,
        enabled: src.enabled,
        health: mapSourceHealth(src),
        protocol: 'rule' as const,
        capabilities: {
          search: true,
          explore: d !== 'book',
          track: true,
          import: d === 'book',
        },
      }))
  })

  const remoteSources = computed<SourceDescriptor[]>(() => {
    if (!appClientStore.isStandaloneApp) return []
    const d = activeDomain.value
    if (d === 'video') return []

    const list = d === 'music' ? appClientStore.subsonicSources : appClientStore.opdsSources
    return list.map((src) => ({
      scope: 'remote' as const,
      id: src.id,
      domain: d,
      name: src.name || src.url,
      enabled: true,
      health: 'untested' as const,
      protocol: src.protocol,
      capabilities: {
        search: true,
        explore: true,
        track: true,
        import: true,
      },
    }))
  })

  const sources = computed<SourceDescriptor[]>(() => [
    localDescriptor.value,
    ...onlineSources.value,
    ...remoteSources.value,
  ])

  const current = computed<SourceDescriptor | null>(() => {
    const f = filter.value
    if (f.scope === 'local') {
      return localDescriptor.value
    }
    if (f.scope === 'pool') {
      return null
    }
    if (!f.sourceId) {
      return null
    }
    return sources.value.find((s) => s.scope === f.scope && s.id === f.sourceId) ?? null
  })

  const hasOnlineSources = computed(() => onlineSources.value.length > 0)
  const hasRemoteSources = computed(() => remoteSources.value.length > 0)

  async function refresh(force = true): Promise<void> {
    const targetDomain = activeDomain.value
    const seq = ++loadSeq
    loading.value = true
    error.value = null
    try {
      const tasks: Promise<unknown>[] = [fetchDomainOnlineSources(targetDomain, force)]
      if (
        appClientStore.isStandaloneApp &&
        targetDomain !== 'video' &&
        (force || (appClientStore.remoteSources.length === 0 && appClientStore.remoteSourceCount > 0))
      ) {
        tasks.push(appClientStore.refreshSources().catch(() => []))
      }
      await Promise.all(tasks)
    } catch (err) {
      if (seq === loadSeq) {
        error.value = err instanceof Error ? err.message : String(err)
      }
    } finally {
      if (seq === loadSeq) {
        loading.value = false
      }
    }
  }

  async function setScope(scope: SourceScope): Promise<void> {
    const nextQuery: LocationQueryRaw = {}

    // Preserve q; preserve continue/favorites tab only for local scope; drop source/page.
    if (typeof route.query.q === 'string' && route.query.q.trim()) {
      nextQuery.q = route.query.q
    }
    if (
      scope === 'local' &&
      typeof route.query.tab === 'string' &&
      (route.query.tab === 'continue' || route.query.tab === 'favorites')
    ) {
      nextQuery.tab = route.query.tab
    }

    if (scope === 'local') {
      // Retain local query parameters if already local, always dropping scope/source/page.
      for (const [k, v] of Object.entries(route.query)) {
        if (
          k === 'scope' ||
          k === 'source' ||
          k === 'source_id' ||
          k === 'sourceId' ||
          k === 'page'
        ) {
          continue
        }
        if (v !== undefined && v !== null) {
          nextQuery[k] = v
        }
      }
    } else {
      nextQuery.scope = scope
      for (const [k, v] of Object.entries(route.query)) {
        if (
          k === 'scope' ||
          k === 'source' ||
          k === 'source_id' ||
          k === 'sourceId' ||
          k === 'page' ||
          k === 'q' ||
          k === 'tab' ||
          LOCAL_ONLY_QUERY_KEYS.has(k)
        ) {
          continue
        }
        if (v !== undefined && v !== null) {
          nextQuery[k] = v
        }
      }
    }

    await router.replace({ path: route.path, query: nextQuery })
  }

  async function setSource(id: string | null): Promise<void> {
    const trimmed = id?.trim() || null
    if (trimmed === 'local') {
      await setScope('local')
      return
    }

    let nextScope: SourceScope = filter.value.scope
    if (trimmed) {
      const matched = sources.value.find((s) => s.id === trimmed && s.scope !== 'local')
      if (matched) {
        nextScope = matched.scope
      } else if (nextScope === 'local') {
        nextScope = 'online'
      }
    }

    const nextQuery: LocationQueryRaw = {}
    for (const [k, v] of Object.entries(route.query)) {
      if (
        k === 'source' ||
        k === 'source_id' ||
        k === 'sourceId' ||
        k === 'page' ||
        (nextScope !== 'local' && LOCAL_ONLY_QUERY_KEYS.has(k))
      ) {
        continue
      }
      if (v !== undefined && v !== null) {
        nextQuery[k] = v
      }
    }

    if (nextScope === 'local') {
      delete nextQuery.scope
    } else {
      nextQuery.scope = nextScope
      if (trimmed) {
        nextQuery.source = trimmed
      }
    }

    await router.replace({ path: route.path, query: nextQuery })
  }

  if (autoLoad) {
    watch(
      activeDomain,
      () => {
        void refresh(false)
      },
      { immediate: true },
    )
  }

  return {
    filter,
    sources,
    onlineSources,
    remoteSources,
    current,
    hasOnlineSources,
    hasRemoteSources,
    loading,
    error,
    setScope,
    setSource,
    refresh,
  }
}
