import {
  onActivated,
  onDeactivated,
  onMounted,
  ref,
  watch,
  type Ref,
  type WatchSource,
} from 'vue'
import { fetchHomeOverview, peekHomeCache, type DomainHomeMap } from '@/config/homeCache'
import type { MediaKind } from '@/types/api'

export interface UseHomeOverviewOptions {
  refreshSidebar: () => Promise<unknown>
  watchSources: WatchSource<unknown> | WatchSource<unknown>[]
}

export function useHomeOverview<K extends MediaKind>(
  domain: K,
  options: UseHomeOverviewOptions,
) {
  const overview = ref(peekHomeCache(domain)) as Ref<DomainHomeMap[K] | null>
  const loading = ref<boolean>(overview.value === null)
  const error = ref<string | null>(null)

  let viewActive = true
  let dirtyWhileInactive = false
  let loadSeq = 0

  async function loadHome(force = false): Promise<void> {
    const seq = ++loadSeq
    if (!overview.value) loading.value = true
    error.value = null
    try {
      const data = await fetchHomeOverview(domain, { force })
      if (seq !== loadSeq) return
      overview.value = data
    } catch (err) {
      if (seq !== loadSeq) return
      error.value = err instanceof Error ? err.message : String(err)
    } finally {
      if (seq === loadSeq) {
        loading.value = false
      }
    }
  }

  async function refreshPage(): Promise<void> {
    await Promise.allSettled([loadHome(true), options.refreshSidebar()])
  }

  onMounted(() => {
    viewActive = true
    void loadHome(false)
  })

  onActivated(() => {
    viewActive = true
    if (dirtyWhileInactive) {
      dirtyWhileInactive = false
      void loadHome(true)
    } else {
      void loadHome(false)
    }
  })

  onDeactivated(() => {
    viewActive = false
  })

  watch(options.watchSources, () => {
    if (!viewActive) {
      dirtyWhileInactive = true
      return
    }
    void loadHome(true)
  })

  return {
    overview,
    loading,
    error,
    loadHome,
    refreshPage,
  }
}
