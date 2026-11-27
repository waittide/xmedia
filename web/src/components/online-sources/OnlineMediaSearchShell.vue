<script setup lang="ts">
import { computed, ref, useSlots, type Component } from 'vue'
import { useI18n } from 'vue-i18n'
import type { RouteLocationRaw } from 'vue-router'
import {
  LuAlertCircle,
  LuChevronDown,
  LuChevronUp,
  LuCompass,
  LuGlobe,
  LuRefreshCw,
  LuSearch,
  LuSettings2,
} from 'vue-icons-plus/lu'
import {
  MediaErrorState,
  MediaItemCard,
  PaginationBar,
  mediaGridClassFor,
  resolveDomainAspect,
  resolveDomainCoverStyle,
  resolveDomainIcon,
  type MediaItemCardAspect,
  type MediaItemCoverStyle,
} from '@/components/domain'
import { MdButton, MdChip, MdEmptyState, MdSkeleton, MdTextField } from '@/components/ui'
import { useDelayedLoading } from '@/composables/useDelayedLoading'
import {
  useOnlineItemStates,
  type MediaItemVM,
  type TrackedItemRef,
  type UseOnlineItemStatesReturn,
} from '@/sources'
import { useAuthStore } from '@/stores/auth'
import type { OnlineDomain, OnlineSourceSummaryDto } from './types'
import type { OnlineBrowseCategory, OnlineSearchErrorItem } from './useOnlineSearchBrowse'

const props = withDefaults(
  defineProps<{
    domain: OnlineDomain
    keyword: string
    /** Set to `false` inside DomainLibraryTemplate to prevent duplicate search boxes. */
    showSearchInput?: boolean
    searchPlaceholder?: string
    loadingInit?: boolean
    sourcesLoadError?: string | null
    enabledSources: readonly OnlineSourceSummaryDto[]
    selectedSourceIds: readonly string[]
    /** Max source chips shown before folding behind "+N" toggle. */
    sourceCollapseLimit?: number
    searching?: boolean
    browsing?: boolean
    hasSearched?: boolean
    lastSearchKeyword?: string
    searchErrors?: readonly OnlineSearchErrorItem[]
    searchToasts?: readonly string[]
    showBrowseView?: boolean
    browseCategories?: readonly OnlineBrowseCategory[]
    activeCategoryId?: string | null
    /** Max category chips shown before folding behind "+N" toggle. */
    categoryCollapseLimit?: number
    browseError?: string | null
    /** Unified MediaItemVM list rendered with MediaItemCard when `#results` slot is omitted. */
    items?: readonly MediaItemVM[]
    /** Explicit item count when caller renders custom `#results` slot. */
    itemCount?: number
    itemAspect?: MediaItemCardAspect
    itemCoverStyle?: MediaItemCoverStyle
    /** Optional externally managed `useOnlineItemStates` (falls back to built-in lookup over `items`). */
    itemStates?: UseOnlineItemStatesReturn
    showFavoriteOnCards?: boolean
    hasMore?: boolean
    loadingMore?: boolean
    isAdmin?: boolean
    settingsTo?: RouteLocationRaw
    idleHint?: string
  }>(),
  {
    showSearchInput: true,
    searchPlaceholder: '',
    loadingInit: false,
    sourcesLoadError: null,
    sourceCollapseLimit: 6,
    searching: false,
    browsing: false,
    hasSearched: false,
    lastSearchKeyword: '',
    searchErrors: () => [],
    searchToasts: () => [],
    showBrowseView: false,
    browseCategories: () => [],
    activeCategoryId: null,
    categoryCollapseLimit: 10,
    browseError: null,
    items: () => [],
    itemCount: undefined,
    itemAspect: undefined,
    itemCoverStyle: undefined,
    itemStates: undefined,
    showFavoriteOnCards: true,
    hasMore: false,
    loadingMore: false,
    isAdmin: undefined,
    settingsTo: undefined,
    idleHint: '',
  },
)

