<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import {
  LuFolderOpen,
  LuLibrary,
  LuLock,
  LuPlus,
  LuRefreshCw,
  LuTrash2,
} from 'vue-icons-plus/lu'
import { api } from '@/api/client'
import {
  MdButton,
  MdCard,
  MdChip,
  MdDialog,
  MdEmptyState,
  MdIconButton,
  MdSelect,
  MdSwitch,
  MdTextField,
  RefreshButton,
} from '@/components/ui'
import { useAuthStore } from '@/stores/auth'
import { useDialogStore } from '@/stores/dialog'
import { useDomainStore } from '@/stores/domain'
import { useEventsStore } from '@/stores/events'
import { useSnackbarStore } from '@/stores/snackbar'
import type { CoreLibraryDto, MediaKind } from '@/types/api'
import { resolveLibraryRoute, useSettingsI18n } from './i18nHelper'
import { DOMAIN_ICONS } from './sections'

const props = withDefaults(
  defineProps<{
    filterDomain?: 'all' | MediaKind
  }>(),
  {
    filterDomain: 'all',
  },
)

const emit = defineEmits<{
  error: [message: string]
  'update:filterDomain': [value: 'all' | MediaKind]
}>()

const { t, tr } = useSettingsI18n()
const router = useRouter()
const authStore = useAuthStore()
const dialogStore = useDialogStore()
const domainStore = useDomainStore()
const eventsStore = useEventsStore()
const snackbarStore = useSnackbarStore()

const allLibraries = ref<CoreLibraryDto[]>([])
const loading = ref(false)
const scanningId = ref<string | null>(null)
const scanningAll = ref(false)

// Add Library Dialog state
const addDialogOpen = ref(false)
const libKind = ref<MediaKind>(
  props.filterDomain !== 'all' ? props.filterDomain : domainStore.activeDomain,
)
const libName = ref('')
const libPath = ref('')
const scanNow = ref(true)
const mounting = ref(false)
const dialogError = ref('')

const enabledDomains = computed<MediaKind[]>(() => authStore.enabledModules)

const domainSelectOptions = computed(() =>
  enabledDomains.value.map((k) => ({
    value: k,
    label: t(`app.domains.${k}`),
  })),
)

const enabledLibraries = computed(() =>
  allLibraries.value.filter((l) => authStore.hasModule(l.media_kind)),
)

const countsByDomain = computed<Record<MediaKind, number>>(() => {
  const counts: Record<MediaKind, number> = { book: 0, comic: 0, music: 0, video: 0 }
  for (const lib of enabledLibraries.value) {
    counts[lib.media_kind] = (counts[lib.media_kind] ?? 0) + 1
  }
  return counts
})

const filteredLibraries = computed(() => {
  if (props.filterDomain === 'all') return enabledLibraries.value
  return enabledLibraries.value.filter((l) => l.media_kind === props.filterDomain)
})

const hasLocalMountInView = computed(() =>
  filteredLibraries.value.some((lib) => lib.kind.type === 'local_mount'),
)

async function refreshAdminLibraries(): Promise<void> {
  if (!authStore.isAdmin) return
  loading.value = true
  try {
    allLibraries.value = await api.listLibraries()
  } finally {
    loading.value = false
  }
}

onMounted(() => {
  if (!authStore.isAdmin) return
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
    if (!authStore.isAdmin) return
    void refreshAdminLibraries().catch(() => {})
  },
)

function inferNameFromPath(pathStr: string): string {
  const clean = pathStr.trim().replace(/[\\/]+$/, '')
  if (!clean) return ''
  const parts = clean.split(/[\\/]/).filter(Boolean)
  return parts[parts.length - 1] ?? ''
}

function openAddDialog(): void {
  dialogError.value = ''
  libKind.value =
    props.filterDomain !== 'all'
      ? props.filterDomain
      : authStore.hasModule(domainStore.activeDomain)
        ? domainStore.activeDomain
        : (enabledDomains.value[0] ?? 'book')
  libName.value = ''
  libPath.value = ''
  scanNow.value = true
  addDialogOpen.value = true
}

