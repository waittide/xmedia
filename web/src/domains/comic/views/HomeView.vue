<script setup lang="ts">
import { computed, ref } from 'vue'
import { useI18n as useGlobalI18n } from 'vue-i18n'
import { LuBookOpen, LuChevronRight } from 'vue-icons-plus/lu'
import { DomainHomeTemplate, MediaItemCard } from '@/components/domain'
import { MdButton } from '@/components/ui'
import { useHomeOverview } from '@/composables/useHomeOverview'
import { useComicI18n as useI18n } from '@/domains/comic/i18n'
import { useSidebarStore } from '@/domains/comic/store'
import { seriesToMediaItemVM } from '@/domains/comic/utils/mappers'
import { libraryRoute, useMergedShelf, useSourceFilter, type MediaItemVM } from '@/sources'
import { useUploadStore } from '@/stores/uploads'

defineOptions({ name: 'ComicHomeView' })

const { t } = useI18n()
const { t: globalT } = useGlobalI18n()
const sidebarStore = useSidebarStore()
const upload = useUploadStore()

const fileInputRef = ref<HTMLInputElement | null>(null)
const refreshing = ref(false)

const { overview, loading: overviewLoading, error: overviewError, refreshPage } = useHomeOverview(
  'comic',
  {
    refreshSidebar: () => sidebarStore.refresh(),
    watchSources: () => upload.catalogVersion,
  },
)

const continueShelf = useMergedShelf('comic', 'continue', { limit: 12 })
const favoritesShelf = useMergedShelf('comic', 'favorites', { limit: 12 })
const sourceFilter = useSourceFilter('comic')

const summaryText = computed(() => {
  const st = overview.value?.stats
  if (!st || st.series_count <= 0) return ''
  const reading = Math.max(st.in_progress_count, continueShelf.items.value.length)
  if (reading > 0) {
    return t('home.summary', { series: st.series_count, reading })
  }
  return t('home.summarySeriesOnly', { series: st.series_count })
})

const localRecentItems = computed<MediaItemVM[]>(() =>
  (overview.value?.recently_updated ?? []).map((series) =>
    seriesToMediaItemVM(series, {
      chaptersCountLabel: t('home.chaptersCount', { count: series.chapter_count }),
      statusLabel:
        series.status === 'completed'
          ? t('series.statusCompleted')
          : series.status === 'hiatus'
            ? t('series.statusHiatus')
            : t('series.statusOngoing'),
    }),
  ),
)

const hasLocalRecent = computed(() => localRecentItems.value.length > 0)
const hasContinue = computed(() => continueShelf.items.value.length > 0)
const hasFavorites = computed(() => favoritesShelf.items.value.length > 0)

const promoteFavoritesToPrimary = computed(
  () => !hasContinue.value && !hasLocalRecent.value && hasFavorites.value,
)

const primaryRecentItems = computed<MediaItemVM[]>(() =>
  promoteFavoritesToPrimary.value ? favoritesShelf.items.value : localRecentItems.value,
)

const primaryRecentTitle = computed(() =>
  promoteFavoritesToPrimary.value ? t('home.favoritesSection') : t('home.recentlyUpdated'),
)

const primaryRecentSeeAll = computed(() =>
  promoteFavoritesToPrimary.value
    ? libraryRoute('comic', { tab: 'favorites' })
    : libraryRoute('comic'),
)

const showExtraFavorites = computed(
  () => !promoteFavoritesToPrimary.value && hasFavorites.value,
)

const combinedLoading = computed(
  () =>
    (overviewLoading.value && !overview.value) ||
    (continueShelf.loading.value &&
      !hasContinue.value &&
      !hasLocalRecent.value &&
      !hasFavorites.value),
)

const combinedError = computed(() => {
  if (hasContinue.value || hasLocalRecent.value || hasFavorites.value) {
    return overviewError.value
  }
  return overviewError.value || continueShelf.error.value
})

function onPickUpload(ev: Event): void {
  const input = ev.target as HTMLInputElement
  if (input.files && input.files.length > 0) {
    upload.enqueueFiles(input.files)
    input.value = ''
  }
}

function triggerUpload(): void {
  fileInputRef.value?.click()
}

async function handleRefresh(): Promise<void> {
  refreshing.value = true
  try {
    await Promise.allSettled([
      refreshPage(),
      continueShelf.refresh(),
      favoritesShelf.refresh(),
      sourceFilter.refresh(true),
    ])
  } finally {
    refreshing.value = false
  }
}

function itemKey(item: MediaItemVM, idx: number): string {
  if (item.ref.scope === 'local') return `local:${item.ref.id}:${idx}`
  return `${item.ref.scope}:${item.ref.sourceId}:${item.ref.ref}:${idx}`
}
</script>

<template>
  <div class="px-4 py-6 sm:px-6 md:px-8">
    <input
      ref="fileInputRef"
      type="file"
      multiple
      accept=".cbz,.zip"
      class="hidden"
      @change="onPickUpload"
    />

    <DomainHomeTemplate
      domain="comic"
      :title="t('home.title')"
      :summary="summaryText"
      :continue-items="continueShelf.items.value"
      :recent-items="primaryRecentItems"
      :continue-title="t('home.continueReading')"
      :recent-title="primaryRecentTitle"
      :continue-see-all-to="libraryRoute('comic', { tab: 'continue' })"
      :recent-see-all-to="primaryRecentSeeAll"
      aspect="poster"
      cover-style="book"
      :loading="combinedLoading"
      :refreshing="refreshing"
      :error="combinedError"
      :has-online-sources="sourceFilter.hasOnlineSources.value || true"
      :empty-title="t('home.emptyTitle')"
      :empty-description="t('home.emptyDesc')"
      @refresh="handleRefresh"
      @retry="handleRefresh"
      @upload="triggerUpload"
    >
      <template v-if="showExtraFavorites" #extra-sections>
        <section class="space-y-3">
          <div class="flex items-center justify-between gap-2">
            <h2 class="text-title-large text-on-surface">
              {{ t('home.favoritesSection') }}
            </h2>
            <MdButton
              variant="text"
              size="sm"
              :to="libraryRoute('comic', { tab: 'favorites' })"
            >
              <span>{{ globalT('common.viewAll') }}</span>
              <template #trailing>
                <LuChevronRight class="h-4 w-4" aria-hidden="true" />
              </template>
            </MdButton>
          </div>

          <div
            class="grid grid-cols-2 gap-5 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6"
          >
            <MediaItemCard
              v-for="(item, idx) in favoritesShelf.items.value"
              :key="itemKey(item, idx)"
              :item="item"
              aspect="poster"
              cover-style="book"
              :fallback-icon="LuBookOpen"
            />
          </div>
        </section>
      </template>
    </DomainHomeTemplate>
  </div>
</template>
