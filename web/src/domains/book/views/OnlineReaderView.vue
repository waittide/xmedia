<script setup lang="ts">
import { ref } from 'vue'
import { LuChevronLeft, LuChevronRight, LuList, LuLoader2 } from 'vue-icons-plus/lu'
import OnlineReaderDrawers from '../components/online-reader/OnlineReaderDrawers.vue'
import OnlineReaderHeader from '../components/online-reader/OnlineReaderHeader.vue'
import { useOnlineReader } from '../components/online-reader/useOnlineReader'

const contentScrollRef = ref<HTMLElement | null>(null)

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
  tocOpen,
  tocQuery,
  typographyOpen,
  switchSourceOpen,
  searchingAlts,
  altCandidates,
  switchingKey,
  downloadOpen,
  maxWidthPx,
  fontSize,
  lineHeight,
  fontFamily,
  fontFamilyStyle,
  currentChapter,
  chapterTitle,
  contentCached,
  prevReadableIndex,
  nextAppendableIndex,
  downloadTarget,
  loadTocAndOpen,
  openChapter,
  appendNextChapter,
  handleContentScroll,
  handleSaveToShelf,
  openSwitchSource,
  handleSelectAltSource,
  handleBackToCatalog,
} = useOnlineReader(contentScrollRef)
</script>

