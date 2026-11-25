<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRouter } from 'vue-router'
import {
  LuFolderOpen,
  LuLibrary,
  LuLock,
  LuPencil,
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
import { DOMAIN_ICONS, resolveLibraryRoute } from './sections'

const props = withDefaults(
  defineProps<{
    filterDomain?: 'all' | MediaKind
    lockedDomain?: MediaKind
  }>(),
  {
    filterDomain: 'all',
    lockedDomain: undefined,
  },
)

const emit = defineEmits<{
  error: [message: string]
  'update:filterDomain': [value: 'all' | MediaKind]
}>()

const { t } = useI18n()
const router = useRouter()
const authStore = useAuthStore()
const dialogStore = useDialogStore()
const domainStore = useDomainStore()
const eventsStore = useEventsStore()
const snackbarStore = useSnackbarStore()

const effectiveFilterDomain = computed<'all' | MediaKind>(
  () => props.lockedDomain ?? props.filterDomain,
)

const allLibraries = ref<CoreLibraryDto[]>([])
const loading = ref(false)
const scanningId = ref<string | null>(null)
const scanningAll = ref(false)

// Add Library Dialog state
const addDialogOpen = ref(false)
const libKind = ref<MediaKind>(
  props.lockedDomain ??
    (props.filterDomain !== 'all' ? props.filterDomain : domainStore.activeDomain),
)
const libName = ref('')
const libPath = ref('')
const scanNow = ref(true)
const mounting = ref(false)
const dialogError = ref('')

// Rename Library Dialog state
const renameDialogOpen = ref(false)
const renameTarget = ref<CoreLibraryDto | null>(null)
const renameValue = ref('')
const renaming = ref(false)
const renameError = ref('')

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
  if (effectiveFilterDomain.value === 'all') return enabledLibraries.value
  return enabledLibraries.value.filter((l) => l.media_kind === effectiveFilterDomain.value)
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

watch(
  () => props.lockedDomain,
  (next) => {
    if (next) {
      libKind.value = next
    }
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
    props.lockedDomain ??
    (props.filterDomain !== 'all'
      ? props.filterDomain
      : authStore.hasModule(domainStore.activeDomain)
        ? domainStore.activeDomain
        : (enabledDomains.value[0] ?? 'book'))
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
    snackbarStore.show(t('settings.libraryMountedMsg', { name: created.name }))
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

function openRenameDialog(lib: CoreLibraryDto): void {
  renameTarget.value = lib
  renameValue.value = lib.name
  renameError.value = ''
  renameDialogOpen.value = true
}

async function handleRenameLibrary(): Promise<void> {
  const target = renameTarget.value
  if (!target || renaming.value) return
  renameError.value = ''
  emit('error', '')
  const trimmed = renameValue.value.trim()
  if (!trimmed) {
    renameError.value = t('admin.libraryNamePlaceholder')
    return
  }
  if (trimmed === target.name) {
    renameDialogOpen.value = false
    return
  }
  renaming.value = true
  try {
    const updated = await api.renameLibrary(target.id, trimmed)
    renameDialogOpen.value = false
    renameTarget.value = null
    eventsStore.bumpDomain(updated.media_kind)
    await refreshAdminLibraries()
    snackbarStore.show(t('admin.libraryRenamedMsg', { name: updated.name }))
  } catch (err) {
    renameError.value = err instanceof Error ? err.message : String(err)
  } finally {
    renaming.value = false
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
  <MdCard v-if="!authStore.isAdmin" variant="tonal" padding="lg" class="py-14 text-center">
    <div
      class="mx-auto mb-3 flex h-12 w-12 items-center justify-center rounded-corner-lg bg-surface-container-high text-on-surface-variant"
    >
      <LuLock class="h-5 w-5" />
    </div>
    <p class="text-title-medium text-on-surface">
      {{ t('settings.librariesAdminOnly') }}
    </p>
  </MdCard>

  <div v-else class="flex flex-col gap-5">
    <!-- Filter Chips + Action Bar -->
    <div class="flex flex-wrap items-center justify-between gap-3">
      <div
        v-if="!lockedDomain"
        class="no-scrollbar flex flex-nowrap items-center gap-2 overflow-x-auto py-0.5"
      >
        <MdChip
          variant="filter"
          :label="`${t('admin.libraryFilterAll')} (${enabledLibraries.length})`"
          :selected="effectiveFilterDomain === 'all'"
          @click="emit('update:filterDomain', 'all')"
        />
        <MdChip
          v-for="kind in enabledDomains"
          :key="kind"
          variant="filter"
          hide-check
          :icon="DOMAIN_ICONS[kind]"
          :label="`${t(`app.domains.${kind}`)} (${countsByDomain[kind]})`"
          :selected="effectiveFilterDomain === kind"
          @click="emit('update:filterDomain', effectiveFilterDomain === kind ? 'all' : kind)"
        />
      </div>
      <div v-else class="min-w-0">
        <h2 class="text-title-medium text-on-surface">
          {{ t('settings.navDomainLibraries') }}
        </h2>
        <p class="text-body-small text-on-surface-variant">
          {{ t('admin.mountLibraryDesc') }}
        </p>
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
          {{ t('settings.addLibraryBtn') }}
        </MdButton>
      </div>
    </div>

    <!-- Unified Library List -->
    <MdCard variant="tonal" padding="none" class="overflow-hidden">
      <MdEmptyState
        v-if="filteredLibraries.length === 0 && !loading"
        compact
        :icon="LuLibrary"
        :title="t('settings.librariesEmptyTitle')"
        :description="t('admin.mountLibraryDesc')"
      >
        <template #action>
          <MdButton variant="filled" size="sm" :icon="LuPlus" @click="openAddDialog">
            {{ t('settings.addLibraryBtn') }}
          </MdButton>
        </template>
      </MdEmptyState>

      <ul v-else class="divide-y divide-on-surface/6">
        <li
          v-for="lib in filteredLibraries"
          :key="lib.id"
          class="flex flex-col justify-between gap-3 px-5 py-4 transition-colors hover:bg-surface-container sm:flex-row sm:items-center"
        >
          <div class="min-w-0 flex-1 space-y-1">
            <div class="flex flex-wrap items-center gap-2">
              <span
                v-if="!lockedDomain"
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

          <div class="flex shrink-0 items-center gap-1.5">
            <MdButton
              v-if="lib.kind.type === 'local_mount'"
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
              :icon="LuPencil"
              :tooltip="t('admin.renameLibrary')"
              :aria-label="t('admin.renameLibrary')"
              @click="openRenameDialog(lib)"
            />

            <MdIconButton
              v-if="lib.kind.type === 'local_mount'"
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

        <div v-if="!lockedDomain">
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
          :supporting-text="t('settings.libraryNameAutoHint')"
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

    <!-- Rename Library Dialog -->
    <MdDialog
      :open="renameDialogOpen"
      :title="t('admin.renameLibraryTitle')"
      :subtitle="t('admin.renameLibraryDesc')"
      size="sm"
      @close="!renaming && (renameDialogOpen = false)"
    >
      <template #icon>
        <LuPencil class="h-5 w-5" />
      </template>

      <form
        id="settings-rename-library-form"
        class="flex flex-col gap-4 py-1"
        @submit.prevent="handleRenameLibrary"
      >
        <div
          v-if="renameError"
          role="alert"
          class="text-body-small rounded-corner-md bg-error-container px-3.5 py-2.5 text-on-error-container"
        >
          {{ renameError }}
        </div>

        <p
          v-if="renameTarget?.kind.type === 'local_mount'"
          class="text-body-small truncate rounded-corner-md bg-surface-container px-3.5 py-2 font-mono text-on-surface-variant"
          :title="renameTarget.kind.root_path"
        >
          {{ renameTarget.kind.root_path }}
        </p>

        <MdTextField
          v-model="renameValue"
          required
          autofocus
          :maxlength="128"
          :disabled="renaming"
          :label="t('admin.libraryName')"
          :placeholder="t('admin.libraryNamePlaceholder')"
        />
      </form>

      <template #footer>
        <MdButton variant="text" :disabled="renaming" @click="renameDialogOpen = false">
          {{ t('common.cancel') }}
        </MdButton>
        <MdButton
          type="submit"
          form="settings-rename-library-form"
          variant="filled"
          :loading="renaming"
          :disabled="!renameValue.trim()"
        >
          {{ t('common.save') }}
        </MdButton>
      </template>
    </MdDialog>
  </div>
</template>
