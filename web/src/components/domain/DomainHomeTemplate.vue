<script setup lang="ts">
import { computed, type Component } from 'vue'
import { useI18n } from 'vue-i18n'
import type { RouteLocationRaw } from 'vue-router'
import {
  LuChevronRight,
  LuCompass,
  LuDatabase,
  LuFolderPlus,
  LuRefreshCw,
  LuUpload,
} from 'vue-icons-plus/lu'
import { MdButton, MdEmptyState, MdIconButton, MdSkeleton } from '@/components/ui'
import { useDelayedLoading } from '@/composables/useDelayedLoading'
import { libraryRoute, type MediaItemVM } from '@/sources'
import { useAuthStore } from '@/stores/auth'
import type { MediaKind } from '@/types/api'
import MediaErrorState from './MediaErrorState.vue'
import MediaItemCard from './MediaItemCard.vue'
import {
  mediaGridClassFor,
  resolveDomainAspect,
  resolveDomainCoverStyle,
  resolveDomainIcon,
  type MediaItemCardAspect,
  type MediaItemCoverStyle,
} from './mediaLayout'

const props = withDefaults(
  defineProps<{
    domain: MediaKind
    title?: string
    /** Compact single-line text summary (e.g. "128 本书 · 3 本在读"). Zero stat cards. */
    summary?: string
    continueItems?: readonly MediaItemVM[]
    recentItems?: readonly MediaItemVM[]
    continueTitle?: string
    recentTitle?: string
    continueSeeAllTo?: RouteLocationRaw
    recentSeeAllTo?: RouteLocationRaw
    aspect?: MediaItemCardAspect
    coverStyle?: MediaItemCoverStyle
    loading?: boolean
    refreshing?: boolean
    showRefresh?: boolean
    error?: string | null
    /** Defaults to `useAuthStore().isAdmin`. */
    isAdmin?: boolean
    canUpload?: boolean
    hasOnlineSources?: boolean
    emptyTitle?: string
    emptyDescription?: string
    addLibraryTo?: RouteLocationRaw
    exploreOnlineTo?: RouteLocationRaw
    maxWidthClass?: string
  }>(),
  {
    title: '',
    summary: '',
    continueItems: () => [],
    recentItems: () => [],
    continueTitle: '',
    recentTitle: '',
    continueSeeAllTo: undefined,
    recentSeeAllTo: undefined,
    aspect: undefined,
    coverStyle: undefined,
    loading: false,
    refreshing: false,
    showRefresh: true,
    error: null,
    isAdmin: undefined,
    canUpload: true,
    hasOnlineSources: true,
    emptyTitle: '',
    emptyDescription: '',
    addLibraryTo: undefined,
    exploreOnlineTo: undefined,
    maxWidthClass: 'max-w-7xl',
  },
)

const emit = defineEmits<{
  refresh: []
  retry: []
  upload: []
  'select-item': [item: MediaItemVM]
}>()

const { t } = useI18n()
const authStore = useAuthStore()

const resolvedIsAdmin = computed(() =>
  props.isAdmin !== undefined ? props.isAdmin : authStore.isAdmin,
)

const resolvedAspect = computed<MediaItemCardAspect>(() =>
  resolveDomainAspect(props.domain, props.aspect),
)

const resolvedCoverStyle = computed<MediaItemCoverStyle>(() =>
  resolveDomainCoverStyle(props.domain, props.coverStyle),
)

const domainIcon = computed<Component>(() => resolveDomainIcon(props.domain))

const domainLabel = computed(() => t(`app.domains.${props.domain}`))
const resolvedTitle = computed(() => props.title || domainLabel.value)

const resolvedContinueTitle = computed(() => {
  if (props.continueTitle) return props.continueTitle
  if (props.domain === 'music') return t('domainUi.continueListening')
  if (props.domain === 'video') return t('domainUi.continueWatching')
  return t('domainUi.continueReading')
})

const resolvedRecentTitle = computed(() => {
  if (props.recentTitle) return props.recentTitle
  return t('domainUi.recentlyAdded')
})

const resolvedContinueSeeAll = computed<RouteLocationRaw>(
  () => props.continueSeeAllTo ?? libraryRoute(props.domain, { tab: 'continue' }),
)

