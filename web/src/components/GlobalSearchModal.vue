<script setup lang="ts">
import { computed, watch, type Component } from 'vue'
import { useRouter, type RouteLocationRaw } from 'vue-router'
import { useI18n } from 'vue-i18n'
import {
  LuAlertCircle,
  LuArrowRight,
  LuBookOpen,
  LuClapperboard,
  LuClock,
  LuDisc3,
  LuGlobe,
  LuLayers,
  LuSearch,
  LuSettings,
} from 'vue-icons-plus/lu'
import {
  MdButton,
  MdChip,
  MdEmptyState,
  MdListItem,
  MdSearchView,
  MdSkeleton,
} from '@/components/ui'
import { useUnifiedSearch, type MediaItemVM, type OnlineQuickAction, type SearchGroup } from '@/sources'
import { useAuthStore } from '@/stores/auth'
import { useDomainStore } from '@/stores/domain'
import type { MediaKind } from '@/types/api'

const props = defineProps<{
  open: boolean
}>()

const emit = defineEmits<{
  close: []
}>()

const { t } = useI18n()
const router = useRouter()
const authStore = useAuthStore()
const domainStore = useDomainStore()

const {
  query,
  groups,
  onlineActions,
  recent,
  loading,
  hasQuery,
  error,
  cancel,
  addRecent,
  removeRecent,
  clearRecent,
} = useUnifiedSearch({
  preferDomain: () => domainStore.activeDomain,
  limitPerGroup: 5,
  debounceMs: 250,
})

const DOMAIN_ICONS: Record<MediaKind, Component> = {
  book: LuBookOpen,
  comic: LuLayers,
  music: LuDisc3,
  video: LuClapperboard,
}

watch(
  () => props.open,
  (isOpen) => {
    if (isOpen) {
      query.value = ''
    } else {
      cancel()
    }
  },
)

const placeholderText = computed(() => {
  const names = authStore.enabledModules.map((m) => t(`app.domains.${m}`)).join(' / ')
  return `${t('common.search')} ${names}…`
})

const visibleGroups = computed<SearchGroup[]>(() =>
  groups.value.filter((g) => g.items.length > 0),
)

const allGroupsFailed = computed(
  () =>
    hasQuery.value &&
    groups.value.length > 0 &&
    groups.value.every((g) => g.status === 'error'),
)

const quickNavItems = computed(() => {
  const out: Array<{ key: string; label: string; icon: Component; to: RouteLocationRaw; domain?: MediaKind }> = []
  for (const kind of ['book', 'comic', 'music', 'video'] as const) {
    if (!authStore.hasModule(kind)) continue
    out.push({
      key: kind,
      label: t(`app.domains.${kind}`),
      icon: DOMAIN_ICONS[kind],
      to: `/${kind}`,
      domain: kind,
    })
  }
  out.push({
    key: 'settings',
    label: t('nav.settings'),
    icon: LuSettings,
    to: '/settings',
  })
  return out
})

/**
 * Uses canonical Stage 3C/4 routes when registered in router; gracefully translates
 * to current legacy routes before router switchover so navigation never hits catch-all.
 */
function resolveRoute(target: RouteLocationRaw): RouteLocationRaw {
  const matched = router.resolve(target)
  const isCatchAll =
    matched.matched.length === 0 ||
    matched.matched[matched.matched.length - 1]?.path === '/:pathMatch(.*)*'
  if (!isCatchAll) return target

  if (typeof target === 'string') return target
  const path = 'path' in target && typeof target.path === 'string' ? target.path : ''
  const queryObj = 'query' in target ? target.query : undefined

  const libMatch = /^\/(book|comic|music|video)\/library$/.exec(path)
  if (libMatch) {
    const d = libMatch[1] as MediaKind
    const legacyPath =
      d === 'comic' ? '/comic/series' : d === 'music' ? '/music/songs' : `/${d}/catalog`
    return { path: legacyPath, query: queryObj }
  }

  const itemMatch = /^\/(book|comic|music|video)\/item\/(.+)$/.exec(path)
  if (itemMatch) {
    const d = itemMatch[1] as MediaKind
    const id = itemMatch[2]!
    if (d === 'book') return { path: `/book/item/${id}`, query: queryObj }
    if (d === 'comic') return { path: `/comic/item/${id}`, query: queryObj }
    if (d === 'music') return { path: `/music/albums/${id}`, query: queryObj }
    return { path: `/video/shows/${id}`, query: queryObj }
  }

  const playMatch = /^\/(book|comic|music|video)\/play\/(.+)$/.exec(path)
  if (playMatch) {
    const d = playMatch[1] as MediaKind
    const id = playMatch[2]!
    if (d === 'book') return { path: `/book/read/${id}`, query: queryObj }
    if (d === 'comic') return { path: `/comic/read/${id}`, query: queryObj }
    if (d === 'video') return { path: `/video/watch/${id}`, query: queryObj }
  }

  return target
}

