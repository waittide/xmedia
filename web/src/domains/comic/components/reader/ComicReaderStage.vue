<script setup lang="ts">
import { ref } from 'vue'
import { useI18n as useGlobalI18n } from 'vue-i18n'
import { LuAlertTriangle, LuChevronRight, LuRotateCw } from 'vue-icons-plus/lu'
import { MdButton, MdCircularProgress } from '@/components/ui'
import { useComicI18n } from '@/domains/comic/i18n'
import type { ReaderMode } from '@/domains/comic/types'

defineProps<{
  loading: boolean
  loadError: string | null
  hasContent: boolean
  backLabel: string
  readerMode: ReaderMode
  chapterKey: string
  totalPages: number
  webtoonWidth: number
  zoomPercent: number
  isRtl: boolean
  currentSpread: readonly number[]
  eyeCareClass: string
  brokenPages: Readonly<Record<number, boolean>>
  hasNextChapter: boolean
  nextChapterToastLabel: string
  getDisplayPageSrc: (pageNum: number) => string
  getPageImageStyle: (pageNum: number) => Record<string, string>
}>()

const emit = defineEmits<{
  retry: []
  back: []
  webtoonScroll: []
  imageLoaded: [pageNum: number, ev: Event]
  imageError: [pageNum: number]
  retryPage: [pageNum: number]
  nextChapter: []
}>()

const { t } = useComicI18n()
const { t: globalT } = useGlobalI18n()
const webtoonContainerRef = ref<HTMLDivElement | null>(null)

defineExpose({
  getWebtoonContainer: (): HTMLDivElement | null => webtoonContainerRef.value,
})
</script>

<template>
  <div class="flex h-full w-full flex-col overflow-hidden">
    <div
      v-if="loading"
      class="text-body-small flex flex-1 flex-col items-center justify-center gap-3 px-6 pt-14 text-center text-on-surface-variant"
    >
      <MdCircularProgress indeterminate :size="36" :stroke-width="3" />
      <span>{{ t('online.loadingPages') }}</span>
    </div>

    <div
      v-else-if="loadError || !hasContent"
      class="flex flex-1 flex-col items-center justify-center gap-4 px-6 pt-14 text-center"
      data-immersive-ignore
    >
      <p role="alert" class="text-body-medium max-w-md text-error">
        {{ loadError || t('online.emptyPages') }}
      </p>
      <div class="flex flex-wrap items-center justify-center gap-2">
        <MdButton variant="tonal" @click="emit('retry')">
          {{ globalT('common.refresh') }}
        </MdButton>
        <MdButton variant="outlined" @click="emit('back')">
          {{ backLabel }}
        </MdButton>
        <slot name="error-actions" />
      </div>
    </div>

    <div
      v-else-if="readerMode === 'webtoon'"
      ref="webtoonContainerRef"
      class="h-full w-full overflow-y-auto motion-safe:scroll-smooth"
      @scroll.passive="emit('webtoonScroll')"
    >
      <div
        class="mx-auto flex flex-col items-center"
        :style="{
          width: '100%',
          maxWidth: `${Math.round((webtoonWidth * zoomPercent) / 100)}px`,
        }"
      >
        <div
          v-for="p in totalPages"
          :key="`${chapterKey}-wt-${p}`"
          :data-page="p"
          class="relative w-full"
        >
          <img
            v-if="getDisplayPageSrc(p)"
            :src="getDisplayPageSrc(p)"
            :alt="`Page ${p}`"
            :loading="p <= 4 ? 'eager' : 'lazy'"
            class="m-0 block min-h-[40vh] w-full bg-surface-container-low/40 p-0"
            :class="eyeCareClass"
            @load="emit('imageLoaded', p, $event)"
            @error="emit('imageError', p)"
          />
          <div
            v-if="!getDisplayPageSrc(p) || brokenPages[p]"
            data-immersive-ignore
            class="text-body-small flex min-h-40 w-full flex-col items-center justify-center gap-2 bg-surface-container-low/95 p-6 text-center text-on-surface-variant"
            :class="getDisplayPageSrc(p) ? 'absolute inset-0' : ''"
            @click.stop
          >
            <LuAlertTriangle class="h-6 w-6 text-error" aria-hidden="true" />
            <span>{{ t('online.imageLoadError', { page: p }) }}</span>
            <MdButton
              v-if="getDisplayPageSrc(p)"
              variant="tonal"
              :icon="LuRotateCw"
              @click="emit('retryPage', p)"
            >
              {{ t('online.retryImage') }}
            </MdButton>
          </div>
        </div>

        <div
          data-immersive-ignore
          class="my-12 flex w-full flex-col items-center gap-3 px-6 text-center"
          @click.stop
        >
          <MdButton
            v-if="hasNextChapter"
            variant="filled"
            :aria-label="t('reader.nextChapterAria')"
            @click="emit('nextChapter')"
          >
            <span>
              {{ t('reader.nextChapter')
              }}<template v-if="nextChapterToastLabel">: {{ nextChapterToastLabel }}</template>
            </span>
            <template #trailing>
              <LuChevronRight class="h-4 w-4" aria-hidden="true" />
            </template>
          </MdButton>
          <p v-else class="text-body-medium text-on-surface-variant">
            {{ t('reader.reachedSeriesEnd') }}
          </p>
        </div>
      </div>
    </div>

    <div v-else class="flex h-full w-full overflow-auto">
      <div
        class="m-auto flex items-center justify-center gap-0"
        :class="isRtl ? 'flex-row-reverse' : 'flex-row'"
      >
        <div
          v-for="p in currentSpread"
          :key="`${chapterKey}-sp-${p}`"
          class="relative flex items-center justify-center"
        >
          <img
            v-if="getDisplayPageSrc(p)"
            :src="getDisplayPageSrc(p)"
            :alt="`Page ${p}`"
            :style="getPageImageStyle(p)"
            class="m-0 block object-contain p-0 select-none"
            :class="eyeCareClass"
            @load="emit('imageLoaded', p, $event)"
            @error="emit('imageError', p)"
          />
          <div
            v-if="!getDisplayPageSrc(p) || brokenPages[p]"
            data-immersive-ignore
            class="text-body-small flex min-h-60 min-w-52 flex-col items-center justify-center gap-2 rounded-corner-md bg-surface-container-low/95 p-6 text-center text-on-surface-variant"
            :class="getDisplayPageSrc(p) ? 'absolute inset-0' : ''"
            @click.stop
          >
            <LuAlertTriangle class="h-6 w-6 text-error" aria-hidden="true" />
            <span>{{ t('online.imageLoadError', { page: p }) }}</span>
            <MdButton
              v-if="getDisplayPageSrc(p)"
              variant="tonal"
              :icon="LuRotateCw"
              @click="emit('retryPage', p)"
            >
              {{ t('online.retryImage') }}
            </MdButton>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
