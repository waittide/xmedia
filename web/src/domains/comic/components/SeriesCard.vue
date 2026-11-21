<script setup lang="ts">
import { computed } from 'vue'
import { useRouter } from 'vue-router'
import { useComicI18n as useI18n } from '@/domains/comic/i18n'
import { LuBookOpen, LuHeart, LuPlay } from 'vue-icons-plus/lu'
import { withAuthUrl } from '@/domains/comic/api'
import type { SeriesSummaryDto } from '@/domains/comic/types'

const props = defineProps<{
  series: SeriesSummaryDto
  subtitleOverride?: string
}>()

const router = useRouter()
const { t } = useI18n()

const progressPercent = computed(() => {
  if (props.series.chapter_count <= 0) return 0
  return Math.min(
    100,
    Math.round((props.series.completed_chapters / props.series.chapter_count) * 100)
  )
})

function openDetail(): void {
  router.push('/comic/series/' + props.series.id)
}

function quickRead(ev: MouseEvent): void {
  ev.stopPropagation()
  if (props.series.continue_reading) {
    router.push(
      '/comic/read/' +
        props.series.continue_reading.chapter_id +
        '?page=' +
        props.series.continue_reading.current_page
    )
  } else {
    router.push('/comic/series/' + props.series.id)
  }
}
</script>

<template>
  <div
    role="link"
    tabindex="0"
    :aria-label="series.title"
    class="group flex cursor-pointer flex-col gap-2.5 rounded-2xl select-none focus-visible:outline-3 focus-visible:outline-offset-2 focus-visible:outline-[var(--md-sys-color-secondary)]"
    @click="openDetail"
    @keydown.enter.self="openDetail"
    @keydown.space.self.prevent="openDetail"
  >
    <div
      class="book-shadow relative aspect-[2/3] w-full overflow-hidden rounded-2xl bg-[var(--md-sys-color-surface-container)]"
    >
      <img
        v-if="series.cover_url"
        :src="withAuthUrl(series.cover_url)"
        :alt="series.title"
        loading="lazy"
        decoding="async"
        class="h-full w-full object-cover"
      />
      <div
        v-else
        class="flex h-full w-full flex-col items-center justify-center gap-2 p-4 text-center text-[var(--md-sys-color-on-surface-variant)]"
      >
        <LuBookOpen class="h-8 w-8 opacity-55" />
        <span class="line-clamp-2 text-xs font-medium">{{ series.title }}</span>
      </div>

      <!-- Top Badges -->
      <div class="absolute top-2 right-2 left-2 flex items-center justify-between gap-1">
        <span
          class="rounded-full bg-black/65 px-2 py-0.5 text-[10px] font-semibold text-white backdrop-blur-xs"
        >
          {{ t('home.chaptersCount', { count: series.chapter_count }) }}
        </span>
        <span
          v-if="series.is_favorite"
          class="flex h-6 w-6 items-center justify-center rounded-full bg-[var(--md-sys-color-error)] text-[var(--md-sys-color-on-error)] shadow-[var(--md-sys-elevation-1)]"
        >
          <LuHeart class="h-3 w-3 fill-current" />
        </span>
      </div>

      <!-- Hover Quick Resume Overlay -->
      <button
        v-if="series.continue_reading"
        type="button"
        :aria-label="t('home.continueReading')"
        class="md-state absolute right-2.5 bottom-3 z-10 flex h-9 w-9 items-center justify-center rounded-full bg-[var(--md-sys-color-primary)] text-[var(--md-sys-color-on-primary)] opacity-0 shadow-[var(--md-sys-elevation-2)] transition group-hover:opacity-100 focus-visible:opacity-100 hover:scale-105"
        @click="quickRead"
      >
        <LuPlay class="h-4 w-4 fill-current" />
      </button>

      <!-- Bottom Reading Progress Bar -->
      <div
        v-if="progressPercent > 0"
        class="absolute right-0 bottom-0 left-0 h-1 bg-black/40"
      >
        <div
          class="h-full bg-[var(--md-sys-color-primary)] transition-all"
          :style="{ width: progressPercent + '%' }"
        />
      </div>
    </div>

    <div class="min-w-0 px-0.5">
      <div
        class="line-clamp-1 text-sm font-semibold text-[var(--md-sys-color-on-surface)] transition group-hover:text-[var(--md-sys-color-primary)]"
      >
        {{ series.title }}
      </div>
      <div class="mt-0.5 line-clamp-1 text-xs text-[var(--md-sys-color-on-surface-variant)]">
        {{
          subtitleOverride ||
          [...series.writers, ...series.pencillers].join(' · ') ||
          (series.status === 'completed'
            ? t('series.statusCompleted')
            : series.status === 'hiatus'
              ? t('series.statusHiatus')
              : t('series.statusOngoing'))
        }}
      </div>
    </div>
  </div>
</template>
