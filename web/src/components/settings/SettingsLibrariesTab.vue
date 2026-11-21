<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { LuLibrary, LuRefreshCw, LuTrash2 } from 'vue-icons-plus/lu'
import { api } from '@/api/client'
import { useAuthStore } from '@/stores/auth'
import { useDialogStore } from '@/stores/dialog'
import { useDomainStore } from '@/stores/domain'
import { useEventsStore } from '@/stores/events'
import type { CoreLibraryDto, MediaKind } from '@/types/api'
import AppSelect from '@/components/ui/AppSelect.vue'
import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import RefreshButton from '@/components/ui/RefreshButton.vue'

const props = defineProps<{
  domain?: MediaKind
}>()

const emit = defineEmits<{
  error: [message: string]
}>()

const { t } = useI18n()
const authStore = useAuthStore()
const dialogStore = useDialogStore()
const domainStore = useDomainStore()
const eventsStore = useEventsStore()

const allLibraries = ref<CoreLibraryDto[]>([])
const libraryFilterKind = ref<'all' | MediaKind>(props.domain ?? 'all')
const libKind = ref<MediaKind>(props.domain ?? domainStore.activeDomain)
const libName = ref('')
const libPath = ref('')
const scanNow = ref(true)
const mounting = ref(false)
const scanningId = ref<string | null>(null)
const scanningAll = ref(false)

watch(
  () => props.domain,
  (next) => {
    if (next) {
      libKind.value = next
      libraryFilterKind.value = next
    }
  },
)

const effectiveFilterKind = computed<'all' | MediaKind>(() => props.domain ?? libraryFilterKind.value)
const effectiveMountKind = computed<MediaKind>(() => props.domain ?? libKind.value)

const domainSelectOptions = computed(() =>
  authStore.enabledModules.map((k) => ({
    value: k,
    label: t(`app.domains.${k}`),
  })),
)

const libraryFilterOptions = computed(() => [
  { value: 'all', label: t('admin.libraryFilterAll') },
  ...domainSelectOptions.value,
])

const filteredLibraries = computed(() => {
  if (effectiveFilterKind.value === 'all') return allLibraries.value
  return allLibraries.value.filter((l) => l.media_kind === effectiveFilterKind.value)
})

async function refreshAdminLibraries(): Promise<void> {
  allLibraries.value = await api.listLibraries()
}

onMounted(() => {
  void refreshAdminLibraries().catch((err) => {
    emit('error', err instanceof Error ? err.message : String(err))
  })
})

watch(
  () => [
    eventsStore.domainRefreshTick.book,
    eventsStore.domainRefreshTick.comic,
    eventsStore.domainRefreshTick.music,
    eventsStore.domainRefreshTick.video,
  ],
  () => {
    void refreshAdminLibraries().catch(() => {})
  },
)

async function handleMountLibrary(): Promise<void> {
  emit('error', '')
  const trimmedName = libName.value.trim()
  const trimmedPath = libPath.value.trim()
  if (!trimmedName || !trimmedPath) {
    emit('error', t('admin.libraryPathPlaceholder'))
    return
  }
  mounting.value = true
  try {
    const created = await api.createLibrary({
      name: trimmedName,
      media_kind: effectiveMountKind.value,
      root_path: trimmedPath,
    })
    if (scanNow.value) {
      await api.scanLibrary(created.id)
    }
    libName.value = ''
    libPath.value = ''
    eventsStore.bumpDomain(created.media_kind)
    await refreshAdminLibraries()
  } catch (err) {
    emit('error', err instanceof Error ? err.message : String(err))
  } finally {
    mounting.value = false
  }
}

async function handleRescan(lib: CoreLibraryDto): Promise<void> {
  emit('error', '')
  scanningId.value = lib.id
  try {
    await Promise.all([
      (async () => {
        await api.scanLibrary(lib.id)
        eventsStore.bumpDomain(lib.media_kind)
        await refreshAdminLibraries()
      })(),
      new Promise((resolve) => setTimeout(resolve, 450)),
    ])
  } catch (err) {
    emit('error', err instanceof Error ? err.message : String(err))
  } finally {
    scanningId.value = null
  }
}

