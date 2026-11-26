<script setup lang="ts">
import { computed, ref } from 'vue'
import {
  LuCheckCircle2,
  LuChevronLeft,
  LuChevronRight,
  LuList,
  LuRefreshCw,
  LuRepeat,
} from 'vue-icons-plus/lu'
import { MediaErrorState } from '@/components/domain'
import { MdButton, MdCircularProgress, MdIconButton } from '@/components/ui'
import { useBookI18n as useI18n } from '@/domains/book/i18n'
import { usePreferencesStore } from '@/stores/preferences'
import { useThemeStore } from '@/stores/theme'
import { uploadedFonts } from '@/theme/customFonts'
import { resolveFontStack } from '@/utils/format'
import type { ContinuousChapterSection } from './useContinuousChapterReader'

const props = withDefaults(
  defineProps<{
    chapters: readonly ContinuousChapterSection[]
    activePos: number
    totalChapters: number
    loading?: boolean
    loadingLabel?: string
    errorMsg?: string | null
    appendingNext?: boolean
    appendError?: string | null
    hasPrev?: boolean
    hasNext?: boolean
    showChapterRefetch?: boolean
    showSwitchSourceOnError?: boolean
    showTocButton?: boolean
    cachedSummary?: string | null
  }>(),
  {
    loading: false,
    loadingLabel: '',
    errorMsg: null,
    appendingNext: false,
    appendError: null,
    hasPrev: false,
    hasNext: false,
    showChapterRefetch: false,
    showSwitchSourceOnError: false,
    showTocButton: false,
    cachedSummary: null,
  },
)

const emit = defineEmits<{
  scroll: []
  retry: []
  'retry-append': []
  'prev-chapter': []
  'next-chapter': []
  'refetch-chapter': [pos: number]
  'open-toc': []
  'switch-source': []
}>()

const { t } = useI18n()
const preferencesStore = usePreferencesStore()
const themeStore = useThemeStore()

const scrollContainerRef = ref<HTMLElement | null>(null)

const fontFamilyStyle = computed(() => {
  void uploadedFonts.value.length
  return resolveFontStack(preferencesStore.reader.font_family, themeStore.uiFont)
})

const fontSizePx = computed(() => preferencesStore.reader.font_size || 18)
const lineHeightVal = computed(() => preferencesStore.reader.line_height || 1.85)
const maxWidthPx = computed(() => Math.max(520, 920 - (preferencesStore.reader.margin_px || 48) * 2))

const articleStyle = computed(() => ({
  maxWidth: `${maxWidthPx.value}px`,
  fontFamily: fontFamilyStyle.value,
}))

const paragraphContainerStyle = computed(() => ({
  fontSize: `${fontSizePx.value}px`,
  lineHeight: String(lineHeightVal.value),
}))

const chapterTitleStyle = computed(() => ({
  fontSize: `${Math.round(fontSizePx.value * 1.3)}px`,
}))

defineExpose({
  scrollContainerRef,
})
</script>

