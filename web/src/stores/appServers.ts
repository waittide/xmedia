import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import {
  appHostApi,
  type CreateRemoteSourcePayload,
  type RemoteSourceSummaryDto,
  type TestRemoteSourcePayload,
  type TestRemoteSourceResponse,
  type UpdateRemoteSourcePayload,
} from '@/api/appHost'
import type { AuthSessionDto, MediaKind } from '@/types/api'

export const useAppClientStore = defineStore('appClient', () => {
  const initialized = ref(false)
  const isStandaloneApp = ref(false)
  const appVersion = ref<string | null>(null)
  const enabledModules = ref<MediaKind[]>(['book', 'comic', 'music', 'video'])
  const remoteSourceCount = ref(0)
  const remoteSources = ref<RemoteSourceSummaryDto[]>([])
  const loadingSources = ref(false)

  const opdsSources = computed(() =>
    remoteSources.value.filter((source) => source.protocol === 'opds'),
  )
  const subsonicSources = computed(() =>
    remoteSources.value.filter((source) => source.protocol === 'subsonic'),
  )

  let bootstrapPromise: Promise<void> | null = null
  let autoSessionPromise: Promise<AuthSessionDto> | null = null

  async function bootstrap(): Promise<void> {
    if (initialized.value) return
    if (bootstrapPromise) return bootstrapPromise

    bootstrapPromise = (async () => {
      try {
        const health = await appHostApi.probe()
        if (!health) {
          isStandaloneApp.value = false
          appVersion.value = null
          remoteSourceCount.value = 0
          remoteSources.value = []
          return
        }

        isStandaloneApp.value = true
        appVersion.value = health.version || null
        remoteSourceCount.value = health.remote_source_count
        enabledModules.value = health.enabled_modules
      } finally {
        initialized.value = true
        bootstrapPromise = null
      }
    })()

    return bootstrapPromise
  }

  async function fetchAutoSession(): Promise<AuthSessionDto> {
    if (autoSessionPromise) return autoSessionPromise
    autoSessionPromise = appHostApi.fetchAutoSession().finally(() => {
      autoSessionPromise = null
    })
    return autoSessionPromise
  }

  async function refreshSources(): Promise<RemoteSourceSummaryDto[]> {
    if (!isStandaloneApp.value) return []
    loadingSources.value = true
    try {
      const res = await appHostApi.listRemoteSources()
      remoteSources.value = res.sources
      remoteSourceCount.value = res.sources.length
      return res.sources
    } finally {
      loadingSources.value = false
    }
  }

  async function createSource(
    payload: CreateRemoteSourcePayload,
  ): Promise<RemoteSourceSummaryDto> {
    const res = await appHostApi.createRemoteSource(payload)
    remoteSources.value = res.sources
    remoteSourceCount.value = res.sources.length
    return res.source
  }

  async function updateSource(
    id: string,
    payload: UpdateRemoteSourcePayload,
  ): Promise<RemoteSourceSummaryDto> {
    const res = await appHostApi.updateRemoteSource(id, payload)
    remoteSources.value = res.sources
    remoteSourceCount.value = res.sources.length
    return res.source
  }

  async function deleteSource(id: string): Promise<void> {
    const res = await appHostApi.deleteRemoteSource(id)
    remoteSources.value = res.sources
    remoteSourceCount.value = res.sources.length
  }

  async function testSource(
    payload: TestRemoteSourcePayload,
  ): Promise<TestRemoteSourceResponse> {
    return appHostApi.testRemoteSource(payload)
  }

  return {
    initialized,
    isStandaloneApp,
    appVersion,
    enabledModules,
    remoteSourceCount,
    remoteSources,
    opdsSources,
    subsonicSources,
    loadingSources,
    bootstrap,
    fetchAutoSession,
    refreshSources,
    createSource,
    updateSource,
    deleteSource,
    testSource,
  }
})

export const useAppServersStore = useAppClientStore