async function handleScanAll(): Promise<void> {
  emit('error', '')
  scanningAll.value = true
  try {
    const targets = filteredLibraries.value.filter(
      (lib) => lib.kind.type === 'local_mount' && lib.scan_status.state !== 'scanning',
    )
    const [results] = await Promise.all([
      Promise.allSettled(
        targets.map(async (lib) => {
          await api.scanLibrary(lib.id)
          eventsStore.bumpDomain(lib.media_kind)
        }),
      ),
      new Promise((resolve) => setTimeout(resolve, 450)),
    ])
    const firstErr = results.find((r): r is PromiseRejectedResult => r.status === 'rejected')
    if (firstErr) {
      emit(
        'error',
        firstErr.reason instanceof Error ? firstErr.reason.message : String(firstErr.reason),
      )
    }
    await refreshAdminLibraries()
  } catch (err) {
    emit('error', err instanceof Error ? err.message : String(err))
  } finally {
    scanningAll.value = false
  }
}

async function handleDeleteLibrary(lib: CoreLibraryDto): Promise<void> {
  const confirmed = await dialogStore.confirm({
    title: t('admin.deleteLibrary'),
    message: t('admin.confirmDeleteLibrary', { name: lib.name }),
    danger: true,
  })
  if (!confirmed) return
  emit('error', '')
  try {
    await api.deleteLibrary(lib.id)
    eventsStore.bumpDomain(lib.media_kind)
    await refreshAdminLibraries()
  } catch (err) {
    emit('error', err instanceof Error ? err.message : String(err))
  }
}

function formatScanStatus(lib: CoreLibraryDto): string {
  const s = lib.scan_status
  if (s.state === 'scanning') {
    if (s.total && s.total > 0) {
      const pct = Math.min(100, Math.round((s.scanned / s.total) * 100))
      return t('admin.scanningWithTotal', { scanned: s.scanned, total: s.total, pct })
    }
    return t('admin.scanning', { scanned: s.scanned })
  }
  if (s.state === 'completed') {
    return t('admin.scanCompleted', {
      scanned: s.scanned,
      added: s.added,
      updated: s.updated,
      missing: s.missing,
    })
  }
  if (s.state === 'failed') return t('admin.scanFailed', { message: s.message })
  return t('admin.scanIdle')
}
</script>

