<script setup lang="ts">
import { computed, ref } from 'vue'
import { useI18n as useGlobalI18n } from 'vue-i18n'
import { RouterLink, useRoute } from 'vue-router'
import {
  LuBookOpen,
  LuCloud,
  LuDownload,
  LuFileCheck,
  LuFolder,
  LuImage,
  LuPencil,
  LuRefreshCw,
  LuRepeat,
  LuTrash2,
  LuX,
} from 'vue-icons-plus/lu'
import {
  MediaDetailTemplate,
  type MediaDetailSpec,
  type MediaDetailTag,
} from '@/components/domain'
import { MdIconButton, MdMenu, MdMenuItem } from '@/components/ui'
import { api } from '@/domains/book/api'
import BookDetailSections from '@/domains/book/components/BookDetailSections.vue'
import BookMetadataModal from '@/domains/book/components/BookMetadataModal.vue'
import BookSourceSwitchDialog from '@/domains/book/components/BookSourceSwitchDialog.vue'
import OnlineDownloadModal from '@/domains/book/components/OnlineDownloadModal.vue'
import { useBookDetailActions } from '@/domains/book/composables/useBookDetailActions'
import { useBookI18n as useI18n } from '@/domains/book/i18n'
import { useCatalogStore } from '@/domains/book/store'
import {
  formatProgressPercentValue,
  hasBookProgress,
  normalizeBookProgressPercent,
} from '@/domains/book/utils/bookVm'
import { libraryRoute, playRoute } from '@/sources'
import { useAuthStore } from '@/stores/auth'
import { useThemeStore } from '@/stores/theme'
import { formatBytes } from '@/utils/format'

const { t } = useI18n()
const { t: globalT } = useGlobalI18n()
const route = useRoute()
const authStore = useAuthStore()
const catalogStore = useCatalogStore()
const themeStore = useThemeStore()

const coverInputRef = ref<HTMLInputElement | null>(null)

const {
  book,
  toc,
  bookmarks,
  loading,
  loadError,
  trackedSourceName,
  refreshingToc,
  syncingEpub,
  togglingFavorite,
  togglingShelfIds,
  deletingBook,
  unbindingBook,
  deletingBookmarkIds,
  editModalOpen,
  switchSourceOpen,
  switchingSourceKey,
  onlineDownloadTarget,
  onlineDownloadJobId,
  trackedDownloadJob,
  onlineDownloadsStore,
  loadDetail,
  goBack,
  triggerFileDownload,
  toggleFavorite,
  toggleShelf,
  handleMetadataSaved,
  onCoverChange,
  handleRefreshTrackedToc,
  handleSyncTrackedEpub,
  handleSwitchTrackedSource,
  openTrackedBatchDownload,
  closeTrackedDownloadModal,
  handleUnbindBook,
  handleDeleteBook,
  deleteBookmark,
  openTocTargetInReader,
  openBookmarkInReader,
} = useBookDetailActions()

const hasProgress = computed(() => hasBookProgress(book.value?.progress))

const progressPercent = computed<number | null>(() =>
  normalizeBookProgressPercent(book.value?.progress),
)

const progressLabel = computed(() =>
  progressPercent.value !== null
    ? t('catalog.progressLabel', { percent: formatProgressPercentValue(progressPercent.value) })
    : '',
)

const formattedLanguage = computed(() => {
  const raw = book.value?.metadata.language
  if (!raw) return null
  try {
    const dn = new Intl.DisplayNames([themeStore.locale], { type: 'language' })
    return dn.of(raw) || raw
  } catch {
    return raw
  }
})

const detailTags = computed<MediaDetailTag[]>(() =>
  (book.value?.metadata.tags ?? []).map((tg) => ({
    label: `#${tg}`,
    value: tg,
    to: libraryRoute('book', { tag: tg }),
  })),
)

