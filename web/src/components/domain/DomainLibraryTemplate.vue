<script setup lang="ts">
import { computed, nextTick, ref, useSlots, type Component } from 'vue'
import { useI18n } from 'vue-i18n'
import { LuSearch, LuSlidersHorizontal, LuUpload, LuX } from 'vue-icons-plus/lu'
import {
  MdButton,
  MdChip,
  MdEmptyState,
  MdIconButton,
  MdSheet,
  MdSkeleton,
  MdTabs,
  MdTextField,
  type MdTabItem,
} from '@/components/ui'
import { useDelayedLoading } from '@/composables/useDelayedLoading'
import { useSourceFilter, type SourceScope } from '@/sources'
import { useAuthStore } from '@/stores/auth'
import { UPLOAD_EXTENSIONS_BY_KIND, useUploadStore } from '@/stores/uploads'
import type { MediaKind } from '@/types/api'
import MediaErrorState from './MediaErrorState.vue'
import {
  resolveDomainAspect,
  resolveDomainIcon,
  type MediaItemCardAspect,
} from './mediaLayout'
import PaginationBar, { type PaginationMode } from './PaginationBar.vue'
import SourceScopeBar from './SourceScopeBar.vue'

const props = withDefaults(
  defineProps<{
    domain: MediaKind
    title?: string
    count?: number | null
    /** Controlled source scope. Omit to let SourceScopeBar sync URL (`scope=online`) automatically. */
    scope?: SourceScope
    syncScopeUrl?: boolean
    /** Always visible by default in both local and online scopes (E2E §2.2). */
    showScopeBar?: boolean
    showRemoteScope?: boolean
    /** Whether the current user can upload files in local scope (defaults to `authStore.isAdmin`). */
    canUpload?: boolean
    /** Show the built-in local-scope upload button. Defaults to `true`. */
    showUpload?: boolean
    /** Single `main input[type="search"]` query value. */
    searchQuery?: string
    searchPlaceholder?: string
    showSearch?: boolean
    tabs?: MdTabItem<string>[]
    activeTab?: string
    loading?: boolean
    skeletonCount?: number
    skeletonAspect?: MediaItemCardAspect
    skeletonMode?: 'grid' | 'list'
    /** Explicit error state (hides empty state when set). */
    error?: string | null
    empty?: boolean
    emptyTitle?: string
    emptyDescription?: string
    emptyIcon?: Component
    page?: number
    pageSize?: number
    total?: number | null
    totalPages?: number | null
    hasMore?: boolean
    paginationMode?: PaginationMode
    loadingMore?: boolean
    filterSheetOpen?: boolean
    filterSheetTitle?: string
    activeFilterCount?: number
    maxWidthClass?: string
  }>(),
  {
    title: '',
    count: null,
    scope: undefined,
    syncScopeUrl: undefined,
    showScopeBar: true,
    showRemoteScope: undefined,
    canUpload: undefined,
    showUpload: true,
    searchQuery: '',
    searchPlaceholder: '',
    showSearch: true,
    tabs: () => [],
    activeTab: '',
    loading: false,
    skeletonCount: 12,
    skeletonAspect: undefined,
    skeletonMode: 'grid',
    error: null,
    empty: false,
    emptyTitle: '',
    emptyDescription: '',
    emptyIcon: undefined,
    page: 1,
    pageSize: 24,
    total: null,
    totalPages: null,
    hasMore: false,
    paginationMode: 'pages',
    loadingMore: false,
    filterSheetOpen: false,
    filterSheetTitle: '',
    activeFilterCount: 0,
    maxWidthClass: 'max-w-7xl',
  },
)

const emit = defineEmits<{
  'update:scope': [scope: SourceScope]
  'scope-change': [scope: SourceScope]
  'update:searchQuery': [query: string]
  'search-submit': [query: string]
  'search-clear': []
  'update:activeTab': [tab: string]
  'update:page': [page: number]
  'load-more': []
  retry: []
  'update:filterSheetOpen': [open: boolean]
  'reset-filters': []
  upload: []
}>()

