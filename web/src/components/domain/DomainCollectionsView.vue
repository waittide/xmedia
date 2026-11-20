<script setup lang="ts">
import { computed, ref, watch, type Component } from 'vue'
import {
  LuArrowUpRight,
  LuFolder,
  LuFolderPlus,
  LuLibrary,
  LuPencil,
  LuRefreshCw,
  LuSearch,
  LuSettings,
  LuTag,
  LuTrash2,
  LuX,
} from 'vue-icons-plus/lu'
import AppSelect, { type SelectOption } from '@/components/ui/AppSelect.vue'
import RefreshButton from '@/components/ui/RefreshButton.vue'

export type CollectionSortKey = 'count_desc' | 'count_asc' | 'name_asc' | 'updated_desc'
export type CollectionViewMode = 'libraries' | 'collections' | 'chips'

export interface CollectionLibraryItem {
  id: string
  name: string
  kindBadge: string
  countLabel: string
  count: number
  updatedAt?: string
  rootPath?: string | null
  scanText?: string | null
  isMounted: boolean
  isScanning: boolean
}

export interface CollectionGroupItem {
  id: string
  name: string
  countLabel: string
  count: number
  updatedAt?: string
  badge?: string | null
  canEdit: boolean
}

export interface CollectionChipItem {
  key: string
  name: string
  count: number
  sublabel?: string | null
}

const props = withDefaults(
  defineProps<{
    mode: CollectionViewMode
    resetKey: string
    title: string
    subtitle: string
    total: number
    searchPlaceholder: string
    emptyText: string
    seeAllLabel: string
    sortOptions: SelectOption[]
    onRefresh: () => Promise<unknown>
    maxWidthClass?: string
    libraries?: CollectionLibraryItem[]
    showManageLibraries?: boolean
    manageLibrariesLabel?: string
    rescanLabel?: string
    showRescan?: boolean
    onScanLibrary?: (id: string) => Promise<void>
    collections?: CollectionGroupItem[]
    collectionIcon?: Component
    createPlaceholder?: string
    createButtonLabel?: string
    renameTitle?: string
    deleteTitle?: string
    onCreateCollection?: (name: string) => Promise<void>
    chips?: CollectionChipItem[]
    chipIcon?: Component
    chipIconTone?: 'primary' | 'secondary'
  }>(),
  {
    maxWidthClass: 'max-w-7xl',
    libraries: () => [],
    showManageLibraries: false,
    manageLibrariesLabel: '',
    rescanLabel: '',
    showRescan: false,
    onScanLibrary: undefined,
    collections: () => [],
    collectionIcon: undefined,
    createPlaceholder: '',
    createButtonLabel: '',
    renameTitle: '',
    deleteTitle: '',
    onCreateCollection: undefined,
    chips: () => [],
    chipIcon: undefined,
    chipIconTone: 'primary',
  },
)

const emit = defineEmits<{
  openLibrary: [id: string]
  openCollection: [id: string]
  renameCollection: [id: string]
  deleteCollection: [id: string]
  openChip: [key: string]
}>()

const filterKeyword = ref('')
const sortKey = ref<CollectionSortKey>('count_desc')
const newName = ref('')
const creating = ref(false)
const scanningId = ref<string | null>(null)

watch(
  () => props.resetKey,
  () => {
    filterKeyword.value = ''
    sortKey.value = 'count_desc'
    newName.value = ''
  },
)

const filteredLibraries = computed(() => {
  const q = filterKeyword.value.trim().toLowerCase()
  const list = props.libraries.filter((lib) => {
    if (!q) return true
    const path = (lib.rootPath ?? '').toLowerCase()
    return lib.name.toLowerCase().includes(q) || path.includes(q)
  })
  return [...list].sort((a, b) => {
    if (sortKey.value === 'count_desc') return b.count - a.count
    if (sortKey.value === 'count_asc') return a.count - b.count
    if (sortKey.value === 'updated_desc') return (b.updatedAt ?? '').localeCompare(a.updatedAt ?? '')
    return a.name.localeCompare(b.name)
  })
})

