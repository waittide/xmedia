<script setup lang="ts">
import { computed, useSlots, type Component } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  LuBookOpen,
  LuFilm,
  LuImage,
  LuMusic,
  LuSlidersHorizontal,
} from 'vue-icons-plus/lu'
import {
  MdButton,
  MdChip,
  MdEmptyState,
  MdSheet,
  MdSkeleton,
  MdTabs,
  MdTextField,
  type MdTabItem,
} from '@/components/ui'
import { useDelayedLoading } from '@/composables/useDelayedLoading'
import type { SourceScope } from '@/sources'
import type { MediaKind } from '@/types/api'
import MediaErrorState from './MediaErrorState.vue'
import type { MediaItemCardAspect } from './MediaItemCard.vue'
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
}>()

const slots = useSlots()
const { t, locale } = useI18n()
const isZh = computed(() => locale.value.startsWith('zh'))

const resolvedAspect = computed<MediaItemCardAspect>(() => {
  if (props.skeletonAspect) return props.skeletonAspect
  if (props.domain === 'music') return 'square'
  if (props.domain === 'video') return 'video'
  return 'poster'
})

const domainIcon = computed<Component>(() => {
  if (props.emptyIcon) return props.emptyIcon
  switch (props.domain) {
    case 'comic':
      return LuImage
    case 'music':
      return LuMusic
    case 'video':
      return LuFilm
    case 'book':
    default:
      return LuBookOpen
  }
})

const resolvedTitle = computed(() => {
  if (props.title) return props.title
  return isZh.value ? `${t(`app.domains.${props.domain}`)}资料库` : `${t(`app.domains.${props.domain}`)} Library`
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
  const base = isZh.value ? '筛选' : 'Filter'
  return props.activeFilterCount > 0 ? `${base} (${props.activeFilterCount})` : base
})

const resolvedFilterSheetTitle = computed(
  () => props.filterSheetTitle || (isZh.value ? '筛选条件' : 'Filters'),
)

const resetLabel = computed(() => (isZh.value ? '重置' : 'Reset'))
const doneLabel = computed(() => (isZh.value ? '完成' : 'Done'))

const hasFilterSheet = computed(() => Boolean(slots['filter-sheet']))
const hasToolbarRow = computed(
  () =>
    props.showSearch ||
    Boolean(slots['toolbar-chips']) ||
    hasFilterSheet.value ||
    Boolean(slots['toolbar-trailing']),
)

const skeletonGridClass = computed(() =>
  resolvedAspect.value === 'video'
    ? 'grid grid-cols-1 gap-5 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4'
    : 'grid grid-cols-2 gap-5 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6',
)

const showSkeleton = useDelayedLoading(() => props.loading)

function handleScopeUpdate(next: SourceScope): void {
  emit('update:scope', next)
  emit('scope-change', next)
}

function handleSearchSubmit(): void {
  emit('search-submit', props.searchQuery)
}

function handleSearchClear(): void {
  emit('update:searchQuery', '')
  emit('search-clear')
  emit('search-submit', '')
}
</script>

<template>
  <div class="mx-auto w-full space-y-4" :class="maxWidthClass">
    <!-- Row 1: Compact title + always-visible Local/Online[/Remote] SourceScopeBar -->
    <header class="flex flex-wrap items-center justify-between gap-3">
      <div class="flex min-w-0 items-baseline gap-2.5">
        <h1 class="text-headline-small truncate text-on-surface">
          {{ resolvedTitle }}
        </h1>
        <span
          v-if="count !== null && count !== undefined"
          class="text-body-small shrink-0 tabular-nums text-on-surface-variant"
        >
          {{ count }}
        </span>
      </div>

      <div class="flex flex-wrap items-center gap-2">
        <SourceScopeBar
          v-if="showScopeBar"
          :model-value="scope"
          :domain="domain"
          :sync-url="syncScopeUrl"
          :show-remote="showRemoteScope"
          size="sm"
          @update:model-value="handleScopeUpdate"
        />
        <slot name="header-trailing" />
      </div>
    </header>

    <!-- Optional Primary Tabs (replaces stacked SegmentedButtons) -->
    <MdTabs
      v-if="tabs.length > 1"
      :model-value="activeTab"
      :tabs="tabs"
      variant="primary"
      @update:model-value="emit('update:activeTab', $event)"
    />

    <!-- Row 2: Single compact control bar (1 search input + inline chips + trailing) -->
    <div v-if="hasToolbarRow" class="flex flex-wrap items-center gap-2">
      <form
        v-if="showSearch"
        class="min-w-48 flex-1 sm:max-w-sm"
        @submit.prevent="handleSearchSubmit"
      >
        <MdTextField
          :model-value="searchQuery"
          type="search"
          size="sm"
          clearable
          :placeholder="resolvedSearchPlaceholder"
          @update:model-value="emit('update:searchQuery', $event)"
          @clear="handleSearchClear"
        />
      </form>

      <div
        v-if="$slots['toolbar-chips'] || hasFilterSheet"
        class="no-scrollbar flex min-w-0 flex-1 items-center gap-2 overflow-x-auto py-0.5"
      >
        <MdChip
          v-if="hasFilterSheet"
          variant="filter"
          :icon="LuSlidersHorizontal"
          :label="resolvedFilterLabel"
          :selected="activeFilterCount > 0"
          hide-check
          @click="emit('update:filterSheetOpen', !filterSheetOpen)"
        />
        <slot name="toolbar-chips" />
      </div>

      <div v-if="$slots['toolbar-trailing']" class="ml-auto flex shrink-0 items-center gap-1.5">
        <slot name="toolbar-trailing" />
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
  </div>
</template>
