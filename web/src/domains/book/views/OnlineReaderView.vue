<script setup lang="ts">
import { computed, ref } from 'vue'
import { useI18n as useGlobalI18n } from 'vue-i18n'
import {
  LuBookMarked,
  LuDownload,
  LuHeart,
  LuList,
  LuMoreVertical,
  LuRefreshCw,
  LuRepeat,
  LuSlidersHorizontal,
} from 'vue-icons-plus/lu'
import { ImmersiveMediaTemplate } from '@/components/domain'
import { MdButton, MdIconButton, MdMenu, MdMenuItem } from '@/components/ui'
import BookSourceSwitchDialog from '@/domains/book/components/BookSourceSwitchDialog.vue'
import OnlineDownloadModal from '@/domains/book/components/OnlineDownloadModal.vue'
import { useOnlineReader } from '@/domains/book/components/online-reader/useOnlineReader'
import BookTocDrawer from '@/domains/book/components/reader/BookTocDrawer.vue'
import BookTypographySheet from '@/domains/book/components/reader/BookTypographySheet.vue'
import { useReaderAppearance } from '@/domains/book/composables/useReaderAppearance'
import { useBookI18n as useI18n } from '@/domains/book/i18n'
import ContinuousTextArticle from '@/domains/book/readers/ContinuousTextArticle.vue'

const { t } = useI18n()
const { t: globalT } = useGlobalI18n()
const readerAppearance = useReaderAppearance()

const articleRef = ref<InstanceType<typeof ContinuousTextArticle> | null>(null)
const contentScrollRef = computed<HTMLElement | null>(
  () => articleRef.value?.scrollContainerRef ?? null,
)
const hudVisible = ref(true)

const {
  readerCtx,
  chapters,
  tocItems,
  activeTocHref,
  activeIndex,
  loadedChapters,
  loadingToc,
  loadingContent,
  combinedLoading,
  appendingNext,
  appendError,
  errorMsg,
  savingShelf,
  favorite,
  togglingFavorite,
  toggleFavorite,
  activeOverlay,
  switchingKey,
  currentChapter,
  chapterTitle,
  contentCached,
  prevReadableIndex,
  nextAppendableIndex,
  downloadTarget,
  openOverlay,
  closeOverlay,
  loadTocAndOpen,
  openChapter,
  selectTocHref,
  appendNextChapter,
  handleContentScroll,
  handleSaveToShelf,
  handleSelectAltSource,
  handleBackToCatalog,
} = useOnlineReader(contentScrollRef)

const activeSheet = computed<string | null>({
  get: () =>
    activeOverlay.value === 'toc' || activeOverlay.value === 'typography'
      ? activeOverlay.value
      : null,
  set: (val) => {
    if (val === 'toc' || val === 'typography') {
      activeOverlay.value = val
    } else if (activeOverlay.value === 'toc' || activeOverlay.value === 'typography') {
      activeOverlay.value = null
    }
  },
})

const headerSubtitle = computed(() => {
  const parts: string[] = []
  if (chapterTitle.value) parts.push(chapterTitle.value)
  if (chapters.value.length > 0) parts.push(`${activeIndex.value + 1} / ${chapters.value.length}`)
  if (readerCtx.value?.sourceName) parts.push(readerCtx.value.sourceName)
  if (contentCached.value) parts.push(t('online.cachedTag'))
  return parts.join(' · ')
})

const sheetTitle = computed(() => {
  if (activeSheet.value === 'toc') return `${t('reader.toc')} (${chapters.value.length})`
  if (activeSheet.value === 'typography') return t('reader.typography')
  return ''
})

const sheetSubtitle = computed(() =>
  activeSheet.value === 'typography' ? t('reader.typographySubtitle') : '',
)

function handleRetryPrimary(): void {
  if (currentChapter.value) {
    void openChapter(activeIndex.value, true)
  } else {
    void loadTocAndOpen(activeIndex.value)
  }
}
</script>