const resolvedRecentSeeAll = computed<RouteLocationRaw>(
  () => props.recentSeeAllTo ?? libraryRoute(props.domain),
)

const resolvedAddLibraryTo = computed<RouteLocationRaw>(
  () => props.addLibraryTo ?? { path: `/${props.domain}/settings`, query: { tab: 'libraries' } },
)

const resolvedExploreOnlineTo = computed<RouteLocationRaw>(
  () => props.exploreOnlineTo ?? libraryRoute(props.domain, { scope: 'online' }),
)

const resolvedBrowsePoolTo = computed<RouteLocationRaw>(() =>
  libraryRoute(props.domain, { scope: 'pool' }),
)

const gridClass = computed(() => mediaGridClassFor(resolvedAspect.value))

const skeletonCount = computed(() => (resolvedAspect.value === 'video' ? 8 : 12))
const showSkeleton = useDelayedLoading(() => props.loading && !hasAnyContent.value)

const hasAnyContent = computed(
  () => props.continueItems.length > 0 || props.recentItems.length > 0,
)

const resolvedEmptyTitle = computed(() => {
  if (props.emptyTitle) return props.emptyTitle
  return t('domainUi.emptyDomainTitle', { domain: domainLabel.value })
})

const resolvedEmptyDescription = computed(() => {
  if (props.emptyDescription) return props.emptyDescription
  if (resolvedIsAdmin.value) {
    return t('domainUi.emptyAdminDesc')
  }
  return t('domainUi.emptyMemberDesc')
})

const addLibraryLabel = computed(() => t('domainUi.addLibraryBtn'))
const browsePoolLabel = computed(() => t('domainUi.browsePoolBtn'))
const exploreOnlineLabel = computed(() => t('domainUi.exploreOnlineBtn'))
const uploadLabel = computed(() => t('common.uploadMedia'))

function itemKey(item: MediaItemVM, idx: number): string {
  if (item.ref.scope === 'local') {
    return `local:${item.ref.domain}:${item.ref.id}:${idx}`
  }
  return `${item.ref.scope}:${item.ref.domain}:${item.ref.sourceId}:${item.ref.ref}:${idx}`
}
</script>

