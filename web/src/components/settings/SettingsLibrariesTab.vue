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

const emit = defineEmits<{
  error: [message: string]
}>()

const { t } = useI18n()
const authStore = useAuthStore()
const dialogStore = useDialogStore()
const domainStore = useDomainStore()
const eventsStore = useEventsStore()

const allLibraries = ref<CoreLibraryDto[]>([])
const libraryFilterKind = ref<'all' | MediaKind>('all')
const libKind = ref<MediaKind>(domainStore.activeDomain)
const libName = ref('')
const libPath = ref('')
const scanNow = ref(true)
const mounting = ref(false)
const scanningId = ref<string | null>(null)
const scanningAll = ref(false)

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
  if (libraryFilterKind.value === 'all') return allLibraries.value
  return allLibraries.value.filter((l) => l.media_kind === libraryFilterKind.value)
})

async function refreshAdminLibraries(): Promise<void> {
  allLibraries.value = await api.listLibraries()
}

onMounted(() => {
  void refreshAdminLibraries().catch(() => {})
})

watch(
  () => eventsStore.domainRefreshTick,
  () => {
    void refreshAdminLibraries().catch(() => {})
  },
  { deep: true },
)

async function handleMountLibrary(): Promise<void> {
  emit('error', '')
  mounting.value = true
  try {
    const created = await api.createLibrary({
      name: libName.value.trim(),
      media_kind: libKind.value,
      root_path: libPath.value.trim(),
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
    await Promise.all([
      (async () => {
        for (const lib of filteredLibraries.value) {
          if (lib.kind.type === 'local_mount' && lib.scan_status.state !== 'scanning') {
            await api.scanLibrary(lib.id)
            eventsStore.bumpDomain(lib.media_kind)
          }
        }
        await refreshAdminLibraries()
      })(),
      new Promise((resolve) => setTimeout(resolve, 450)),
    ])
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
  <section class="card-soft rounded-2xl bg-[var(--ctp-shell)] p-6">
    <h2 class="text-sm font-bold">{{ t('admin.mountLibraryTitle') }}</h2>
    <p class="mt-1 text-xs text-[var(--ctp-subtext0)]">{{ t('admin.mountLibraryDesc') }}</p>

    <form class="mt-4 space-y-4" @submit.prevent="handleMountLibrary">
      <div class="grid grid-cols-1 gap-4 sm:grid-cols-3">
        <div>
          <label class="mb-1.5 block text-xs font-semibold text-[var(--ctp-subtext1)]">
            {{ t('admin.libraryKind') }}
          </label>
          <AppSelect
            :model-value="libKind"
            :options="domainSelectOptions"
            @update:model-value="libKind = $event as MediaKind"
          />
        </div>

        <div>
          <label class="mb-1.5 block text-xs font-semibold text-[var(--ctp-subtext1)]">
            {{ t('admin.libraryName') }}
          </label>
          <input
            v-model="libName"
            type="text"
            required
            :placeholder="t('admin.libraryNamePlaceholder')"
            class="h-10 w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3.5 text-xs outline-none focus:border-[var(--ctp-accent)]"
          />
        </div>

        <div>
          <label class="mb-1.5 block text-xs font-semibold text-[var(--ctp-subtext1)]">
            {{ t('admin.libraryPath') }}
          </label>
          <input
            v-model="libPath"
            type="text"
            required
            :placeholder="t('admin.libraryPathPlaceholder')"
            class="h-10 w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3.5 font-mono text-xs outline-none focus:border-[var(--ctp-accent)]"
          />
        </div>
      </div>

      <div class="flex flex-wrap items-center justify-between gap-3">
        <AppCheckbox v-model="scanNow" :label="t('admin.scanAfterMountLabel')" />
        <button
          type="submit"
          :disabled="mounting"
          class="inline-flex h-10 items-center gap-2 rounded-xl bg-[var(--ctp-accent)] px-5 text-xs font-semibold text-[var(--ctp-on-accent)] shadow-sm transition hover:opacity-90 disabled:opacity-55"
        >
          <LuLibrary class="h-4 w-4" />
          <span>{{ scanNow ? t('admin.mountAndScanBtn') : t('admin.mountOnlyBtn') }}</span>
        </button>
      </div>
    </form>
  </section>

  <section class="card-soft space-y-4 rounded-2xl bg-[var(--ctp-shell)] p-6">
    <div class="flex flex-wrap items-center justify-between gap-3">
      <div class="w-48">
        <AppSelect
          :model-value="libraryFilterKind"
          :options="libraryFilterOptions"
          @update:model-value="libraryFilterKind = $event as 'all' | MediaKind"
        />
      </div>

      <div class="flex items-center gap-2">
        <RefreshButton size="md" :action="refreshAdminLibraries" />
        <button
          type="button"
          :disabled="scanningAll"
          class="inline-flex h-9 items-center gap-1.5 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3.5 text-xs font-semibold text-[var(--ctp-text)] transition hover:border-[var(--ctp-accent)] disabled:opacity-50"
          @click="handleScanAll"
        >
          <LuRefreshCw class="h-3.5 w-3.5" :class="scanningAll ? 'animate-spin' : ''" />
          <span>{{ t('admin.scanAllBtn') }}</span>
        </button>
      </div>
    </div>

    <div class="space-y-3">
      <div
        v-for="lib in filteredLibraries"
        :key="lib.id"
        class="flex flex-col justify-between gap-3 rounded-2xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] p-4 sm:flex-row sm:items-center"
      >
        <div class="min-w-0 flex-1 space-y-1">
          <div class="flex flex-wrap items-center gap-2">
            <span
              class="rounded-md bg-[var(--ctp-accent-soft)] px-2 py-0.5 text-[11px] font-bold text-[var(--ctp-accent)]"
            >
              {{ t(`app.domains.${lib.media_kind}`) }}
            </span>
            <p class="truncate text-sm font-bold text-[var(--ctp-text)]">{{ lib.name }}</p>
            <span
              class="rounded-full bg-[var(--ctp-mantle)] px-2.5 py-0.5 text-[11px] font-medium text-[var(--ctp-subtext0)]"
            >
              {{
                lib.kind.type === 'managed'
                  ? t('admin.managedBadge')
                  : t('admin.localMountBadge')
              }}
            </span>
            <span class="text-xs font-semibold text-[var(--ctp-subtext1)]">
              · {{ t('admin.itemsCountBadge', { count: lib.item_count }) }}
            </span>
          </div>

          <p
            v-if="lib.kind.type === 'local_mount'"
            class="truncate font-mono text-xs text-[var(--ctp-subtext0)]"
          >
            {{ lib.kind.root_path }}
          </p>
          <p class="text-xs text-[var(--ctp-subtext0)]">{{ formatScanStatus(lib) }}</p>
        </div>

        <div class="flex shrink-0 items-center gap-2">
          <button
            v-if="lib.kind.type === 'local_mount'"
            type="button"
            :disabled="scanningId === lib.id || lib.scan_status.state === 'scanning'"
            class="inline-flex h-9 items-center gap-1.5 rounded-xl bg-[var(--ctp-accent-soft)] px-3 text-xs font-semibold text-[var(--ctp-accent)] transition hover:opacity-85 disabled:opacity-50"
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
            class="rounded-xl p-2 text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-red)]/10 hover:text-[var(--ctp-red)]"
            :title="t('admin.deleteLibrary')"
            @click="handleDeleteLibrary(lib)"
          >
            <LuTrash2 class="h-4 w-4" />
          </button>
        </div>
      </div>
    </div>
  </section>
</template>