<template>
  <div class="flex h-screen flex-col bg-[var(--md-sys-color-surface)] text-[var(--md-sys-color-on-surface)]">
    <OnlineReaderHeader
      :reader-ctx="readerCtx"
      :chapter-title="chapterTitle"
      :active-index="activeIndex"
      :chapters-length="chapters.length"
      :content-cached="contentCached"
      :font-size="fontSize"
      :typography-open="typographyOpen"
      :saving-shelf="savingShelf"
      :loading-content="loadingContent"
      @back="handleBackToCatalog"
      @update:font-size="fontSize = $event"
      @toggle-typography="typographyOpen = !typographyOpen"
      @save-shelf="handleSaveToShelf"
      @refresh="openChapter(activeIndex, true)"
      @open-switch-source="openSwitchSource"
      @open-download="downloadOpen = true"
      @toggle-toc="tocOpen = !tocOpen"
    />

    <!-- Main Reading Area -->
    <div
      ref="contentScrollRef"
      class="flex-1 overflow-y-auto px-4 py-8 sm:px-8"
      @scroll.passive="handleContentScroll"
    >
      <div class="mx-auto" :style="{ maxWidth: `${maxWidthPx}px` }">
        <div
          v-if="loadingToc || loadingContent"
          class="flex flex-col items-center justify-center py-24 text-xs text-[var(--md-sys-color-on-surface-variant)]"
        >
          <LuLoader2 class="mb-3 h-6 w-6 animate-spin text-[var(--md-sys-color-primary)]" />
          <span>{{ loadingToc ? '正在获取章节目录…' : '正在拉取章节正文…' }}</span>
        </div>

        <div
          v-else-if="errorMsg"
          class="rounded-2xl bg-[var(--md-sys-color-error-container)] p-5 text-center text-xs text-[var(--md-sys-color-on-error-container)]"
        >
          <p class="font-semibold">加载失败</p>
          <p class="mt-1">{{ errorMsg }}</p>
          <div class="mt-4 flex justify-center gap-2">
            <button
              type="button"
              class="md-btn md-btn-sm md-btn-filled md-state"
              @click="currentChapter ? openChapter(activeIndex, true) : loadTocAndOpen(activeIndex)"
            >
              重试
            </button>
            <button
              type="button"
              class="md-btn md-btn-sm md-btn-tonal md-state"
              @click="openSwitchSource"
            >
              尝试换源
            </button>
          </div>
        </div>

        <article v-else class="pb-16" :style="{ fontFamily: fontFamilyStyle }">
          <section
            v-for="(item, itemIdx) in loadedChapters"
            :key="`chapter-${item.pos}-${item.chapterIndex}`"
            :data-chapter-pos="item.pos"
            :class="itemIdx > 0 ? 'mt-14 border-t border-[var(--md-sys-color-outline-variant)]/45 pt-12' : ''"
          >
            <h1
              class="mb-8 text-center font-bold tracking-tight"
              :style="{ fontSize: `${Math.round(fontSize * 1.3)}px` }"
            >
              {{ item.title }}
            </h1>

            <div
              class="space-y-4 select-text"
              :style="{
                fontSize: `${fontSize}px`,
                lineHeight: String(lineHeight),
              }"
            >
              <p
                v-for="(para, idx) in item.paragraphs"
                :key="`p-${item.pos}-${idx}`"
                class="indent-[2em] break-words"
              >
                {{ para }}
              </p>
            </div>

            <p
              v-if="item.paragraphs.length === 0"
              class="py-12 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
            >
              本章正文为空，可点击右上角刷新重试或切换书源。
            </p>
          </section>

          <!-- Continuous Scroll Footer Status -->
          <div class="mt-10 flex flex-col items-center gap-3 text-xs text-[var(--md-sys-color-on-surface-variant)]">
            <div v-if="appendingNext" class="flex items-center gap-2 py-2">
              <LuLoader2 class="h-4 w-4 animate-spin text-[var(--md-sys-color-primary)]" />
              <span>正在自动加载下一章…</span>
            </div>
            <div
              v-else-if="appendError"
              class="flex flex-wrap items-center justify-center gap-2 rounded-2xl bg-[var(--md-sys-color-error-container)] px-4 py-2.5 text-[var(--md-sys-color-on-error-container)]"
            >
              <span>自动加载下一章失败：{{ appendError }}</span>
              <button
                type="button"
                class="md-btn md-btn-sm md-btn-filled md-state"
                @click="appendNextChapter()"
              >
                重试
              </button>
            </div>
            <span
              v-else-if="nextAppendableIndex === null && loadedChapters.length > 0"
              class="py-1 opacity-75"
            >
              已读至最后一章
            </span>
          </div>

          <!-- Chapter Navigation Footer -->
          <div
            class="mt-6 flex items-center justify-between gap-3 border-t border-[var(--md-sys-color-outline-variant)]/45 pt-6"
          >
            <button
              type="button"
              :disabled="prevReadableIndex === null"
              class="md-btn md-btn-tonal md-state disabled:opacity-40"
              @click="prevReadableIndex !== null && openChapter(prevReadableIndex)"
            >
              <LuChevronLeft class="h-4 w-4" />
              <span>上一章</span>
            </button>

            <button
              type="button"
              class="md-btn md-btn-text md-state"
              @click="tocOpen = true"
            >
              <LuList class="h-4 w-4" />
              <span>目录</span>
            </button>

            <button
              type="button"
              :disabled="nextAppendableIndex === null || appendingNext"
              class="md-btn md-btn-filled md-state disabled:opacity-40"
              @click="nextAppendableIndex !== null && openChapter(nextAppendableIndex)"
            >
              <span>下一章</span>
              <LuChevronRight class="h-4 w-4" />
            </button>
          </div>
        </article>
      </div>
    </div>

    <OnlineReaderDrawers
      :reader-ctx="readerCtx"
      :chapters="chapters"
      :active-index="activeIndex"
      :typography-open="typographyOpen"
      :font-family="fontFamily"
      :font-size="fontSize"
      :line-height="lineHeight"
      :toc-open="tocOpen"
      :toc-query="tocQuery"
      :switch-source-open="switchSourceOpen"
      :searching-alts="searchingAlts"
      :alt-candidates="altCandidates"
      :switching-key="switchingKey"
      :download-open="downloadOpen"
      :download-target="downloadTarget"
      @close-typography="typographyOpen = false"
      @update:font-family="fontFamily = $event"
      @update:font-size="fontSize = $event"
      @update:line-height="lineHeight = $event"
      @close-toc="tocOpen = false"
      @update:toc-query="tocQuery = $event"
      @select-chapter="openChapter($event)"
      @close-switch-source="switchSourceOpen = false"
      @select-alt-source="handleSelectAltSource"
      @close-download="downloadOpen = false"
    />
  </div>
</template>
