<script setup lang="ts">
import { ref } from 'vue'
import { useI18n as useGlobalI18n } from 'vue-i18n'
import {
  LuBookOpen,
  LuBookmark,
  LuCheckCheck,
  LuCloud,
  LuImage,
  LuPencil,
  LuPlay,
  LuRefreshCw,
  LuTrash2,
  LuUpload,
  LuX,
} from 'vue-icons-plus/lu'
import { MediaDetailTemplate } from '@/components/domain'
import { MdMenuItem } from '@/components/ui'
import { withAuthUrl } from '@/domains/comic/api'
import ChapterMetadataModal from '@/domains/comic/components/ChapterMetadataModal.vue'
import ComicSeriesChaptersSection from '@/domains/comic/components/ComicSeriesChaptersSection.vue'
import ComicSeriesShelvesDialog from '@/domains/comic/components/ComicSeriesShelvesDialog.vue'
import SeriesMetadataModal from '@/domains/comic/components/SeriesMetadataModal.vue'
import { useComicSeriesDetail } from '@/domains/comic/composables/useComicSeriesDetail'
import { useComicI18n as useI18n } from '@/domains/comic/i18n'
import { libraryRoute } from '@/sources'
import { useAuthStore } from '@/stores/auth'

const { t } = useI18n()
const { t: globalT } = useGlobalI18n()
const auth = useAuthStore()

const coverInputRef = ref<HTMLInputElement | null>(null)
const chapterUploadRef = ref<HTMLInputElement | null>(null)

const {
  detail,
  loading,
  loadError,
  editSeriesOpen,
  editingChapter,
  shelfModalOpen,
  allShelves,
  selectedShelfIds,
  savingShelves,
  refreshingTrackedToc,
  cachingSeries,
  cachingChapterIds,
  isManagedSeries,
  isTrackedSeries,
  trackedCacheStats,
  heroSubtitle,
  overallProgressPercent,
  allCompleted,
  primaryActionLabel,
  detailTags,
  detailSpecs,
  loadDetail,
  refreshPage,
  primaryReadAction,
  openChapter,
  toggleFavorite,
  openShelfDialog,
  setShelfChecked,
  saveShelvesSelection,
  onSeriesMetadataSaved,
  toggleChapterRead,
  handleMarkPreviousRead,
  handleMarkSeriesRead,
  handleMarkVolumeRead,
  onPickCover,
  onUploadChaptersToSeries,
  handleDeleteSeries,
  handleDeleteChapter,
  handleRefreshTrackedToc,
  handleCacheAllChapters,
  handleUnbindSeries,
  handleCacheChapter,
} = useComicSeriesDetail()
</script>