const slots = useSlots()
const { t } = useI18n()
const authStore = useAuthStore()
const uploadStore = useUploadStore()
const fallbackSourceFilter = useSourceFilter(() => props.domain, { autoLoad: false })
const uploadInputRef = ref<HTMLInputElement | null>(null)

const resolvedCanUpload = computed(() => props.canUpload ?? authStore.isAdmin)
const resolvedScope = computed<SourceScope>(
  () => props.scope ?? fallbackSourceFilter.filter.value.scope,
)
const showLocalUploadBtn = computed(
  () => props.showUpload && resolvedCanUpload.value && resolvedScope.value === 'local',
)
const acceptedUploadExts = computed(() =>
  UPLOAD_EXTENSIONS_BY_KIND[props.domain].map((ext) => `.${ext}`).join(','),
)

const resolvedAspect = computed<MediaItemCardAspect>(() =>
  resolveDomainAspect(props.domain, props.skeletonAspect),
)

const domainIcon = computed<Component>(() => resolveDomainIcon(props.domain, props.emptyIcon))

const resolvedTitle = computed(() => {
  if (props.title) return props.title
  return t('domainUi.domainResourcesTitle', { domain: t(`app.domains.${props.domain}`) })
})

const resolvedSearchPlaceholder = computed(() => {
  if (props.searchPlaceholder) return props.searchPlaceholder
  switch (props.domain) {
    case 'comic':
      return t('common.searchComic')
    case 'music':
      return t('common.searchMusic')
    case 'video':
      return t('common.searchVideo')
    case 'book':
    default:
      return t('common.searchBook')
  }
})

const resolvedEmptyTitle = computed(() => props.emptyTitle || t('common.noResults'))

const resolvedFilterLabel = computed(() => {
  const base = t('common.filter')
  return props.activeFilterCount > 0 ? `${base} (${props.activeFilterCount})` : base
})

const resolvedFilterSheetTitle = computed(
  () => props.filterSheetTitle || t('common.filters'),
)

const resetLabel = computed(() => t('common.reset'))
const doneLabel = computed(() => t('common.done'))

const hasFilterSheet = computed(() => Boolean(slots['filter-sheet']))
const hasToolbarRow = computed(
  () =>
    props.showScopeBar ||
    props.showSearch ||
    Boolean(slots['toolbar-chips']) ||
    hasFilterSheet.value ||
    Boolean(slots['toolbar-trailing']) ||
    showLocalUploadBtn.value,
)

const skeletonGridClass = computed(() =>
  resolvedAspect.value === 'video'
    ? 'grid grid-cols-1 gap-5 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4'
    : 'grid grid-cols-2 gap-5 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6',
)

const showSkeleton = useDelayedLoading(() => props.loading)
const mobileSearchOpen = ref(false)
const searchFieldRef = ref<InstanceType<typeof MdTextField> | null>(null)
const isMobileSearchExpanded = computed(
  () => mobileSearchOpen.value || props.searchQuery.trim().length > 0,
)

function openMobileSearch(): void {
  mobileSearchOpen.value = true
  void nextTick(() => {
    searchFieldRef.value?.focus()
  })
}

function closeMobileSearch(): void {
  mobileSearchOpen.value = false
}

function handleScopeUpdate(next: SourceScope): void {
  if (!props.searchQuery.trim()) {
    mobileSearchOpen.value = false
  }
  emit('update:scope', next)
  emit('scope-change', next)
}

function handleSearchSubmit(): void {
  emit('search-submit', props.searchQuery)
}

function handleSearchClear(): void {
  mobileSearchOpen.value = false
  emit('update:searchQuery', '')
  emit('search-clear')
  emit('search-submit', '')
}

function triggerUpload(): void {
  if (!resolvedCanUpload.value) return
  uploadInputRef.value?.click()
}

function onUploadFilesChange(ev: Event): void {
  const input = ev.target
  if (!(input instanceof HTMLInputElement)) return
  if (input.files && input.files.length > 0) {
    uploadStore.enqueueFiles(input.files)
    emit('upload')
  }
  input.value = ''
}

defineExpose({
  triggerUpload,
})
</script>