function navigateTo(target: RouteLocationRaw, domain?: MediaKind): void {
  if (query.value.trim()) {
    addRecent(query.value)
  }
  if (domain) {
    domainStore.setActiveDomain(domain)
  }
  emit('close')
  void router.push(resolveRoute(target))
}

function onSelectItem(item: MediaItemVM): void {
  navigateTo(item.to, item.ref.domain)
}

function onSelectSeeAll(group: SearchGroup): void {
  navigateTo(group.seeAll, group.domain)
}

function onSelectOnlineAction(action: OnlineQuickAction): void {
  navigateTo(action.to, action.domain)
}

function onSubmitSearch(): void {
  const trimmed = query.value.trim()
  if (!trimmed) return
  addRecent(trimmed)

  const preferredGroup = groups.value[0]
  if (preferredGroup) {
    navigateTo(preferredGroup.seeAll, preferredGroup.domain)
    return
  }

  const firstOnline = onlineActions.value[0]
  if (firstOnline) {
    navigateTo(firstOnline.to, firstOnline.domain)
  }
}

function applyRecentTerm(term: string): void {
  query.value = term
}
</script>

<template>
  <MdSearchView
    :open="open"
    :model-value="query"
    :placeholder="placeholderText"
    :loading="loading"
    :aria-label="t('common.search')"
    @update:model-value="query = $event"
    @submit="onSubmitSearch"
    @close="emit('close')"
  >
    <!-- Recent Searches & Quick Navigation (when query is empty) -->
    <template #history>
      <div class="space-y-5 px-1">
        <section v-if="recent.length > 0" class="space-y-2">
          <div class="flex items-center justify-between px-2">
            <span class="text-label-medium flex items-center gap-1.5 text-on-surface-variant">
              <LuClock class="h-3.5 w-3.5" aria-hidden="true" />
              <span>{{ t('globalSearch.recentSearches') }}</span>
            </span>
            <MdButton variant="text" size="sm" @click="clearRecent">
              {{ t('common.clear') }}
            </MdButton>
          </div>
          <div class="flex flex-wrap gap-2 px-2">
            <MdChip
              v-for="term in recent"
              :key="term"
              variant="input"
              :label="term"
              removable
              @click="applyRecentTerm(term)"
              @remove="removeRecent(term)"
            />
          </div>
        </section>

        <section class="space-y-1">
          <div class="text-label-medium px-2 py-1 text-on-surface-variant">
            {{ t('globalSearch.quickNavigation') }}
          </div>
          <div class="overflow-hidden rounded-corner-md">
            <MdListItem
              v-for="nav in quickNavItems"
              :key="nav.key"
              :headline="nav.label"
              :lines="1"
              clickable
              @click="navigateTo(nav.to, nav.domain)"
            >
              <template #leading>
                <component :is="nav.icon" class="h-5 w-5 text-on-surface-variant" aria-hidden="true" />
              </template>
              <template #trailing>
                <LuArrowRight class="h-4 w-4 text-on-surface-variant" aria-hidden="true" />
              </template>
            </MdListItem>
          </div>
        </section>
      </div>
    </template>

    <!-- Local Multi-Domain Search Results -->
    <template #results>
      <div v-if="hasQuery" class="space-y-4">
        <!-- Loading skeletons when first fetching -->
        <div v-if="loading && visibleGroups.length === 0" class="space-y-3 px-2 py-2">
          <MdSkeleton variant="text" width="30%" />
          <div v-for="n in 3" :key="`search-sk-${n}`" class="flex items-center gap-3">
            <MdSkeleton variant="rect" width="40px" height="40px" />
            <div class="flex-1 space-y-1.5">
              <MdSkeleton variant="text" width="60%" />
              <MdSkeleton variant="text" width="35%" />
            </div>
          </div>
        </div>

        <!-- Error alert if local search failed -->
        <div
          v-else-if="allGroupsFailed && error"
          role="alert"
          class="flex items-center gap-2.5 rounded-corner-md bg-error-container px-4 py-3 text-body-medium text-on-error-container"
        >
          <LuAlertCircle class="h-4 w-4 shrink-0" aria-hidden="true" />
          <span class="min-w-0 flex-1 truncate">{{ error }}</span>
        </div>

        <!-- Grouped local results -->
        <template v-else-if="visibleGroups.length > 0">
          <section v-for="group in visibleGroups" :key="group.key" class="space-y-1">
            <div class="flex items-center justify-between px-2">
              <span class="text-label-medium flex items-center gap-1.5 text-on-surface-variant">
                <component :is="DOMAIN_ICONS[group.domain]" class="h-3.5 w-3.5" aria-hidden="true" />
                <span>{{ group.title }}</span>
              </span>
              <MdButton variant="text" size="sm" @click="onSelectSeeAll(group)">
                {{ t('common.viewAll') }}
              </MdButton>
            </div>

            <div class="overflow-hidden rounded-corner-md">
              <MdListItem
                v-for="(item, idx) in group.items"
                :key="`${group.key}-${idx}`"
                :headline="item.title"
                :supporting-text="item.subtitle"
                :lines="item.subtitle ? 2 : 1"
                clickable
                @click="onSelectItem(item)"
              >
                <template #leading>
                  <img
                    v-if="item.cover"
                    :src="item.cover"
                    :alt="item.title"
                    loading="lazy"
                    class="h-10 w-10 rounded-corner-sm bg-surface-container-highest object-cover"
                  />
                  <div
                    v-else
                    class="flex h-10 w-10 items-center justify-center rounded-corner-sm bg-surface-container-highest text-on-surface-variant"
                  >
                    <component :is="DOMAIN_ICONS[group.domain]" class="h-5 w-5" aria-hidden="true" />
                  </div>
                </template>
                <template #trailing>
                  <LuArrowRight class="h-4 w-4 text-on-surface-variant" aria-hidden="true" />
                </template>
              </MdListItem>
            </div>
          </section>
        </template>

        <!-- Empty local state (online quick actions still render below) -->
        <MdEmptyState
          v-else-if="!loading"
          :icon="LuSearch"
          :title="t('common.noResults')"
          compact
        />
      </div>
    </template>

    <!-- 4-Domain Online Quick Actions (rendered only when query.trim() is non-empty) -->
    <template #online-actions>
      <div v-if="onlineActions.length > 0" class="space-y-1">
        <div class="text-label-medium flex items-center gap-1.5 px-2 py-1 text-on-surface-variant">
          <LuGlobe class="h-3.5 w-3.5" aria-hidden="true" />
          <span>{{ t('globalSearch.onlineSources') }}</span>
        </div>
        <div class="overflow-hidden rounded-corner-md">
          <MdListItem
            v-for="action in onlineActions"
            :key="`online-${action.domain}`"
            :headline="action.label"
            :supporting-text="action.subtitle"
            :lines="action.subtitle ? 2 : 1"
            clickable
            @click="onSelectOnlineAction(action)"
          >
            <template #leading>
              <div
                class="flex h-10 w-10 items-center justify-center rounded-corner-sm bg-secondary-container text-on-secondary-container"
              >
                <component :is="DOMAIN_ICONS[action.domain]" class="h-5 w-5" aria-hidden="true" />
              </div>
            </template>
            <template #trailing>
              <LuArrowRight class="h-4 w-4 text-on-surface-variant" aria-hidden="true" />
            </template>
          </MdListItem>
        </div>
      </div>
    </template>
  </MdSearchView>
</template>