<template>
  <section class="md-card-outlined p-6">
    <h2 class="md-title-small text-[var(--md-sys-color-on-surface)]">
      {{ t('admin.mountLibraryTitle') }}
    </h2>
    <p class="md-body-small mt-1 text-[var(--md-sys-color-on-surface-variant)]">
      {{ t('admin.mountLibraryDesc') }}
    </p>

    <form class="mt-4 space-y-4" @submit.prevent="handleMountLibrary">
      <div
        class="grid grid-cols-1 gap-4"
        :class="domain ? 'sm:grid-cols-2' : 'sm:grid-cols-3'"
      >
        <div v-if="!domain">
          <label class="md-label-medium mb-1.5 block text-[var(--md-sys-color-on-surface-variant)]">
            {{ t('admin.libraryKind') }}
          </label>
          <AppSelect
            :model-value="libKind"
            :options="domainSelectOptions"
            @update:model-value="libKind = $event as MediaKind"
          />
        </div>

        <div>
          <label class="md-label-medium mb-1.5 block text-[var(--md-sys-color-on-surface-variant)]">
            {{ t('admin.libraryName') }}
          </label>
          <input
            v-model="libName"
            type="text"
            required
            :placeholder="t('admin.libraryNamePlaceholder')"
            class="md-input h-10 w-full text-xs"
          />
        </div>

        <div>
          <label class="md-label-medium mb-1.5 block text-[var(--md-sys-color-on-surface-variant)]">
            {{ t('admin.libraryPath') }}
          </label>
          <input
            v-model="libPath"
            type="text"
            required
            :placeholder="t('admin.libraryPathPlaceholder')"
            class="md-input h-10 w-full font-mono text-xs"
          />
        </div>
      </div>

      <div class="flex flex-wrap items-center justify-between gap-3">
        <AppCheckbox v-model="scanNow" :label="t('admin.scanAfterMountLabel')" />
        <button
          type="submit"
          :disabled="mounting"
          class="md-btn md-btn-filled"
        >
          <LuLibrary class="h-4 w-4" />
          <span>{{ scanNow ? t('admin.mountAndScanBtn') : t('admin.mountOnlyBtn') }}</span>
        </button>
      </div>
    </form>
  </section>

  <section class="md-card-outlined space-y-4 p-6">
    <div class="flex flex-wrap items-center justify-between gap-3">
      <div v-if="!domain" class="w-48">
        <AppSelect
          :model-value="libraryFilterKind"
          :options="libraryFilterOptions"
          @update:model-value="libraryFilterKind = $event as 'all' | MediaKind"
        />
      </div>
      <div v-else class="flex items-center gap-2">
        <h2 class="md-title-small text-[var(--md-sys-color-on-surface)]">
          {{ t('settings.domainMediaSourcesListTitle') }}
        </h2>
        <span
          class="md-label-small rounded-full bg-[var(--md-sys-color-surface-container-high)] px-2.5 py-0.5 tabular-nums text-[var(--md-sys-color-on-surface-variant)]"
        >
          {{ filteredLibraries.length }}
        </span>
      </div>

      <div class="flex items-center gap-2">
        <RefreshButton size="md" :action="refreshAdminLibraries" />
        <button
          type="button"
          :disabled="scanningAll"
          class="md-btn md-btn-outlined"
          @click="handleScanAll"
        >
          <LuRefreshCw class="h-4 w-4" :class="scanningAll ? 'animate-spin' : ''" />
          <span>{{ t('admin.scanAllBtn') }}</span>
        </button>
      </div>
    </div>

    <div class="space-y-3">
      <div
        v-for="lib in filteredLibraries"
        :key="lib.id"
        class="md-card-filled flex flex-col justify-between gap-3 p-4 sm:flex-row sm:items-center"
      >
        <div class="min-w-0 flex-1 space-y-1">
          <div class="flex flex-wrap items-center gap-2">
            <span
              class="md-label-small rounded-full bg-[var(--md-sys-color-secondary-container)] px-2.5 py-0.5 text-[var(--md-sys-color-on-secondary-container)]"
            >
              {{ t(`app.domains.${lib.media_kind}`) }}
            </span>
            <p class="truncate text-sm font-semibold text-[var(--md-sys-color-on-surface)]">
              {{ lib.name }}
            </p>
            <span
              class="md-label-small rounded-full bg-[var(--md-sys-color-surface-container-low)] px-2.5 py-0.5 text-[var(--md-sys-color-on-surface-variant)]"
            >
              {{
                lib.kind.type === 'managed'
                  ? t('admin.managedBadge')
                  : t('admin.localMountBadge')
              }}
            </span>
            <span class="md-body-small font-medium text-[var(--md-sys-color-on-surface-variant)]">
              · {{ t('admin.itemsCountBadge', { count: lib.item_count }) }}
            </span>
          </div>

          <p
            v-if="lib.kind.type === 'local_mount'"
            class="md-body-small truncate font-mono text-[var(--md-sys-color-on-surface-variant)]"
          >
            {{ lib.kind.root_path }}
          </p>
          <p class="md-body-small text-[var(--md-sys-color-on-surface-variant)]">
            {{ formatScanStatus(lib) }}
          </p>
        </div>

        <div class="flex shrink-0 items-center gap-2">
          <button
            v-if="lib.kind.type === 'local_mount'"
            type="button"
            :disabled="scanningId === lib.id || lib.scan_status.state === 'scanning'"
            class="md-btn md-btn-sm md-btn-tonal"
            @click="handleRescan(lib)"
          >
            <LuRefreshCw
              class="h-3.5 w-3.5"
              :class="
                scanningId === lib.id || lib.scan_status.state === 'scanning'
                  ? 'animate-spin'
                  : ''
              "
            />
            <span>{{ t('admin.rescanBtn') }}</span>
          </button>

          <button
            v-if="lib.kind.type === 'local_mount'"
            type="button"
            class="md-icon-btn md-icon-btn-sm hover:!text-[var(--md-sys-color-error)]"
            :title="t('admin.deleteLibrary')"
            :aria-label="t('admin.deleteLibrary')"
            @click="handleDeleteLibrary(lib)"
          >
            <LuTrash2 class="h-4 w-4" />
          </button>
        </div>
      </div>
    </div>
  </section>
</template>
