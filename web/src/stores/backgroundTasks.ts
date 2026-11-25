import { computed } from 'vue'
import {
  listDomainPlugins,
  tryGetDomainPlugin,
  type DomainBackgroundDownloadTask,
  type DomainBackgroundTasksProvider,
  type TranslateFn,
} from '@/sources/domainRegistry'
import { useAuthStore } from '@/stores/auth'
import type { MediaKind } from '@/types/api'

function getActiveProviders(onlyEnabledModules = false): Array<{
  kind: MediaKind
  provider: DomainBackgroundTasksProvider
}> {
  const authStore = onlyEnabledModules ? useAuthStore() : null
  const out: Array<{ kind: MediaKind; provider: DomainBackgroundTasksProvider }> = []
  for (const plugin of listDomainPlugins()) {
    if (!plugin.getBackgroundTasks) continue
    if (authStore && !authStore.hasModule(plugin.kind)) continue
    out.push({ kind: plugin.kind, provider: plugin.getBackgroundTasks() })
  }
  return out
}

export function useBackgroundDownloadTasks() {
  const hasProviders = computed(() => {
    const authStore = useAuthStore()
    return listDomainPlugins().some(
      (plugin) => Boolean(plugin.getBackgroundTasks) && authStore.hasModule(plugin.kind),
    )
  })

  const activeCount = computed(() =>
    getActiveProviders(false).reduce((sum, entry) => sum + entry.provider.getActiveCount(), 0),
  )

  const failedCount = computed(() =>
    getActiveProviders(false).reduce((sum, entry) => sum + entry.provider.getFailedCount(), 0),
  )

  const finishedCount = computed(() =>
    getActiveProviders(false).reduce((sum, entry) => sum + entry.provider.getFinishedCount(), 0),
  )

  function getTasks(t: TranslateFn): DomainBackgroundDownloadTask[] {
    const all: DomainBackgroundDownloadTask[] = []
    for (const { provider } of getActiveProviders(false)) {
      all.push(...provider.getTasks(t))
    }
    return all
  }

  async function refreshAll(): Promise<void> {
    const authStore = useAuthStore()
    if (!authStore.isAuthenticated) return
    await Promise.all(getActiveProviders(true).map(({ provider }) => provider.refresh()))
  }

  async function cancelTask(domain: MediaKind, id: string): Promise<void> {
    const provider = tryGetDomainPlugin(domain)?.getBackgroundTasks?.()
    if (provider) {
      await provider.cancelTask(id)
    }
  }

  async function deleteTask(domain: MediaKind, id: string): Promise<void> {
    const provider = tryGetDomainPlugin(domain)?.getBackgroundTasks?.()
    if (provider) {
      await provider.deleteTask(id)
    }
  }

  async function clearFinished(): Promise<void> {
    await Promise.all(
      getActiveProviders(false)
        .filter(({ provider }) => provider.getFinishedCount() > 0)
        .map(({ provider }) => provider.clearFinished()),
    )
  }

  function resetAll(): void {
    for (const { provider } of getActiveProviders(false)) {
      provider.resetState()
    }
  }

  return {
    hasProviders,
    activeCount,
    failedCount,
    finishedCount,
    getTasks,
    refreshAll,
    cancelTask,
    deleteTask,
    clearFinished,
    resetAll,
  }
}