const detailSpecs = computed<MediaDetailSpec[]>(() => {
  const b = book.value
  if (!b) return []
  const items: MediaDetailSpec[] = [
    { label: t('detail.library'), value: b.library_name || '—' },
    {
      label: t('detail.sourceMode'),
      value: b.is_managed ? t('catalog.managedBadge') : t('catalog.mountedBadge'),
    },
    { label: t('detail.format'), value: b.format.toUpperCase(), mono: true },
    { label: t('detail.fileSize'), value: formatBytes(b.file_size) },
  ]
  if (b.online_tracking) {
    if (trackedSourceName.value) {
      items.push({ label: t('online.sourceLabel'), value: trackedSourceName.value })
    }
    items.push({
      label: t('reader.toc'),
      value: t('online.trackedBadgeStatus', {
        cached: b.online_tracking.cached_chapters,
        total: b.online_tracking.total_chapters || '?',
      }),
    })
  }
  if (b.metadata.page_count) {
    items.push({
      label: t('detail.structure'),
      value: t('detail.pages', { count: b.metadata.page_count }),
    })
  } else if (b.metadata.chapter_count) {
    items.push({
      label: t('detail.structure'),
      value: t('detail.chapters', { count: b.metadata.chapter_count }),
    })
  }
  if (formattedLanguage.value) {
    items.push({ label: t('detail.language'), value: formattedLanguage.value })
  }
  if (b.metadata.series) {
    const idxSuffix =
      b.metadata.series_index !== null && b.metadata.series_index !== undefined
        ? ` #${b.metadata.series_index}`
        : ''
    items.push({ label: t('detail.series'), value: `${b.metadata.series}${idxSuffix}` })
  }
  if (b.metadata.rating !== null && b.metadata.rating !== undefined) {
    items.push({ label: t('detail.rating'), value: `★ ${b.metadata.rating.toFixed(1)}` })
  }
  if (b.metadata.publisher) {
    items.push({ label: t('detail.publisher'), value: b.metadata.publisher })
  }
  if (b.metadata.published_at) {
    items.push({ label: t('detail.publishedAt'), value: b.metadata.published_at })
  }
  if (b.metadata.isbn) {
    items.push({ label: t('detail.isbn'), value: b.metadata.isbn, mono: true })
  }
  return items
})

const readRoute = computed(() =>
  book.value ? playRoute({ scope: 'local', domain: 'book', id: book.value.id }) : undefined,
)

const bookCoverUrl = computed(() =>
  book.value?.has_cover ? api.coverUrl(book.value.id, book.value.updated_at) : null,
)

const isBookInAnyShelf = computed(() => {
  const id = book.value?.id
  if (!id) return false
  return catalogStore.shelves.some((sh) => sh.book_ids.includes(id))
})
</script>