const emit = defineEmits<{
  'update:keyword': [value: string]
  'search-submit': []
  'search-clear': []
  'toggle-source': [sourceId: string]
  'clear-sources': []
  'select-category': [categoryId: string | null]
  'retry-init': []
  'retry-browse': []
  'retry-search': []
  'load-more': []
  'select-item': [item: MediaItemVM]
  'toggle-favorite': [item: MediaItemVM]
}>()

const slots = useSlots()
const { t } = useI18n()
const authStore = useAuthStore()

const sourcesExpanded = ref(false)
const categoriesExpanded = ref(false)

const resolvedIsAdmin = computed(() =>
  props.isAdmin !== undefined ? props.isAdmin : authStore.isAdmin,
)

const resolvedSettingsTo = computed<RouteLocationRaw>(
  () => props.settingsTo ?? { path: `/${props.domain}/settings`, query: { tab: 'sources' } },
)

const resolvedAspect = computed<MediaItemCardAspect>(() =>
  resolveDomainAspect(props.domain, props.itemAspect),
)

const resolvedCoverStyle = computed<MediaItemCoverStyle>(() =>
  resolveDomainCoverStyle(props.domain, props.itemCoverStyle),
)

const domainLabel = computed(() => t(`onlineSources.domainLabel.${props.domain}`))

const domainIcon = computed<Component>(() => resolveDomainIcon(props.domain))

const resolvedSearchPlaceholder = computed(() => {
  if (props.searchPlaceholder) return props.searchPlaceholder
  return t('onlineSearch.placeholder', { domain: domainLabel.value })
})

// Built-in batch online item state lookup when caller passes `items` without external `itemStates`
const internalTrackedRefs = computed<TrackedItemRef[]>(() => {
  if (props.itemStates) return []
  const refs: TrackedItemRef[] = []
  for (const item of props.items) {
    if (item.ref.scope === 'online' || item.ref.scope === 'remote') {
      refs.push(item.ref)
    }
  }
  return refs
})

const internalItemStates = useOnlineItemStates(internalTrackedRefs, {
  autoLoad: false,
})

const resolvedItemStates = computed(() => props.itemStates ?? internalItemStates)

const resolvedItems = computed<MediaItemVM[]>(() =>
  props.items.map((item) => {
    if (item.ref.scope === 'local') return item
    const snap = resolvedItemStates.value.getState(item.ref)
    if (!snap) return item
    const ratioPct =
      snap.progressRatio !== null && Number.isFinite(snap.progressRatio)
        ? Math.round(snap.progressRatio * 100)
        : undefined
    return {
      ...item,
      badge: {
        ...item.badge,
        source: item.badge?.source || snap.sourceName || undefined,
        sourceKind: item.badge?.sourceKind ?? snap.sourceKind,
        progressPercent: item.badge?.progressPercent ?? ratioPct,
        availability: item.badge?.availability ?? snap.availability,
        blocked: item.badge?.blocked ?? snap.blocked,
      },
    }
  }),
)

const resolvedItemCount = computed(() => props.itemCount ?? resolvedItems.value.length)

// Collapsible source chip bar (never renders a 40+ chip wall)
const canCollapseSources = computed(
  () => props.enabledSources.length > props.sourceCollapseLimit,
)

const visibleSources = computed<OnlineSourceSummaryDto[]>(() => {
  if (!canCollapseSources.value || sourcesExpanded.value) {
    return [...props.enabledSources]
  }
  const head = props.enabledSources.slice(0, props.sourceCollapseLimit)
  const headIds = new Set(head.map((s) => s.id))
  const selectedOverflow = props.enabledSources.filter(
    (s) => props.selectedSourceIds.includes(s.id) && !headIds.has(s.id),
  )
  return [...head, ...selectedOverflow]
})

const hiddenSourcesCount = computed(() =>
  Math.max(0, props.enabledSources.length - visibleSources.value.length),
)

// Collapsible category chip bar
const canCollapseCategories = computed(
  () => props.browseCategories.length > props.categoryCollapseLimit,
)