const filteredCollections = computed(() => {
  const q = filterKeyword.value.trim().toLowerCase()
  const list = props.collections.filter((item) => !q || item.name.toLowerCase().includes(q))
  return [...list].sort((a, b) => {
    if (sortKey.value === 'count_desc') return b.count - a.count
    if (sortKey.value === 'count_asc') return a.count - b.count
    if (sortKey.value === 'updated_desc') return (b.updatedAt ?? '').localeCompare(a.updatedAt ?? '')
    return a.name.localeCompare(b.name)
  })
})

const filteredChips = computed(() => {
  const q = filterKeyword.value.trim().toLowerCase()
  const list = props.chips.filter((c) => !q || c.name.toLowerCase().includes(q))
  return [...list].sort((a, b) => {
    if (sortKey.value === 'count_desc') return b.count - a.count
    if (sortKey.value === 'count_asc') return a.count - b.count
    return a.name.localeCompare(b.name)
  })
})

async function handleScanLibrary(id: string): Promise<void> {
  if (!props.onScanLibrary) return
  scanningId.value = id
  try {
    await Promise.all([
      props.onScanLibrary(id),
      new Promise((resolve) => setTimeout(resolve, 450)),
    ])
  } finally {
    scanningId.value = null
  }
}

async function handleCreate(): Promise<void> {
  const trimmed = newName.value.trim()
  if (!trimmed || !props.onCreateCollection) return
  creating.value = true
  try {
    await props.onCreateCollection(trimmed)
    newName.value = ''
  } finally {
    creating.value = false
  }
}
</script>