<template>
  <div
    ref="scrollContainerRef"
    class="h-full w-full overflow-y-auto bg-surface px-4 pt-20 pb-16 transition-colors duration-150 ease-standard sm:px-8"
    @scroll.passive="emit('scroll')"
  >
    <div class="mx-auto w-full" :style="articleStyle">
      <!-- Primary Loading -->
      <div
        v-if="loading"
        class="flex flex-col items-center justify-center gap-3 py-24 text-on-surface-variant"
      >
        <MdCircularProgress indeterminate :size="32" :stroke-width="3" />
        <span class="text-body-small">{{ loadingLabel || t('reader.loadingBook') }}</span>
      </div>

      <!-- Primary Error -->
      <MediaErrorState
        v-else-if="errorMsg"
        :title="t('reader.loadFailedTitle')"
        :message="errorMsg"
        :retry-label="t('reader.retry')"
        class="my-12"
        @retry="emit('retry')"
      >
        <template v-if="showSwitchSourceOnError" #action>
          <MdButton variant="tonal" :icon="LuRepeat" @click="emit('switch-source')">
            {{ t('reader.trySwitchSource') }}
          </MdButton>
        </template>
      </MediaErrorState>

      <!-- Continuous Chapter Sections -->
      <article v-else-if="chapters.length > 0" class="pb-12">
        <section
          v-for="(item, itemIdx) in chapters"
          :key="`ch-${item.pos}-${item.chapterIndex}`"
          :data-chapter-pos="item.pos"
          :class="itemIdx > 0 ? 'mt-16 pt-4' : ''"
        >
          <header class="mb-8 flex items-start justify-between gap-3 rounded-corner-lg bg-on-surface/[0.04] px-4 py-3.5">
            <div class="min-w-0 flex-1">
              <div class="flex flex-wrap items-center gap-2 text-label-medium text-primary">
                <span
                  v-if="item.volumeTitle"
                  class="rounded-corner-full bg-primary-container px-2.5 py-0.5 text-on-primary-container"
                >
                  {{ item.volumeTitle }}
                </span>
                <span v-if="totalChapters > 0" class="tabular-nums text-on-surface-variant">
                  {{
                    t('reader.chapterIndicator', {
                      current: item.pos + 1,
                      total: totalChapters,
                    })
                  }}
                </span>
                <span
                  v-if="cachedSummary"
                  class="inline-flex items-center gap-1 text-on-surface-variant"
                >
                  <LuCheckCircle2 class="h-3.5 w-3.5 text-primary" aria-hidden="true" />
                  <span>{{ cachedSummary }}</span>
                </span>
              </div>

              <h1
                class="mt-2 leading-snug tracking-tight text-on-surface"
                :style="chapterTitleStyle"
              >
                {{ item.title }}
              </h1>
            </div>

            <MdIconButton
              v-if="showChapterRefetch"
              variant="standard"
              size="sm"
              :icon="LuRefreshCw"
              :aria-label="t('reader.refetchChapter')"
              :tooltip="t('reader.refetchChapter')"
              @click="emit('refetch-chapter', item.pos)"
            />
          </header>

          <div
            v-if="item.paragraphs.length > 0"
            class="space-y-4 text-on-surface select-text"
            :style="paragraphContainerStyle"
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
            v-else
            class="text-body-small py-12 text-center text-on-surface-variant"
          >
            {{ t('reader.emptyChapterContent') }}
          </p>
        </section>

        <!-- Continuous Scroll Auto-Append Status -->
        <div class="mt-10 flex flex-col items-center gap-3 text-body-small text-on-surface-variant">
          <div v-if="appendingNext" class="flex items-center gap-2 py-2">
            <MdCircularProgress indeterminate :size="18" :stroke-width="2.5" />
            <span>{{ t('reader.loadingNextChapter') }}</span>
          </div>

          <div
            v-else-if="appendError"
            role="alert"
            class="flex flex-wrap items-center justify-center gap-2 rounded-corner-md bg-error-container px-4 py-2.5 text-on-error-container"
          >
            <span>{{ t('reader.appendErrorPrefix', { error: appendError }) }}</span>
            <MdButton variant="filled" @click="emit('retry-append')">
              {{ t('reader.retry') }}
            </MdButton>
          </div>

          <span v-else-if="!hasNext" class="py-1 opacity-75">
            {{ t('reader.endOfBook') }}
          </span>
        </div>

        <!-- Chapter Navigation Footer -->
        <div
          class="mt-6 flex items-center justify-between gap-3 rounded-corner-lg bg-on-surface/[0.04] px-4 py-3"
        >
          <MdButton
            variant="tonal"
            :icon="LuChevronLeft"
            :disabled="!hasPrev"
            @click="emit('prev-chapter')"
          >
            {{ t('reader.prevChapter') }}
          </MdButton>

          <MdButton
            v-if="showTocButton"
            variant="text"
            :icon="LuList"
            @click="emit('open-toc')"
          >
            {{ t('reader.toc') }}
          </MdButton>
          <span
            v-else-if="totalChapters > 0"
            class="text-label-medium tabular-nums text-on-surface-variant"
          >
            {{ activePos + 1 }} / {{ totalChapters }}
          </span>

          <MdButton
            variant="filled"
            :disabled="!hasNext || appendingNext"
            @click="emit('next-chapter')"
          >
            <span>{{ t('reader.nextChapter') }}</span>
            <template #trailing>
              <LuChevronRight class="h-4 w-4" aria-hidden="true" />
            </template>
          </MdButton>
        </div>
      </article>
    </div>
  </div>
</template>