<template>
  <div class="px-4 py-5 sm:px-6 sm:py-6">
    <MediaDetailTemplate
      :title="book?.metadata.title || ''"
      :cover="bookCoverUrl"
      aspect="poster"
      cover-style="book"
      :fallback-icon="LuBookOpen"
      :back-label="t('detail.back')"
      :progress-percent="progressPercent"
      :progress-label="progressLabel"
      :primary-action-label="hasProgress ? t('catalog.continueReading') : t('catalog.startReading')"
      :primary-action-icon="LuBookOpen"
      :primary-action-to="readRoute"
      :primary-action-disabled="Boolean(book?.is_missing)"
      :favorite="Boolean(book?.is_favorite)"
      :show-favorite="Boolean(book)"
      :favorite-loading="togglingFavorite"
      :description="book?.metadata.description"
      :tags="detailTags"
      :specs="detailSpecs"
      :specs-title="t('detail.specifications')"
      :loading="loading"
      :error="loadError || (!loading && !book ? t('detail.notFound') : null)"
      :availability-warning="book?.is_missing ? t('catalog.missingBadge') : null"
      @back="goBack"
      @toggle-favorite="toggleFavorite"
      @retry="loadDetail(String(route.params.id))"
    >
      <template #hero-meta>
        <div v-if="book" class="space-y-1">
          <p class="text-body-medium text-on-surface-variant">
            <template v-if="book.metadata.authors.length > 0">
              <template v-for="(author, idx) in book.metadata.authors" :key="author">
                <RouterLink
                  :to="libraryRoute('book', { author })"
                  class="transition-colors duration-150 ease-standard hover:text-primary hover:underline"
                >
                  {{ author }}
                </RouterLink>
                <span v-if="idx < book.metadata.authors.length - 1">, </span>
              </template>
            </template>
            <span v-else>{{ t('catalog.unknownAuthor') }}</span>
          </p>

          <p class="text-label-medium flex flex-wrap items-center gap-x-2 gap-y-0.5 text-on-surface-variant">
            <span class="font-mono uppercase">{{ book.format }}</span>
            <template v-if="book.metadata.series">
              <span>·</span>
              <span>
                {{ book.metadata.series
                }}<template
                  v-if="book.metadata.series_index !== null && book.metadata.series_index !== undefined"
                >
                  #{{ book.metadata.series_index }}
                </template>
              </span>
            </template>
            <template v-if="book.metadata.rating !== null && book.metadata.rating !== undefined">
              <span>·</span>
              <span>★ {{ book.metadata.rating.toFixed(1) }}</span>
            </template>
            <template v-if="book.online_tracking">
              <span>·</span>
              <span>
                {{
                  t('online.trackedBadgeStatus', {
                    cached: book.online_tracking.cached_chapters,
                    total: book.online_tracking.total_chapters || '?',
                  })
                }}
              </span>
              <span
                v-if="book.online_tracking.epub_dirty"
                class="rounded-corner-xs bg-tertiary-container px-1.5 py-0.5 text-label-small text-on-tertiary-container"
              >
                {{ t('online.epubDirtyBadge') }}
              </span>
            </template>
          </p>
        </div>
      </template>

      <template #secondary-actions>
        <MdMenu
          v-if="book"
          placement="bottom-start"
          :min-width="208"
          :aria-label="t('detail.shelvesMenu')"
        >
          <template #trigger="{ toggle, triggerAttrs }">
            <MdIconButton
              v-bind="triggerAttrs"
              variant="tonal"
              :selected="isBookInAnyShelf"
              :icon="LuFolder"
              :aria-label="t('detail.shelvesMenu')"
              :tooltip="t('detail.shelvesMenu')"
              @click="toggle"
            />
          </template>

          <div
            v-if="catalogStore.shelves.length === 0"
            class="text-body-small px-3 py-2 text-on-surface-variant"
          >
            {{ t('detail.noShelvesYet') }}
          </div>
          <MdMenuItem
            v-for="sh in catalogStore.shelves"
            :key="sh.id"
            :label="sh.name"
            :selected="sh.book_ids.includes(book.id)"
            :disabled="togglingShelfIds.has(sh.id)"
            keep-open
            @select="toggleShelf(sh.id, sh.book_ids.includes(book.id))"
          />
        </MdMenu>
      </template>

      <template v-if="book" #overflow-menu>
        <MdMenuItem
          v-if="!book.is_missing"
          :icon="LuDownload"
          :label="book.online_tracking ? t('online.downloadEpub3') : t('catalog.download')"
          @select="triggerFileDownload"
        />

        <template v-if="book.online_tracking">
          <MdMenuItem
            :icon="LuRefreshCw"
            :label="t('online.refreshToc')"
            :disabled="refreshingToc"
            @select="handleRefreshTrackedToc"
          />
          <MdMenuItem
            v-if="book.online_tracking.epub_dirty"
            :icon="LuFileCheck"
            :label="syncingEpub ? t('online.syncingEpub') : t('online.syncEpub')"
            :disabled="syncingEpub"
            @select="handleSyncTrackedEpub"
          />
          <template v-if="authStore.isAdmin">
            <MdMenuItem
              :icon="LuRepeat"
              :label="t('online.switchSource')"
              @select="switchSourceOpen = true"
            />
            <MdMenuItem
              :icon="LuCloud"
              :label="t('online.cacheChapters')"
              @select="openTrackedBatchDownload"
            />
          </template>
        </template>

        <template v-if="authStore.isAdmin">
          <MdMenuItem
            :icon="LuPencil"
            :label="t('detail.editMetadata')"
            @select="editModalOpen = true"
          />
          <MdMenuItem
            :icon="LuImage"
            :label="t('detail.replaceCover')"
            @select="coverInputRef?.click()"
          />
        </template>
      </template>

      <template v-if="book" #overflow-danger>
        <MdMenuItem
          :icon="LuX"
          :label="globalT('pool.unbindBtn')"
          :disabled="unbindingBook"
          destructive
          @select="handleUnbindBook"
        />
        <MdMenuItem
          v-if="authStore.isAdmin"
          :icon="LuTrash2"
          :label="t('detail.deleteBook')"
          :disabled="deletingBook"
          destructive
          @select="handleDeleteBook"
        />
      </template>

      <BookDetailSections
        v-if="book"
        :book-id="book.id"
        :toc="toc"
        :active-locator="book.progress?.locator"
        :bookmarks="bookmarks"
        :deleting-bookmark-ids="deletingBookmarkIds"
        @select-toc="openTocTargetInReader"
        @open-bookmark="openBookmarkInReader"
        @delete-bookmark="deleteBookmark"
      />
    </MediaDetailTemplate>

    <input
      v-if="authStore.isAdmin"
      ref="coverInputRef"
      type="file"
      accept="image/*"
      class="hidden"
      @change="onCoverChange"
    />

    <BookMetadataModal
      :open="editModalOpen"
      :book="book"
      @close="editModalOpen = false"
      @saved="handleMetadataSaved"
    />

    <BookSourceSwitchDialog
      v-if="book?.online_tracking"
      :open="switchSourceOpen"
      :book-title="book.metadata.title"
      :current-source-id="book.online_tracking.source_id"
      :current-book-url="book.online_tracking.book_url"
      :switching-key="switchingSourceKey"
      show-clear-cache-option
      @close="switchSourceOpen = false"
      @select="handleSwitchTrackedSource"
    />

    <OnlineDownloadModal
      :open="onlineDownloadTarget !== null"
      :target="onlineDownloadTarget"
      :external-job="trackedDownloadJob"
      @close="closeTrackedDownloadModal"
      @job-updated="
        onlineDownloadJobId = $event.id;
        onlineDownloadsStore.trackJob($event)
      "
    />
  </div>
</template>
