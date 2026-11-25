import {
  computed,
  ref,
  toValue,
  watch,
  type ComputedRef,
  type MaybeRefOrGetter,
  type Ref,
} from 'vue'
import type { MediaKind } from '@/types/api'
import { getDomainPlugin, type MergedShelfView } from './domainRegistry'
import type { MediaItemVM, SourceScope } from './types'

export type { MergedShelfView } from './domainRegistry'

export interface UseMergedShelfOptions {
  limit?: number
  /** Filter by source scope. Default: `'all'` (reads bound local library items). */
  scope?: MaybeRefOrGetter<SourceScope | 'all'>
  /** Unused; kept for signature compatibility. */
  sourceId?: MaybeRefOrGetter<string | null>
  /** Optional client-side keyword filter. */
  query?: MaybeRefOrGetter<string>
  /** Automatically fetch when inputs change. Default: `true`. */
  autoLoad?: boolean
}

export interface UseMergedShelfReturn {
  items: ComputedRef<MediaItemVM[]>
  loading: Ref<boolean>
  error: Ref<string | null>
  refresh: () => Promise<void>
}

export function parseTimestampMs(raw: number | string | null | undefined): number {
  if (raw === null || raw === undefined) return 0
  if (typeof raw === 'number') {
    if (!Number.isFinite(raw) || raw <= 0) return 0
    return raw < 1e12 ? raw * 1000 : raw
  }
  const trimmed = raw.trim()
  if (!trimmed) return 0
  if (/^\d+$/.test(trimmed)) {
    const num = Number(trimmed)
    if (!Number.isFinite(num) || num <= 0) return 0
    return num < 1e12 ? num * 1000 : num
  }
  const parsed = Date.parse(trimmed)
  return Number.isFinite(parsed) ? parsed : 0
}

export function useMergedShelf(
  domain: MaybeRefOrGetter<MediaKind>,
  view: MaybeRefOrGetter<MergedShelfView>,
  options: UseMergedShelfOptions = {},
): UseMergedShelfReturn {
  const autoLoad = options.autoLoad ?? true
  const limit = options.limit ?? 50

  const rawItems = ref<MediaItemVM[]>([])
  const loading = ref(false)
  const error = ref<string | null>(null)
  let reqSeq = 0

  const activeDomain = computed(() => toValue(domain))
  const activeView = computed(() => toValue(view))
  const activeScope = computed<SourceScope | 'all'>(() => toValue(options.scope) ?? 'all')
  const activeQuery = computed<string>(() => toValue(options.query)?.trim() ?? '')

  const items = computed<MediaItemVM[]>(() => rawItems.value)

  async function refresh(): Promise<void> {
    const d = activeDomain.value
    const v = activeView.value
    const scope = activeScope.value
    const q = activeQuery.value
    const seq = ++reqSeq

    if (scope !== 'all' && scope !== 'local') {
      rawItems.value = []
      error.value = null
      loading.value = false
      return
    }

    loading.value = true
    error.value = null

    try {
      const localItems = await getDomainPlugin(d).fetchLocalShelfItems(v, limit)
      if (seq !== reqSeq) return

      const needle = q.toLowerCase()
      const filtered = needle
        ? localItems.filter(
            (m) =>
              m.title.toLowerCase().includes(needle) ||
              (m.subtitle?.toLowerCase().includes(needle) ?? false),
          )
        : localItems

      filtered.sort((a, b) => (b.sortTimestamp ?? 0) - (a.sortTimestamp ?? 0))
      rawItems.value = limit > 0 ? filtered.slice(0, limit) : filtered
    } catch (err) {
      if (seq !== reqSeq) return
      error.value = err instanceof Error ? err.message : String(err)
    } finally {
      if (seq === reqSeq) {
        loading.value = false
      }
    }
  }

  if (autoLoad) {
    watch(
      [activeDomain, activeView, activeScope, activeQuery],
      () => {
        void refresh()
      },
      { immediate: true },
    )
  }

  return {
    items,
    loading,
    error,
    refresh,
  }
}