const visibleCategories = computed<OnlineBrowseCategory[]>(() => {
  if (!canCollapseCategories.value || categoriesExpanded.value) {
    return [...props.browseCategories]
  }
  const head = props.browseCategories.slice(0, props.categoryCollapseLimit)
  if (
    props.activeCategoryId &&
    !head.some((c) => c.id === props.activeCategoryId)
  ) {
    const active = props.browseCategories.find((c) => c.id === props.activeCategoryId)
    if (active) return [...head, active]
  }
  return head
})

const hiddenCategoriesCount = computed(() =>
  Math.max(0, props.browseCategories.length - visibleCategories.value.length),
)

const isLoadingContent = computed(
  () => props.loadingInit || props.searching || (props.browsing && resolvedItemCount.value === 0),
)

const hasNoEnabledSources = computed(
  () => !props.loadingInit && !props.sourcesLoadError && props.enabledSources.length === 0,
)

const hasFatalBrowseError = computed(
  () => !props.hasSearched && Boolean(props.browseError) && resolvedItemCount.value === 0,
)

const hasFatalSearchError = computed(
  () => props.hasSearched && !props.searching && resolvedItemCount.value === 0 && props.searchErrors.length > 0,
)

const showEmptySearch = computed(
  () =>
    props.hasSearched &&
    !props.searching &&
    resolvedItemCount.value === 0 &&
    props.searchErrors.length === 0,
)

const showIdleState = computed(
  () =>
    !props.hasSearched &&
    !isLoadingContent.value &&
    !props.sourcesLoadError &&
    !props.browseError &&
    props.enabledSources.length > 0 &&
    resolvedItemCount.value === 0,
)

const gridClass = computed(() => mediaGridClassFor(resolvedAspect.value))

const skeletonCount = computed(() => (resolvedAspect.value === 'video' ? 8 : 12))

// Localized labels meeting E2E §2.2 regexes (T2-F13-01, T2-F13-05, T2-F13-06)
const allSourcesLabel = computed(() =>
  t('onlineSearch.allSources', { count: props.enabledSources.length }),
)
const allCategoriesLabel = computed(() => t('onlineSearch.allCategories'))
const categoriesAriaLabel = computed(() => t('onlineSearch.categoriesAriaLabel'))
const noSourcesTitle = computed(() =>
  t('onlineSearch.noSourcesTitle', { domain: domainLabel.value }),
)
const noSourcesDescription = computed(() =>
  resolvedIsAdmin.value
    ? t('onlineSearch.noSourcesAdminDesc')
    : t('onlineSearch.noSourcesMemberDesc'),
)
const manageSourcesLabel = computed(() => t('onlineSearch.manageSourcesBtn'))
const searchBtnLabel = computed(() => t('common.search'))
const expandLabel = computed(() => t('onlineSearch.expand'))
const collapseLabel = computed(() => t('onlineSearch.collapse'))
const noSearchResultsTitle = computed(() => {
  const kw = props.lastSearchKeyword || props.keyword
  return t('onlineSearch.noResultsForKeyword', { keyword: kw })
})
const resolvedIdleHint = computed(() => {
  if (props.idleHint) return props.idleHint
  return t('onlineSearch.idleHint')
})
const resultsSummaryText = computed(() =>
  props.hasSearched
    ? t('onlineSearch.resultsCount', { count: resolvedItemCount.value })
    : t('onlineSearch.itemsCount', { count: resolvedItemCount.value }),
)

function formatSourceSearchError(err: OnlineSearchErrorItem): string {
  return t('onlineSearch.sourceError', { source: err.source_name, error: err.error })
}

function isItemFavorite(item: MediaItemVM): boolean {
  if (item.ref.scope === 'local') return false
  return resolvedItemStates.value.isFavorite(item.ref)
}

function itemKey(item: MediaItemVM, idx: number): string {
  if (item.ref.scope === 'local') return `local:${item.ref.id}:${idx}`
  return `${item.ref.scope}:${item.ref.sourceId}:${item.ref.ref}:${idx}`
}

const showSkeleton = useDelayedLoading(isLoadingContent)

function handleKeywordClear(): void {
  emit('update:keyword', '')
  emit('search-clear')
}
</script>

