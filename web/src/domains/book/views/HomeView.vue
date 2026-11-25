<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { DomainHomeTemplate } from '@/components/domain'
import { useHomeOverview } from '@/composables/useHomeOverview'
import { useBookI18n as useI18n } from '@/domains/book/i18n'
import { useCatalogStore } from '@/domains/book/store'
import { bookSummaryToVM } from '@/domains/book/utils/bookVm'
import { useMergedShelf, useSourceFilter, type MediaItemVM } from '@/sources'
import { useAuthStore } from '@/stores/auth'
import { useUploadStore } from '@/stores/uploads'

defineOptions({ name: 'BookHomeView' })

const { t } = useI18n()
const authStore = useAuthStore()
const catalogStore = useCatalogStore()
const uploadStore = useUploadStore()

const uploadInputRef = ref<HTMLInputElement | null>(null)
const refreshing = ref(false)

const sourceFilter = useSourceFilter('book')

const { overview, loading: overviewLoading, error: overviewError, refreshPage } = useHomeOverview(
  'book',
  {
    refreshSidebar: () => catalogStore.refreshSidebarData(),
    watchSources: () => [uploadStore.versionCounter, catalogStore.scanRefreshVersion],
  },
)

const continueShelf = useMergedShelf('book', 'continue', { limit: 12 })

watch(
  () => [uploadStore.versionCounter, catalogStore.scanRefreshVersion],
  () => {
    void continueShelf.refresh()
  },
)

const summaryText = computed(() => {
  const stats = overview.value?.stats
  if (!stats || stats.total_books <= 0) return ''
  if (stats.reading_count > 0) {
    return t('home.summaryBooksReading', {
      total: stats.total_books,
      reading: stats.reading_count,
    })
  }
  return t('home.summaryBooks', { total: stats.total_books })
})

const recentItems = computed<MediaItemVM[]>(() =>
  (overview.value?.recently_added ?? []).map((book) =>
    bookSummaryToVM(book, {
      unknownAuthor: t('catalog.unknownAuthor'),
      missingLabel: t('catalog.missingBadge'),
    }),
  ),
)

const combinedLoading = computed(
  () =>
    (overviewLoading.value && !overview.value) ||
    (continueShelf.loading.value &&
      continueShelf.items.value.length === 0 &&
      recentItems.value.length === 0),
)

const combinedError = computed(() => overviewError.value || continueShelf.error.value)

async function handleRefresh(): Promise<void> {
  if (refreshing.value) return
  refreshing.value = true
  try {
    await Promise.allSettled([
      refreshPage(),
      continueShelf.refresh(),
      sourceFilter.refresh(true),
    ])
  } finally {
    refreshing.value = false
  }
}

function triggerUpload(): void {
  if (!authStore.isAdmin) return
  uploadInputRef.value?.click()
}

function onUploadFilesChange(ev: Event): void {
  const input = ev.target as HTMLInputElement
  if (input.files && input.files.length > 0) {
    uploadStore.enqueueFiles(input.files)
  }
  input.value = ''
}
</script>

<template>
  <div class="px-4 py-5 sm:px-6 sm:py-6">
    <DomainHomeTemplate
      domain="book"
      :summary="summaryText"
      :continue-items="continueShelf.items.value"
      :recent-items="recentItems"
      :continue-title="t('home.continueReading')"
      :recent-title="t('home.recentlyAdded')"
      aspect="poster"
      cover-style="book"
      :loading="combinedLoading"
      :refreshing="refreshing"
      :error="combinedError"
      :is-admin="authStore.isAdmin"
      :can-upload="authStore.isAdmin"
      :has-online-sources="sourceFilter.hasOnlineSources.value"
      :empty-title="t('home.welcomeEmptyTitle')"
      :empty-description="authStore.isAdmin ? t('home.welcomeEmptyDesc') : undefined"
      @refresh="handleRefresh"
      @retry="handleRefresh"
      @upload="triggerUpload"
    />

    <input
      v-if="authStore.isAdmin"
      ref="uploadInputRef"
      type="file"
      multiple
      accept=".epub,.txt,.pdf,.mobi,.azw3,.fb2"
      class="hidden"
      @change="onUploadFilesChange"
    />
  </div>
</template>
