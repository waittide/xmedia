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

  let viewActive = true
  let dirtyWhileInactive = false

  async function loadHome(force = false): Promise<void> {
    if (!overview.value) loading.value = true
    try {
      overview.value = await fetchHomeOverview(domain, { force })
    } finally {
      loading.value = false
    }
  }

  async function refreshPage(): Promise<void> {
    await Promise.all([loadHome(true), options.refreshSidebar()])
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
    loadHome,
    refreshPage,
  }
}