<template>
  <div class="mx-auto space-y-6 px-6 py-8 sm:px-8" :class="maxWidthClass">
    <!-- Top Header + Actions -->
    <div class="flex flex-wrap items-end justify-between gap-4">
      <div>
        <div class="flex items-center gap-2.5">
          <h1 class="text-2xl font-bold tracking-tight text-[var(--md-sys-color-on-surface)]">
            {{ title }}
          </h1>
          <span
            class="rounded-full bg-[var(--md-sys-color-primary-container)] px-2.5 py-0.5 text-xs font-bold text-[var(--md-sys-color-on-primary-container)]"
          >
            {{ total }}
          </span>
        </div>
        <p class="mt-1 text-xs text-[var(--md-sys-color-on-surface-variant)]">
          {{ subtitle }}
        </p>
      </div>

      <div class="flex flex-wrap items-center gap-2">
        <RefreshButton size="md" :action="onRefresh" />

        <RouterLink
          v-if="mode === 'libraries' && showManageLibraries"
          to="/settings?tab=libraries"
          class="md-btn md-btn-sm md-btn-filled md-state !h-9"
        >
          <LuSettings class="h-3.5 w-3.5" />
          <span>{{ manageLibrariesLabel }}</span>
        </RouterLink>

        <form
          v-else-if="mode === 'collections' && onCreateCollection"
          class="flex items-center gap-2"
          @submit.prevent="handleCreate"
        >
          <input
            v-model="newName"
            type="text"
            :placeholder="createPlaceholder"
            class="md-input h-9 w-52 text-xs"
          />
          <button
            type="submit"
            :disabled="!newName.trim() || creating"
            class="md-btn md-btn-sm md-btn-filled md-state !h-9"
          >
            <LuFolderPlus class="h-3.5 w-3.5" />
            <span>{{ createButtonLabel }}</span>
          </button>
        </form>
      </div>
    </div>

    <!-- Search + Sort Toolbar -->
    <div class="md-card-filled flex flex-wrap items-center justify-between gap-3 p-3">
      <div class="relative min-w-[220px] max-w-md flex-1">
        <LuSearch
          class="pointer-events-none absolute top-1/2 left-3.5 h-4 w-4 -translate-y-1/2 text-[var(--md-sys-color-on-surface-variant)]"
        />
        <input
          v-model="filterKeyword"
          type="text"
          :placeholder="searchPlaceholder"
          class="md-input h-9 w-full !pr-8 !pl-9 text-xs placeholder:text-[var(--md-sys-color-on-surface-variant)]"
        />
        <button
          v-if="filterKeyword"
          type="button"
          class="md-icon-btn md-icon-btn-sm md-state absolute top-1/2 right-1 !h-7 !w-7 -translate-y-1/2"
          @click="filterKeyword = ''"
        >
          <LuX class="h-3.5 w-3.5" />
        </button>
      </div>

      <div class="w-56">
        <AppSelect
          :model-value="sortKey"
          :options="sortOptions"
          variant="compact"
          @update:model-value="sortKey = $event as CollectionSortKey"
        />
      </div>
    </div>

    <!-- 1. Libraries Grid -->
    <template v-if="mode === 'libraries'">
      <div
        v-if="filteredLibraries.length === 0"
        class="md-card-filled py-14 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
      >
        {{ emptyText }}
      </div>

      <div v-else class="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
        <div
          v-for="lib in filteredLibraries"
          :key="lib.id"
          class="group md-card-filled flex flex-col justify-between border border-transparent p-5 transition hover:border-[var(--md-sys-color-primary)]/45 hover:shadow-[var(--md-sys-elevation-1)]"
        >
          <div>
            <div class="flex items-start justify-between gap-3">
              <button
                type="button"
                class="flex min-w-0 items-center gap-3 text-left"
                @click="emit('openLibrary', lib.id)"
              >
                <div
                  class="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)]"
                >
                  <LuLibrary class="h-5 w-5" />
                </div>
                <div class="min-w-0">
                  <h3
                    class="truncate text-sm font-bold text-[var(--md-sys-color-on-surface)] group-hover:text-[var(--md-sys-color-primary)]"
                  >
                    {{ lib.name }}
                  </h3>
                  <span
                    class="mt-0.5 inline-block rounded bg-[var(--md-sys-color-surface-container-high)] px-1.5 py-0.5 text-[10px] font-medium text-[var(--md-sys-color-on-surface-variant)]"
                  >
                    {{ lib.kindBadge }}
                  </span>
                </div>
              </button>

              <span
                class="shrink-0 rounded-full bg-[var(--md-sys-color-surface-container-high)] px-2.5 py-1 text-xs font-bold text-[var(--md-sys-color-on-surface)]"
              >
                {{ lib.countLabel }}
              </span>
            </div>

            <p
              v-if="lib.isMounted && lib.rootPath"
              class="mt-3 truncate font-mono text-[11px] text-[var(--md-sys-color-on-surface-variant)]"
              :title="lib.rootPath"
            >
              {{ lib.rootPath }}
            </p>
            <p
              v-if="lib.isMounted && lib.scanText"
              class="mt-1 text-xs text-[var(--md-sys-color-on-surface-variant)]"
            >
              {{ lib.scanText }}
            </p>
          </div>

          <div
            class="mt-4 flex items-center justify-between border-t border-[var(--md-sys-color-outline-variant)]/55 pt-3"
          >
            <button
              type="button"
              class="inline-flex items-center gap-1 text-xs font-semibold text-[var(--md-sys-color-primary)] hover:underline"
              @click="emit('openLibrary', lib.id)"
            >
              <span>{{ seeAllLabel }}</span>
              <LuArrowUpRight class="h-3.5 w-3.5" />
            </button>

            <button
              v-if="lib.isMounted && showRescan && onScanLibrary"
              type="button"
              :disabled="scanningId === lib.id || lib.isScanning"
              class="md-btn md-btn-sm md-btn-outlined md-state !h-7 !px-2.5 !text-[11px]"
              @click="handleScanLibrary(lib.id)"
            >
              <LuRefreshCw
                class="h-3 w-3"
                :class="scanningId === lib.id || lib.isScanning ? 'animate-spin' : ''"
              />
              <span>{{ rescanLabel }}</span>
            </button>
          </div>
        </div>
      </div>
    </template>

    <!-- 2. Shelves / Playlists Grid -->
    <template v-else-if="mode === 'collections'">
      <div
        v-if="filteredCollections.length === 0"
        class="md-card-filled py-14 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
      >
        {{ emptyText }}
      </div>

      <div v-else class="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
        <div
          v-for="item in filteredCollections"
          :key="item.id"
          class="group md-card-filled flex flex-col justify-between border border-transparent p-5 transition hover:border-[var(--md-sys-color-primary)]/45 hover:shadow-[var(--md-sys-elevation-1)]"
        >
          <div class="flex items-start justify-between gap-3">
            <button
              type="button"
              class="flex min-w-0 items-center gap-3 text-left"
              @click="emit('openCollection', item.id)"
            >
              <slot name="collection-leading" :item="item">
                <div
                  class="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)]"
                >
                  <component :is="collectionIcon ?? LuFolder" class="h-5 w-5" />
                </div>
              </slot>
              <div class="min-w-0">
                <h3
                  class="truncate text-sm font-bold text-[var(--md-sys-color-on-surface)] group-hover:text-[var(--md-sys-color-primary)]"
                >
                  {{ item.name }}
                </h3>
                <p class="mt-0.5 text-xs text-[var(--md-sys-color-on-surface-variant)]">
                  {{ item.countLabel }}
                </p>
              </div>
            </button>

            <span
              v-if="item.badge"
              class="shrink-0 rounded bg-[var(--md-sys-color-surface-container-high)] px-2 py-0.5 text-[10px] font-semibold text-[var(--md-sys-color-on-surface-variant)]"
            >
              {{ item.badge }}
            </span>
          </div>

          <div
            class="mt-4 flex items-center justify-between border-t border-[var(--md-sys-color-outline-variant)]/55 pt-3"
          >
            <button
              type="button"
              class="inline-flex items-center gap-1 text-xs font-semibold text-[var(--md-sys-color-primary)] hover:underline"
              @click="emit('openCollection', item.id)"
            >
              <span>{{ seeAllLabel }}</span>
              <LuArrowUpRight class="h-3.5 w-3.5" />
            </button>

            <div v-if="item.canEdit" class="flex items-center gap-1">
              <button
                type="button"
                class="md-icon-btn md-icon-btn-sm md-state !h-7 !w-7"
                :title="renameTitle || undefined"
                @click="emit('renameCollection', item.id)"
              >
                <LuPencil class="h-3.5 w-3.5" />
              </button>
              <button
                type="button"
                class="md-icon-btn md-icon-btn-sm md-state !h-7 !w-7 hover:bg-[var(--md-sys-color-error-container)]/60 hover:!text-[var(--md-sys-color-error)]"
                :title="deleteTitle || undefined"
                @click="emit('deleteCollection', item.id)"
              >
                <LuTrash2 class="h-3.5 w-3.5" />
              </button>
            </div>
          </div>
        </div>
      </div>
    </template>

    <!-- 3. Tags / Authors / Genres Chips Grid -->
    <template v-else>
      <div
        v-if="filteredChips.length === 0"
        class="md-card-filled py-14 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
      >
        {{ emptyText }}
      </div>

      <div v-else class="grid grid-cols-2 gap-3.5 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5">
        <button
          v-for="chip in filteredChips"
          :key="chip.key"
          type="button"
          class="group md-card-filled md-state flex items-center justify-between gap-2.5 border border-transparent px-4 py-3.5 text-left transition hover:-translate-y-0.5 hover:border-[var(--md-sys-color-primary)]/50 hover:shadow-[var(--md-sys-elevation-1)]"
          @click="emit('openChip', chip.key)"
        >
          <span class="flex min-w-0 items-center gap-2.5">
            <span
              class="flex h-8 w-8 shrink-0 items-center justify-center rounded-xl"
              :class="
                chipIconTone === 'secondary'
                  ? 'bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]'
                  : 'bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)]'
              "
            >
              <component :is="chipIcon ?? LuTag" class="h-4 w-4" />
            </span>
            <span class="min-w-0">
              <span
                class="block truncate text-xs font-bold text-[var(--md-sys-color-on-surface)] group-hover:text-[var(--md-sys-color-primary)]"
              >
                {{ chip.name }}
              </span>
              <span
                v-if="chip.sublabel"
                class="block text-[10px] text-[var(--md-sys-color-on-surface-variant)]"
              >
                {{ chip.sublabel }}
              </span>
            </span>
          </span>
          <span
            class="shrink-0 rounded-full bg-[var(--md-sys-color-surface-container-high)] px-2 py-0.5 text-[11px] font-semibold text-[var(--md-sys-color-on-surface-variant)]"
          >
            {{ chip.count }}
          </span>
        </button>
      </div>
    </template>
  </div>
</template>