<template>
  <div class="mx-auto w-full space-y-2.5 sm:space-y-4" :class="maxWidthClass">
    <div class="flex flex-col gap-2 sm:gap-4">
      <!-- Row 1 (>=600px): Title + Upload (left of SourceScopeBar) + SourceScopeBar (rightmost, h-10) -->
      <header
        class="order-1 items-center justify-between gap-2 sm:flex sm:flex-wrap sm:gap-3"
        :class="$slots['header-trailing'] ? 'flex flex-wrap' : 'hidden sm:flex'"
      >
        <div class="flex min-w-0 items-baseline gap-2">
          <h1 class="text-title-large sm:text-headline-small truncate text-on-surface">
            {{ resolvedTitle }}
          </h1>
          <span
            v-if="count !== null && count !== undefined"
            class="text-body-small shrink-0 tabular-nums text-on-surface-variant"
          >
            {{ count }}
          </span>
        </div>

        <div
          class="no-scrollbar flex max-w-full items-center gap-1.5 overflow-x-auto sm:flex-wrap sm:gap-2"
        >
          <slot name="header-leading" />
          <MdButton
            v-if="showLocalUploadBtn"
            class="hidden sm:inline-flex"
            variant="tonal"
            :icon="LuUpload"
            @click="triggerUpload"
          >
            {{ t('common.upload') }}
          </MdButton>
          <div v-if="showScopeBar" class="hidden sm:inline-flex">
            <SourceScopeBar
              :model-value="scope"
              :domain="domain"
              :sync-url="syncScopeUrl"
              :show-remote="showRemoteScope"
              size="md"
              @update:model-value="handleScopeUpdate"
            />
          </div>
          <slot name="header-trailing" />
        </div>
      </header>

      <!-- MD3 Primary Content Tabs (directly beneath Top App Bar on <600px / Row 2 on >=600px) -->
      <MdTabs
        v-if="tabs.length > 1"
        class="order-2"
        :model-value="activeTab"
        :tabs="tabs"
        :mobile-equal="tabs.length <= 4"
        variant="primary"
        @update:model-value="emit('update:activeTab', $event)"
      />

      <!-- Single Compact Control Bar (1-row horizontal strip on <600px / flexible toolbar on >=600px) -->
      <div
        v-if="hasToolbarRow"
        class="order-3 flex min-w-0 items-center gap-1.5 sm:flex-wrap sm:gap-2"
      >
        <MdIconButton
          v-if="showSearch && !isMobileSearchExpanded"
          class="shrink-0 sm:hidden"
          size="sm"
          :icon="LuSearch"
          :aria-label="resolvedSearchPlaceholder"
          @click="openMobileSearch"
        />

        <form
          v-if="showSearch"
          class="items-center gap-1.5 sm:block sm:min-w-48 sm:max-w-sm sm:flex-1"
          :class="isMobileSearchExpanded ? 'flex min-w-0 flex-1' : 'hidden sm:block'"
          @submit.prevent="handleSearchSubmit"
          @keydown.esc="
            !searchQuery.trim() && isMobileSearchExpanded ? closeMobileSearch() : undefined
          "
        >
          <div class="min-w-0 flex-1">
            <MdTextField
              ref="searchFieldRef"
              :model-value="searchQuery"
              type="search"
              size="sm"
              clearable
              :placeholder="resolvedSearchPlaceholder"
              @update:model-value="emit('update:searchQuery', $event)"
              @clear="handleSearchClear"
            />
          </div>
          <MdIconButton
            v-if="isMobileSearchExpanded && !searchQuery.trim()"
            class="shrink-0 sm:hidden"
            size="sm"
            :icon="LuX"
            :aria-label="t('common.close')"
            @click="closeMobileSearch"
          />
        </form>

        <div
          v-if="showScopeBar || $slots['toolbar-chips'] || hasFilterSheet"
          class="no-scrollbar min-w-0 flex-1 flex-nowrap items-center gap-1.5 overflow-x-auto py-0.5 sm:flex sm:flex-wrap sm:gap-2 sm:overflow-visible sm:py-0"
          :class="isMobileSearchExpanded ? 'hidden sm:flex' : 'flex'"
        >
          <div v-if="showScopeBar" class="inline-flex shrink-0 sm:hidden">
            <SourceScopeBar
              mode="chip-menu"
              :model-value="scope"
              :domain="domain"
              :sync-url="syncScopeUrl"
              :show-remote="showRemoteScope"
              @update:model-value="handleScopeUpdate"
            />
          </div>
          <MdChip
            v-if="hasFilterSheet"
            class="shrink-0"
            variant="filter"
            :icon="LuSlidersHorizontal"
            :label="resolvedFilterLabel"
            :selected="activeFilterCount > 0"
            hide-check
            @click="emit('update:filterSheetOpen', !filterSheetOpen)"
          />
          <slot name="toolbar-chips" />
        </div>

        <div
          v-if="$slots['toolbar-trailing'] || (showLocalUploadBtn && !empty)"
          class="shrink-0 items-center gap-1 sm:ml-auto sm:gap-1.5"
          :class="[
            isMobileSearchExpanded ? 'hidden sm:flex' : 'flex',
            !$slots['toolbar-trailing'] ? 'sm:hidden' : '',
          ]"
        >
          <MdIconButton
            v-if="showLocalUploadBtn && !empty"
            class="sm:hidden"
            size="sm"
            variant="tonal"
            :icon="LuUpload"
            :aria-label="t('common.upload')"
            :tooltip="t('common.upload')"
            @click="triggerUpload"
          />
          <slot name="toolbar-trailing" />
        </div>
      </div>
    </div>

    <!-- Optional contextual selection bar / subheader (e.g. collection or artist header) -->
    <slot name="selection-bar" />
    <slot name="subheader" />

    <!-- Primary content area (occupies >= 80% of above-the-fold height) -->
    <MediaErrorState
      v-if="error"
      :message="error"
      :retrying="loading"
      @retry="emit('retry')"
    />

    <div v-else-if="loading" aria-busy="true">
      <template v-if="showSkeleton">
        <div v-if="skeletonMode === 'list'" class="space-y-2.5">
          <MdSkeleton
            v-for="idx in Math.min(skeletonCount, 10)"
            :key="`list-sk-${idx}`"
            variant="rect"
            height="3.5rem"
          />
        </div>
        <div v-else :class="skeletonGridClass">
          <MdSkeleton
            v-for="idx in skeletonCount"
            :key="`grid-sk-${idx}`"
            variant="card"
            :aspect="resolvedAspect"
          />
        </div>
      </template>
    </div>

    <MdEmptyState
      v-else-if="empty"
      :icon="domainIcon"
      :title="resolvedEmptyTitle"
      :description="emptyDescription"
    >
      <template v-if="$slots['empty-actions']" #action>
        <slot name="empty-actions" />
      </template>
    </MdEmptyState>

    <template v-else>
      <slot />

      <PaginationBar
        :page="page"
        :page-size="pageSize"
        :total="total"
        :total-pages="totalPages"
        :has-more="hasMore"
        :mode="paginationMode"
        :loading="loadingMore"
        @update:page="emit('update:page', $event)"
        @load-more="emit('load-more')"
      />
    </template>

    <!-- Responsive MD3 Filter Sheet (Side sheet >=600px / Bottom sheet <600px) -->
    <MdSheet
      v-if="hasFilterSheet"
      :open="filterSheetOpen"
      :title="resolvedFilterSheetTitle"
      size="sm"
      @close="emit('update:filterSheetOpen', false)"
    >
      <slot name="filter-sheet" />

      <template #footer>
        <slot name="filter-sheet-footer">
          <MdButton
            v-if="activeFilterCount > 0"
            variant="text"
            @click="emit('reset-filters')"
          >
            {{ resetLabel }}
          </MdButton>
          <MdButton variant="filled" @click="emit('update:filterSheetOpen', false)">
            {{ doneLabel }}
          </MdButton>
        </slot>
      </template>
    </MdSheet>

    <input
      v-if="resolvedCanUpload"
      ref="uploadInputRef"
      type="file"
      multiple
      :accept="acceptedUploadExts"
      class="hidden"
      @change="onUploadFilesChange"
    />
  </div>
</template>