async function handleMountLibrary(): Promise<void> {
  dialogError.value = ''
  emit('error', '')
  const trimmedPath = libPath.value.trim()
  const resolvedName = libName.value.trim() || inferNameFromPath(trimmedPath)
  if (!trimmedPath || !resolvedName) {
    dialogError.value = t('admin.libraryPathPlaceholder')
    return
  }
  mounting.value = true
  try {
    const created = await api.createLibrary({
      name: resolvedName,
      media_kind: libKind.value,
      root_path: trimmedPath,
    })
    if (scanNow.value) {
      await api.scanLibrary(created.id)
    }
    addDialogOpen.value = false
    libName.value = ''
    libPath.value = ''
    eventsStore.bumpDomain(created.media_kind)
    await refreshAdminLibraries()
    snackbarStore.show(
      tr('settings.libraryMountedMsg', '已挂载媒体库「{name}」', 'Mounted library "{name}"', {
        name: created.name,
      }),
    )
  } catch (err) {
    dialogError.value = err instanceof Error ? err.message : String(err)
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

function openLibraryInDomain(lib: CoreLibraryDto): void {
  void router.push(resolveLibraryRoute(router, lib.media_kind, { library: lib.id }))
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
  <!-- Admin-only Gate -->
  <MdCard v-if="!authStore.isAdmin" variant="outlined" padding="lg" class="py-14 text-center">
    <div
      class="mx-auto mb-3 flex h-12 w-12 items-center justify-center rounded-corner-lg bg-surface-container-high text-on-surface-variant"
    >
      <LuLock class="h-5 w-5" />
    </div>
    <p class="text-title-medium text-on-surface">
      {{
        tr(
          'settings.librariesAdminOnly',
          '仅管理员可管理本地媒体库',
          'Administrator access required to manage media libraries',
        )
      }}
    </p>
  </MdCard>

  <div v-else class="flex flex-col gap-5">
    <!-- Filter Chips + Action Bar -->
    <div class="flex flex-wrap items-center justify-between gap-3">
      <div class="no-scrollbar flex flex-nowrap items-center gap-2 overflow-x-auto py-0.5">
        <MdChip
          variant="filter"
          :label="`${t('admin.libraryFilterAll')} (${enabledLibraries.length})`"
          :selected="filterDomain === 'all'"
          @click="emit('update:filterDomain', 'all')"
        />
        <MdChip
          v-for="kind in enabledDomains"
          :key="kind"
          variant="filter"
          hide-check
          :icon="DOMAIN_ICONS[kind]"
          :label="`${t(`app.domains.${kind}`)} (${countsByDomain[kind]})`"
          :selected="filterDomain === kind"
          @click="emit('update:filterDomain', filterDomain === kind ? 'all' : kind)"
        />
      </div>

      <div class="flex shrink-0 items-center gap-2">
        <RefreshButton size="sm" :action="refreshAdminLibraries" />
        <MdButton
          v-if="hasLocalMountInView"
          variant="outlined"
          size="sm"
          :icon="LuRefreshCw"
          :loading="scanningAll"
          @click="handleScanAll"
        >
          {{ t('admin.scanAllBtn') }}
        </MdButton>
        <MdButton variant="filled" size="sm" :icon="LuPlus" @click="openAddDialog">
          {{ tr('settings.addLibraryBtn', '添加文件夹', 'Add Folder') }}
        </MdButton>
      </div>
    </div>

    <!-- Unified Library List -->
    <MdCard variant="outlined" padding="none" class="overflow-hidden">
      <MdEmptyState
        v-if="filteredLibraries.length === 0 && !loading"
        compact
        :icon="LuLibrary"
        :title="
          tr(
            'settings.librariesEmptyTitle',
            '尚未添加媒体库',
            'No media libraries yet',
          )
        "
        :description="t('admin.mountLibraryDesc')"
      >
        <template #action>
          <MdButton variant="filled" size="sm" :icon="LuPlus" @click="openAddDialog">
            {{ tr('settings.addLibraryBtn', '添加文件夹', 'Add Folder') }}
          </MdButton>
        </template>
      </MdEmptyState>

      <ul v-else class="divide-y divide-outline-variant/50">
        <li
          v-for="lib in filteredLibraries"
          :key="lib.id"
          class="flex flex-col justify-between gap-3 px-5 py-4 transition-colors hover:bg-surface-container-low sm:flex-row sm:items-center"
        >
          <div class="min-w-0 flex-1 space-y-1">
            <div class="flex flex-wrap items-center gap-2">
              <span
                class="text-label-small inline-flex shrink-0 items-center gap-1 rounded-corner-full bg-secondary-container px-2.5 py-0.5 text-on-secondary-container"
              >
                <component :is="DOMAIN_ICONS[lib.media_kind]" class="h-3 w-3" />
                <span>{{ t(`app.domains.${lib.media_kind}`) }}</span>
              </span>

              <button
                type="button"
                class="text-title-small inline-flex min-w-0 cursor-pointer items-center gap-1.5 truncate text-left text-on-surface transition-colors hover:text-primary"
                @click="openLibraryInDomain(lib)"
              >
                <LuFolderOpen class="h-4 w-4 shrink-0 text-primary" />
                <span class="truncate">{{ lib.name }}</span>
              </button>

              <span
                class="text-label-small rounded-corner-full bg-surface-container-high px-2.5 py-0.5 text-on-surface-variant"
              >
                {{
                  lib.kind.type === 'managed'
                    ? t('admin.managedBadge')
                    : t('admin.localMountBadge')
                }}
              </span>

              <span class="text-body-small text-on-surface-variant">
                · {{ t('admin.itemsCountBadge', { count: lib.item_count }) }}
              </span>
            </div>

            <p
              v-if="lib.kind.type === 'local_mount'"
              class="text-body-small truncate font-mono text-on-surface-variant"
            >
              {{ lib.kind.root_path }}
            </p>
            <p
              class="text-body-small"
              :class="
                lib.scan_status.state === 'failed'
                  ? 'text-error'
                  : lib.scan_status.state === 'scanning'
                    ? 'text-primary'
                    : 'text-on-surface-variant'
              "
            >
              {{ formatScanStatus(lib) }}
            </p>
          </div>

          <div v-if="lib.kind.type === 'local_mount'" class="flex shrink-0 items-center gap-1.5">
            <MdButton
              variant="tonal"
              size="sm"
              :icon="LuRefreshCw"
              :loading="scanningId === lib.id || lib.scan_status.state === 'scanning'"
              @click="handleRescan(lib)"
            >
              {{ t('admin.rescanBtn') }}
            </MdButton>

            <MdIconButton
              size="sm"
              :icon="LuTrash2"
              :tooltip="t('admin.deleteLibrary')"
              :aria-label="t('admin.deleteLibrary')"
              class="hover:text-error"
              @click="handleDeleteLibrary(lib)"
            />
          </div>
        </li>
      </ul>
    </MdCard>

    <!-- Add Library Dialog -->
    <MdDialog
      :open="addDialogOpen"
      :title="t('admin.mountLibraryTitle')"
      :subtitle="t('admin.mountLibraryDesc')"
      size="sm"
      @close="!mounting && (addDialogOpen = false)"
    >
      <template #icon>
        <LuLibrary class="h-5 w-5" />
      </template>

      <form id="settings-add-library-form" class="flex flex-col gap-4 py-1" @submit.prevent="handleMountLibrary">
        <div
          v-if="dialogError"
          role="alert"
          class="text-body-small rounded-corner-md bg-error-container px-3.5 py-2.5 text-on-error-container"
        >
          {{ dialogError }}
        </div>

        <div>
          <label class="text-label-medium mb-1.5 block text-on-surface-variant">
            {{ t('admin.libraryKind') }}
          </label>
          <MdSelect
            :model-value="libKind"
            :options="domainSelectOptions"
            :disabled="mounting"
            @update:model-value="libKind = $event as MediaKind"
          />
        </div>

        <MdTextField
          v-model="libPath"
          required
          :disabled="mounting"
          :label="t('admin.libraryPath')"
          :placeholder="t('admin.libraryPathPlaceholder')"
        />

        <MdTextField
          v-model="libName"
          :disabled="mounting"
          :label="t('admin.libraryName')"
          :placeholder="inferNameFromPath(libPath) || t('admin.libraryNamePlaceholder')"
          :supporting-text="
            tr(
              'settings.libraryNameAutoHint',
              '留空将自动使用目录名称',
              'Leave blank to use the folder name automatically',
            )
          "
        />

        <div class="rounded-corner-md bg-surface-container px-4 py-2.5">
          <MdSwitch
            v-model="scanNow"
            :disabled="mounting"
            :label="t('admin.scanAfterMountLabel')"
          />
        </div>
      </form>

      <template #footer>
        <MdButton variant="text" :disabled="mounting" @click="addDialogOpen = false">
          {{ t('common.cancel') }}
        </MdButton>
        <MdButton
          type="submit"
          form="settings-add-library-form"
          variant="filled"
          :icon="LuLibrary"
          :loading="mounting"
          :disabled="!libPath.trim()"
        >
          {{ scanNow ? t('admin.mountAndScanBtn') : t('admin.mountOnlyBtn') }}
        </MdButton>
      </template>
    </MdDialog>
  </div>
</template>