<template>
  <div class="px-4 py-6 sm:px-6 md:px-8">
    <input
      v-if="auth.isAdmin && isManagedSeries"
      ref="chapterUploadRef"
      type="file"
      multiple
      accept=".cbz,.zip,.epub"
      class="hidden"
      @change="onUploadChaptersToSeries"
    />
    <input
      v-if="auth.isAdmin"
      ref="coverInputRef"
      type="file"
      accept="image/*"
      class="hidden"
      @change="onPickCover"
    />

    <MediaDetailTemplate
      :title="detail?.series.title || ''"
      :subtitle="heroSubtitle"
      :cover="detail?.cover_url ? withAuthUrl(detail.cover_url) : null"
      aspect="poster"
      cover-style="book"
      :fallback-icon="LuBookOpen"
      :show-back="true"
      :back-label="t('series.backToLibrary')"
      :back-to="libraryRoute('comic')"
      :progress-percent="overallProgressPercent"
      :progress-label="
        detail
          ? t('series.progressChapters', {
              done: detail.completed_chapters,
              total: detail.series.chapter_count,
            })
          : ''
      "
      :primary-action-label="primaryActionLabel"
      :primary-action-icon="LuPlay"
      :primary-action-disabled="!detail || detail.chapters.length === 0"
      :secondary-action-label="t('series.addToShelf')"
      :secondary-action-icon="LuBookmark"
      :show-favorite="Boolean(detail)"
      :favorite="Boolean(detail?.is_favorite)"
      :description="detail?.series.summary || null"
      :tags="detailTags"
      :specs="detailSpecs"
      :specs-title="t('series.specsTitle')"
      :loading="loading && !detail"
      :error="loadError"
      @primary-action="primaryReadAction"
      @secondary-action="openShelfDialog"
      @toggle-favorite="toggleFavorite"
      @retry="loadDetail"
    >
      <template v-if="detail && isTrackedSeries" #hero-meta>
        <div class="mt-1 flex flex-wrap items-center gap-2">
          <span
            class="text-label-medium inline-flex items-center gap-1.5 rounded-corner-full bg-secondary-container px-2.5 py-0.5 text-on-secondary-container"
          >
            <LuCloud class="h-3.5 w-3.5 shrink-0" aria-hidden="true" />
            <span>
              {{
                t('online.trackedBadgeStatus', {
                  cached: trackedCacheStats.cached,
                  total: trackedCacheStats.total,
                })
              }}
            </span>
          </span>
        </div>
      </template>

      <template v-if="detail" #overflow-menu="{ close }">
        <template v-if="isTrackedSeries">
          <MdMenuItem
            :label="
              refreshingTrackedToc ? t('online.refreshingTrackedToc') : t('online.refreshTrackedToc')
            "
            :icon="LuRefreshCw"
            :disabled="refreshingTrackedToc"
            @select="
              close();
              handleRefreshTrackedToc()
            "
          />
          <MdMenuItem
            v-if="trackedCacheStats.uncached > 0"
            :label="cachingSeries ? t('online.cachingAllChapters') : t('online.cacheAllChapters')"
            :icon="LuCloud"
            :disabled="cachingSeries"
            @select="
              close();
              handleCacheAllChapters()
            "
          />
        </template>
        <MdMenuItem
          v-if="auth.isAdmin && isManagedSeries"
          :label="t('series.uploadToSeries')"
          :icon="LuUpload"
          @select="
            close();
            chapterUploadRef?.click()
          "
        />
        <MdMenuItem
          :label="allCompleted ? t('series.markAllUnread') : t('series.markAllRead')"
          :icon="LuCheckCheck"
          @select="
            close();
            handleMarkSeriesRead(!allCompleted)
          "
        />
        <MdMenuItem
          v-if="auth.isAdmin"
          :label="t('series.editMetadata')"
          :icon="LuPencil"
          @select="
            close();
            editSeriesOpen = true
          "
        />
        <MdMenuItem
          v-if="auth.isAdmin"
          :label="t('series.changeCover')"
          :icon="LuImage"
          @select="
            close();
            coverInputRef?.click()
          "
        />
        <MdMenuItem
          :label="globalT('common.refresh')"
          :icon="LuRefreshCw"
          @select="
            close();
            refreshPage()
          "
        />
      </template>

      <template v-if="detail" #overflow-danger="{ close }">
        <MdMenuItem
          :label="globalT('pool.unbindBtn')"
          :icon="LuX"
          destructive
          @select="
            close();
            handleUnbindSeries()
          "
        />
        <MdMenuItem
          v-if="auth.isAdmin"
          :label="t('series.deleteSeries')"
          :icon="LuTrash2"
          destructive
          @select="
            close();
            handleDeleteSeries()
          "
        />
      </template>

      <ComicSeriesChaptersSection
        v-if="detail"
        :detail="detail"
        :is-admin="auth.isAdmin"
        :caching-chapter-ids="cachingChapterIds"
        @open-chapter="openChapter"
        @cache-chapter="handleCacheChapter"
        @toggle-chapter-read="toggleChapterRead"
        @mark-prev-read="handleMarkPreviousRead"
        @mark-volume-read="handleMarkVolumeRead"
        @edit-chapter="editingChapter = $event"
        @delete-chapter="handleDeleteChapter"
      />
    </MediaDetailTemplate>

    <SeriesMetadataModal
      :open="editSeriesOpen"
      :series="detail?.series ?? null"
      @close="editSeriesOpen = false"
      @saved="onSeriesMetadataSaved"
    />

    <ChapterMetadataModal
      :open="Boolean(editingChapter)"
      :chapter="editingChapter"
      @close="editingChapter = null"
      @saved="loadDetail"
    />

    <ComicSeriesShelvesDialog
      :open="shelfModalOpen"
      :shelves="allShelves"
      :selected-ids="selectedShelfIds"
      :saving="savingShelves"
      @close="shelfModalOpen = false"
      @toggle-shelf="setShelfChecked"
      @save="saveShelvesSelection"
    />
  </div>
</template>