<template>
  <div class="space-y-4">
    <!-- Optional Single Search Bar (omitted when nested inside DomainLibraryTemplate) -->
    <form
      v-if="showSearchInput"
      class="flex items-center gap-2"
      @submit.prevent="emit('search-submit')"
    >
      <div class="min-w-0 flex-1">
        <MdTextField
          :model-value="keyword"
          type="search"
          size="sm"
          clearable
          :placeholder="resolvedSearchPlaceholder"
          :disabled="enabledSources.length === 0 && !loadingInit"
          @update:model-value="emit('update:keyword', $event)"
          @clear="handleKeywordClear"
        />
      </div>
      <MdButton
        type="submit"
        variant="filled"
        :icon="LuSearch"
        :loading="searching"
        :disabled="!keyword.trim() || enabledSources.length === 0"
      >
        {{ searchBtnLabel }}
      </MdButton>
    </form>

    <!-- Sources Load Error (T2-F13-06: explicit [role="alert"] + Retry, hides empty-sources CTA) -->
    <MediaErrorState
      v-if="!loadingInit && sourcesLoadError"
      compact
      :message="sourcesLoadError"
      @retry="emit('retry-init')"
    />

    <!-- Collapsible Source Filter Chips -->
    <div
      v-else-if="enabledSources.length > 0"
      role="group"
      :aria-label="allSourcesLabel"
      class="no-scrollbar flex flex-nowrap items-center gap-1.5 overflow-x-auto py-0.5 sm:flex-wrap sm:overflow-visible sm:py-0"
    >
      <MdChip
        class="shrink-0"
        variant="filter"
        :icon="LuGlobe"
        :label="allSourcesLabel"
        :selected="selectedSourceIds.length === 0"
        hide-check
        @click="emit('clear-sources')"
      />
      <MdChip
        v-for="src in visibleSources"
        :key="src.id"
        class="shrink-0"
        variant="filter"
        :label="src.name"
        :selected="selectedSourceIds.includes(src.id)"
        @click="emit('toggle-source', src.id)"
      />
      <MdButton
        v-if="canCollapseSources"
        class="shrink-0"
        size="sm"
        variant="text"
        :icon="sourcesExpanded ? LuChevronUp : LuChevronDown"
        @click="sourcesExpanded = !sourcesExpanded"
      >
        {{ sourcesExpanded ? collapseLabel : `${expandLabel} (+${hiddenSourcesCount})` }}
      </MdButton>
    </div>

    <!-- Collapsible Explore Categories Bar (T1-F13-03: shown when q is empty) -->
    <div
      v-if="showBrowseView && browseCategories.length > 0"
      role="group"
      :aria-label="categoriesAriaLabel"
      class="no-scrollbar flex flex-nowrap items-center gap-1.5 overflow-x-auto py-0.5 sm:flex-wrap sm:overflow-visible sm:py-0"
    >
      <MdChip
        class="shrink-0"
        variant="filter"
        :icon="LuCompass"
        :label="allCategoriesLabel"
        :selected="activeCategoryId === null"
        hide-check
        @click="emit('select-category', null)"
      />
      <MdChip
        v-for="cat in visibleCategories"
        :key="cat.id"
        class="shrink-0"
        variant="filter"
        :label="cat.name"
        :selected="activeCategoryId === cat.id"
        hide-check
        @click="emit('select-category', cat.id)"
      />
      <MdButton
        v-if="canCollapseCategories"
        class="shrink-0"
        size="sm"
        variant="text"
        :icon="categoriesExpanded ? LuChevronUp : LuChevronDown"
        @click="categoriesExpanded = !categoriesExpanded"
      >
        {{ categoriesExpanded ? collapseLabel : `${expandLabel} (+${hiddenCategoriesCount})` }}
      </MdButton>
    </div>

    <!-- Search Toasts & Per-Source Search Errors (T2-F13-05: partial results + error notice) -->
    <div v-if="searchToasts.length > 0 || searchErrors.length > 0" class="space-y-2">
      <div
        v-for="(toast, idx) in searchToasts"
        :key="`toast-${idx}`"
        class="text-body-small rounded-corner-md bg-surface-container px-3.5 py-2 text-on-surface-variant"
      >
        {{ toast }}
      </div>

      <div
        v-for="err in searchErrors"
        :key="err.source_id"
        role="alert"
        class="flex flex-wrap items-center justify-between gap-2 rounded-corner-md bg-error-container/80 px-3.5 py-2.5 text-on-error-container"
      >
        <div class="flex min-w-0 flex-1 items-start gap-2">
          <LuAlertCircle class="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" />
          <span class="text-body-small min-w-0 flex-1 break-words">
            {{ formatSourceSearchError(err) }}
          </span>
        </div>
        <MdButton
          v-if="hasFatalSearchError"
          variant="tonal"
          :icon="LuRefreshCw"
          @click="emit('retry-search')"
        >
          {{ t('remote.retryBtn') }}
        </MdButton>
      </div>
    </div>

    <!-- Browse Error Banner (hides empty state) -->
    <MediaErrorState
      v-if="showBrowseView && browseError"
      compact
      :message="browseError"
      :retrying="browsing"
      @retry="emit('retry-browse')"
    />

    <!-- Empty Enabled Sources Guidance (T2-F13-01: includes 来源/source, hidden on error) -->
    <MdEmptyState
      v-if="hasNoEnabledSources"
      :icon="LuGlobe"
      :title="noSourcesTitle"
      :description="noSourcesDescription"
    >
      <template v-if="resolvedIsAdmin" #action>
        <MdButton variant="tonal" :icon="LuSettings2" :to="resolvedSettingsTo">
          {{ manageSourcesLabel }}
        </MdButton>
      </template>
    </MdEmptyState>

    <!-- Primary Loading Skeleton (delayed to avoid flash on fast navigations) -->
    <div v-else-if="isLoadingContent" :class="gridClass" aria-busy="true">
      <template v-if="showSkeleton">
        <MdSkeleton
          v-for="idx in skeletonCount"
          :key="`online-sk-${idx}`"
          variant="card"
          :aspect="resolvedAspect"
        />
      </template>
    </div>

    <!-- Empty Search Results (only when no search errors) -->
    <MdEmptyState
      v-else-if="showEmptySearch"
      compact
      :icon="LuSearch"
      :title="noSearchResultsTitle"
    />

    <!-- Idle State (when q is empty and source has no explore items) -->
    <template v-else-if="showIdleState">
      <slot name="idle">
        <MdEmptyState compact :icon="domainIcon" :title="resolvedIdleHint" />
      </slot>
    </template>

    <!-- Results Area (hidden when fatal browse error or fatal search error) -->
    <template v-else-if="!hasFatalBrowseError && !hasFatalSearchError && resolvedItemCount > 0">
      <!-- Top Summary & Optional Batch Actions (e.g. Music Play All / Shuffle) -->
      <div
        v-if="slots['summary-actions']"
        class="flex flex-wrap items-center justify-between gap-2"
      >
        <span class="text-body-small tabular-nums text-on-surface-variant">
          {{ resultsSummaryText }}
        </span>
        <div class="flex flex-wrap items-center gap-2">
          <slot name="summary-actions" />
        </div>
      </div>

      <!-- Custom Results Slot or Default Borderless MD3 MediaItemCard Grid -->
      <slot name="results" :items="resolvedItems" :item-states="resolvedItemStates">
        <div :class="gridClass">
          <MediaItemCard
            v-for="(item, idx) in resolvedItems"
            :key="itemKey(item, idx)"
            :item="item"
            :aspect="resolvedAspect"
            :cover-style="resolvedCoverStyle"
            :show-source-badge="true"
            :show-favorite="showFavoriteOnCards && item.ref.scope !== 'local'"
            :favorite="showFavoriteOnCards ? isItemFavorite(item) : undefined"
            @select="emit('select-item', $event)"
            @toggle-favorite="emit('toggle-favorite', $event)"
          >
            <template v-if="$slots['card-menu']" #menu="{ close }">
              <slot name="card-menu" :item="item" :close="close" />
            </template>
          </MediaItemCard>
        </div>
      </slot>

      <PaginationBar
        mode="load-more"
        :has-more="hasMore"
        :loading="loadingMore"
        @load-more="emit('load-more')"
      />
    </template>
  </div>
</template>