<template>
  <ImmersiveMediaTemplate
    v-model:hud-visible="hudVisible"
    v-model:active-sheet="activeSheet"
    :title="readerCtx?.name || ''"
    :subtitle="headerSubtitle"
    stage-bg-class="bg-surface text-on-surface"
    :style="readerAppearance.stageStyle.value"
    :auto-hide="true"
    :lock-hud="
      combinedLoading ||
      Boolean(errorMsg) ||
      activeOverlay === 'switch-source' ||
      activeOverlay === 'download'
    "
    center-tap-mode="full-stage"
    :back-label="t('reader.backToBook')"
    :sheet-title="sheetTitle"
    :sheet-subtitle="sheetSubtitle"
    sheet-size="sm"
    :escape-to-exit="true"
    @back="handleBackToCatalog"
  >
    <template #header-controls>
      <MdButton
        v-if="readerCtx"
        variant="tonal"
        :icon="LuBookMarked"
        :loading="savingShelf"
        @click="handleSaveToShelf"
      >
        {{ globalT('pool.bindBtn') }}
      </MdButton>

      <MdIconButton
        v-if="readerCtx"
        size="sm"
        :variant="favorite ? 'tonal' : 'standard'"
        :selected="favorite"
        :disabled="togglingFavorite"
        :aria-label="favorite ? t('detail.favoriteRemove') : t('detail.favoriteAdd')"
        :tooltip="favorite ? t('detail.favoriteRemove') : t('detail.favoriteAdd')"
        @click="toggleFavorite"
      >
        <LuHeart class="h-4 w-4" :class="favorite ? 'fill-current' : ''" aria-hidden="true" />
      </MdIconButton>

      <MdIconButton
        size="sm"
        variant="standard"
        :selected="activeOverlay === 'toc'"
        :icon="LuList"
        :aria-label="t('reader.toc')"
        :tooltip="t('reader.toc')"
        @click="openOverlay('toc')"
      />

      <MdIconButton
        size="sm"
        variant="standard"
        :selected="activeOverlay === 'typography'"
        :icon="LuSlidersHorizontal"
        :aria-label="t('reader.typography')"
        :tooltip="t('reader.typography')"
        @click="openOverlay('typography')"
      />

      <MdMenu placement="bottom-end" :aria-label="t('detail.moreActions')">
        <template #trigger="{ toggle, triggerAttrs }">
          <MdIconButton
            v-bind="triggerAttrs"
            size="sm"
            variant="standard"
            :icon="LuMoreVertical"
            :aria-label="t('detail.moreActions')"
            :tooltip="t('detail.moreActions')"
            @click="toggle"
          />
        </template>

        <MdMenuItem
          :icon="LuRefreshCw"
          :label="t('online.refreshChapter')"
          :disabled="loadingContent"
          @select="openChapter(activeIndex, true)"
        />
        <MdMenuItem
          :icon="LuRepeat"
          :label="t('online.switchSource')"
          @select="openOverlay('switch-source')"
        />
        <MdMenuItem
          :icon="LuDownload"
          :label="t('online.downloadPack')"
          @select="openOverlay('download')"
        />
      </MdMenu>
    </template>

    <ContinuousTextArticle
      ref="articleRef"
      :chapters="loadedChapters"
      :active-pos="activeIndex"
      :total-chapters="chapters.length"
      :loading="combinedLoading"
      :loading-label="loadingToc ? t('reader.loadingToc') : t('reader.loadingContent')"
      :error-msg="errorMsg"
      :appending-next="appendingNext"
      :append-error="appendError"
      :has-prev="prevReadableIndex !== null"
      :has-next="nextAppendableIndex !== null"
      show-toc-button
      show-switch-source-on-error
      @scroll="handleContentScroll"
      @retry="handleRetryPrimary"
      @retry-append="appendNextChapter"
      @prev-chapter="prevReadableIndex !== null && openChapter(prevReadableIndex)"
      @next-chapter="nextAppendableIndex !== null && openChapter(nextAppendableIndex)"
      @open-toc="openOverlay('toc')"
      @switch-source="openOverlay('switch-source')"
    />

    <template v-if="activeSheet === 'toc'" #sheet-header-actions>
      <MdButton variant="tonal" :icon="LuRepeat" @click="openOverlay('switch-source')">
        {{ t('online.switchSource') }}
      </MdButton>
    </template>

    <template #sheet>
      <BookTocDrawer
        v-if="activeSheet === 'toc'"
        :items="tocItems"
        :active-href="activeTocHref"
        searchable
        @select="selectTocHref"
      />

      <BookTypographySheet v-else-if="activeSheet === 'typography'" />
    </template>

    <template #overlay>
      <BookSourceSwitchDialog
        :open="activeOverlay === 'switch-source'"
        :book-title="readerCtx?.name || ''"
        :current-source-id="readerCtx?.sourceId"
        :current-book-url="readerCtx?.bookUrl"
        :switching-key="switchingKey"
        @close="closeOverlay"
        @select="(item) => handleSelectAltSource(item)"
      />

      <OnlineDownloadModal
        :open="activeOverlay === 'download'"
        :target="downloadTarget"
        @close="closeOverlay"
      />
    </template>
  </ImmersiveMediaTemplate>
</template>