<template>
  <div class="mx-auto w-full space-y-8" :class="maxWidthClass">
    <!-- Minimal single-line header (zero stat cards) -->
    <header class="flex items-center justify-between gap-3">
      <div class="flex min-w-0 flex-wrap items-baseline gap-x-3 gap-y-0.5">
        <h1 class="text-headline-small truncate text-on-surface">
          {{ resolvedTitle }}
        </h1>
        <p v-if="summary" class="text-body-small truncate text-on-surface-variant">
          {{ summary }}
        </p>
      </div>

      <div class="flex shrink-0 items-center gap-1">
        <slot name="header-actions" />
        <MdIconButton
          v-if="showRefresh"
          variant="standard"
          size="sm"
          :icon="LuRefreshCw"
          :loading="refreshing"
          :aria-label="t('common.refresh')"
          :tooltip="t('common.refresh')"
          @click="emit('refresh')"
        />
      </div>
    </header>

    <!-- Error state (replaces empty state) -->
    <MediaErrorState
      v-if="error && !hasAnyContent"
      :message="error"
      :retrying="loading || refreshing"
      @retry="emit('retry')"
    />

    <!-- Initial loading skeleton (delayed to avoid flash on fast navigations) -->
    <div v-else-if="loading && !hasAnyContent" class="space-y-8" aria-busy="true">
      <template v-if="showSkeleton">
        <section class="space-y-3">
          <MdSkeleton variant="text" width="10rem" height="1.5rem" />
          <div class="flex gap-4 overflow-hidden">
            <MdSkeleton
              v-for="idx in 3"
              :key="`cont-sk-${idx}`"
              variant="rect"
              width="18rem"
              height="5.5rem"
              class="shrink-0"
            />
          </div>
        </section>

        <section class="space-y-3">
          <MdSkeleton variant="text" width="8rem" height="1.5rem" />
          <div :class="gridClass">
            <MdSkeleton
              v-for="idx in skeletonCount"
              :key="`rec-sk-${idx}`"
              variant="card"
              :aspect="resolvedAspect"
            />
          </div>
        </section>
      </template>
    </div>

    <!-- Unified empty library onboarding -->
    <MdEmptyState
      v-else-if="!loading && !error && !hasAnyContent"
      :icon="domainIcon"
      :title="resolvedEmptyTitle"
      :description="resolvedEmptyDescription"
    >
      <template #action>
        <slot name="empty-actions">
          <template v-if="resolvedIsAdmin">
            <MdButton variant="filled" :icon="LuFolderPlus" :to="resolvedAddLibraryTo">
              {{ addLibraryLabel }}
            </MdButton>
            <MdButton
              v-if="canUpload"
              variant="tonal"
              :icon="LuUpload"
              @click="emit('upload')"
            >
              {{ uploadLabel }}
            </MdButton>
            <MdButton variant="tonal" :icon="LuDatabase" :to="resolvedBrowsePoolTo">
              {{ browsePoolLabel }}
            </MdButton>
            <MdButton
              v-if="hasOnlineSources"
              variant="text"
              :icon="LuCompass"
              :to="resolvedExploreOnlineTo"
            >
              {{ exploreOnlineLabel }}
            </MdButton>
          </template>
          <template v-else>
            <MdButton variant="filled" :icon="LuDatabase" :to="resolvedBrowsePoolTo">
              {{ browsePoolLabel }}
            </MdButton>
            <MdButton
              v-if="hasOnlineSources"
              variant="tonal"
              :icon="LuCompass"
              :to="resolvedExploreOnlineTo"
            >
              {{ exploreOnlineLabel }}
            </MdButton>
          </template>
        </slot>
      </template>
    </MdEmptyState>

    <!-- Primary Home Content (max 3 sections) -->
    <template v-else>
      <MediaErrorState
        v-if="error"
        compact
        :message="error"
        :retrying="refreshing"
        @retry="emit('retry')"
      />

      <!-- Section 1: Cross-Source Continue Rail -->
      <section v-if="continueItems.length > 0" class="space-y-3">
        <div class="flex items-center justify-between gap-2">
          <h2 class="text-title-large text-on-surface">
            {{ resolvedContinueTitle }}
          </h2>
          <MdButton variant="text" size="sm" :to="resolvedContinueSeeAll">
            <span>{{ t('common.viewAll') }}</span>
            <template #trailing>
              <LuChevronRight class="h-4 w-4" aria-hidden="true" />
            </template>
          </MdButton>
        </div>

        <div class="no-scrollbar -mx-1 flex snap-x snap-mandatory gap-4 overflow-x-auto px-1 pb-2">
          <div
            v-for="(item, idx) in continueItems"
            :key="itemKey(item, idx)"
            class="w-72 shrink-0 snap-start sm:w-80"
          >
            <slot name="continue-card" :item="item">
              <MediaItemCard
                :item="item"
                :aspect="resolvedAspect"
                layout="horizontal"
                prefer-play-route
                @select="emit('select-item', $event)"
              />
            </slot>
          </div>
        </div>
      </section>

      <!-- Section 2: Recently Added Borderless Grid -->
      <section v-if="recentItems.length > 0" class="space-y-3">
        <div class="flex items-center justify-between gap-2">
          <h2 class="text-title-large text-on-surface">
            {{ resolvedRecentTitle }}
          </h2>
          <MdButton variant="text" size="sm" :to="resolvedRecentSeeAll">
            <span>{{ t('common.viewAll') }}</span>
            <template #trailing>
              <LuChevronRight class="h-4 w-4" aria-hidden="true" />
            </template>
          </MdButton>
        </div>

        <div :class="gridClass">
          <template v-for="(item, idx) in recentItems" :key="itemKey(item, idx)">
            <slot name="recent-card" :item="item">
              <MediaItemCard
                :item="item"
                :aspect="resolvedAspect"
                :cover-style="resolvedCoverStyle"
                @select="emit('select-item', $event)"
              />
            </slot>
          </template>
        </div>
      </section>

      <!-- Optional 3rd section (e.g. Recommendations / Random) -->
      <slot name="extra-sections" />
    </template>
  </div>
</template>
