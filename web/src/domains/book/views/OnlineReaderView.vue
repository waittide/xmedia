<script setup lang="ts">
import { computed, ref } from 'vue'
import {
  LuBookMarked,
  LuDownload,
  LuList,
  LuMoreVertical,
  LuRefreshCw,
  LuRepeat,
  LuSlidersHorizontal,
} from 'vue-icons-plus/lu'
import { ImmersiveMediaTemplate } from '@/components/domain'
import ThemePopover from '@/components/ThemePopover.vue'
import { MdIconButton, MdMenu, MdMenuItem, MdTextField } from '@/components/ui'
import BookSourceSwitchDialog from '@/domains/book/components/BookSourceSwitchDialog.vue'
import OnlineDownloadModal from '@/domains/book/components/OnlineDownloadModal.vue'
import ReaderTypographyPanel from '@/domains/book/components/ReaderTypographyPanel.vue'
import { useOnlineReader } from '@/domains/book/components/online-reader/useOnlineReader'
import { useBookI18n as useI18n } from '@/domains/book/i18n'
import ContinuousTextArticle from '@/domains/book/readers/ContinuousTextArticle.vue'

const { t } = useI18n()

const articleRef = ref<InstanceType<typeof ContinuousTextArticle> | null>(null)
const contentScrollRef = computed<HTMLElement | null>(
  () => articleRef.value?.scrollContainerRef ?? null,
)
const hudVisible = ref(true)

const {
  readerCtx,
  chapters,
  activeIndex,
  loadedChapters,
  loadingToc,
  loadingContent,
  appendingNext,
  appendError,
  errorMsg,
  savingShelf,
  activeOverlay,
  tocQuery,
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
  appendNextChapter,
  handleContentScroll,
  handleSaveToShelf,
  handleSelectAltSource,
  handleBackToCatalog,
} = useOnlineReader(contentScrollRef)

// Single mutually-exclusive MdSheet binding for TOC / Typography
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

const filteredToc = computed(() => {
  const q = tocQuery.value.trim().toLowerCase()
  const entries = chapters.value.map((chapter, pos) => ({ chapter, pos }))
  if (!q) return entries
  return entries.filter((entry) => entry.chapter.title.toLowerCase().includes(q))
})

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
    :auto-hide="true"
    :lock-hud="
      loadingToc ||
      loadingContent ||
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
    <!-- Top HUD Right Controls (4 controls + built-in Back = 5 total) -->
    <template #header-controls>
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

      <ThemePopover />

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
          v-if="readerCtx && !readerCtx.shelfId"
          :icon="LuBookMarked"
          :label="t('online.saveToLibrary')"
          :disabled="savingShelf"
          @select="handleSaveToShelf"
        />
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

    <!-- Unified Continuous Chapter Reading Stage -->
    <ContinuousTextArticle
      ref="articleRef"
      :chapters="loadedChapters"
      :active-pos="activeIndex"
      :total-chapters="chapters.length"
      :loading="loadingToc || loadingContent"
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

    <!-- Single Mutually-Exclusive Sheet (TOC / Typography) -->
    <template v-if="activeSheet === 'toc'" #sheet-subheader>
      <MdTextField
        v-model="tocQuery"
        type="search"
        size="sm"
        clearable
        :placeholder="t('reader.tocSearchPlaceholder')"
      />
    </template>

    <template #sheet>
      <div v-if="activeSheet === 'toc'" class="space-y-0.5">
        <p
          v-if="filteredToc.length === 0"
          class="text-body-small py-6 text-center text-on-surface-variant"
        >
          {{ t('reader.noToc') }}
        </p>
        <button
          v-for="entry in filteredToc"
          :key="`toc-${entry.pos}-${entry.chapter.index}`"
          type="button"
          :disabled="entry.chapter.is_volume || !entry.chapter.url"
          class="md-state flex w-full cursor-pointer items-center rounded-corner-md px-3 py-2 text-left transition-colors duration-150 ease-standard disabled:cursor-default"
          :class="
            entry.chapter.is_volume
              ? 'text-title-small mt-2 text-primary'
              : entry.pos === activeIndex
                ? 'text-title-small bg-secondary-container text-on-secondary-container'
                : 'text-body-medium text-on-surface-variant hover:bg-surface-container-high hover:text-on-surface'
          "
          @click="openChapter(entry.pos)"
        >
          <span class="truncate">{{ entry.chapter.title }}</span>
        </button>
      </div>

      <ReaderTypographyPanel v-else-if="activeSheet === 'typography'" />
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
