<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { LuBookOpen, LuDownload, LuInfo } from 'vue-icons-plus/lu'
import { buildOnlineCoverUrl, type OnlineShelfBookDto } from '../onlineApi'

const props = defineProps<{
  book: OnlineShelfBookDto
}>()

const emit = defineEmits<{
  read: [book: OnlineShelfBookDto]
  download: [book: OnlineShelfBookDto]
  detail: [book: OnlineShelfBookDto]
}>()

const coverFailed = ref(false)

watch(
  () => props.book.cover_url,
  () => {
    coverFailed.value = false
  },
)

const coverSrc = computed(() =>
  !coverFailed.value && props.book.cover_url ? buildOnlineCoverUrl(props.book.cover_url) : '',
)

const progressPercent = computed(() => {
  if (props.book.total_chapters <= 0) return 0
  return Math.min(
    100,
    Math.max(0, Math.round(((props.book.current_chapter_index + 1) / props.book.total_chapters) * 100)),
  )
})

const chapterProgressText = computed(() => {
  const current = props.book.current_chapter_index + 1
  if (props.book.total_chapters > 0) {
    return `${current} / ${props.book.total_chapters}`
  }
  return `第 ${current} 章`
})
</script>

<template>
  <article class="group flex flex-col">
    <!-- 2:3 Portrait Cover -->
    <div
      role="button"
      tabindex="0"
      :aria-label="`阅读《${book.name}》`"
      class="book-shadow relative aspect-[2/3] w-full cursor-pointer overflow-hidden rounded-xl bg-[var(--md-sys-color-surface-container-low)] outline-none transition duration-200 group-hover:-translate-y-1 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--md-sys-color-primary)]"
      @click="emit('read', book)"
      @keydown.enter.prevent="emit('read', book)"
      @keydown.space.prevent="emit('read', book)"
    >
      <img
        v-if="coverSrc"
        :src="coverSrc"
        :alt="book.name"
        loading="lazy"
        decoding="async"
        class="h-full w-full object-cover transition duration-300 group-hover:scale-[1.03]"
        @error="coverFailed = true"
      />
      <div
        v-else
        class="flex h-full w-full flex-col justify-end bg-gradient-to-br from-[var(--md-sys-color-surface-container-high)] via-[var(--md-sys-color-surface-container)] to-[var(--md-sys-color-primary-container)]/35 p-3.5 text-left"
      >
        <p class="line-clamp-3 text-sm font-bold leading-snug text-[var(--md-sys-color-on-surface)]">
          {{ book.name }}
        </p>
        <p class="mt-1 truncate text-xs text-[var(--md-sys-color-on-surface-variant)]">
          {{ book.author || '佚名' }}
        </p>
      </div>

      <!-- Top-left compact source badge -->
      <span
        class="pointer-events-none absolute top-2 left-2 max-w-[calc(100%-3rem)] truncate rounded-md bg-black/55 px-1.5 py-0.5 text-[10px] font-medium text-white backdrop-blur-sm"
      >
        {{ book.source_name }}
      </span>

      <!-- Top-right detail icon button (always visible on touch, hover/focus on desktop) -->
      <button
        type="button"
        title="详情"
        aria-label="书籍详情"
        class="absolute top-2 right-2 flex h-7 w-7 cursor-pointer items-center justify-center rounded-full bg-black/55 text-white opacity-100 backdrop-blur-sm transition hover:bg-black/75 focus-visible:opacity-100 sm:opacity-0 sm:group-hover:opacity-100"
        @click.stop="emit('detail', book)"
      >
        <LuInfo class="h-3.5 w-3.5" />
      </button>

      <!-- Subtle bottom reading progress bar -->
      <div
        v-if="book.total_chapters > 0"
        class="absolute inset-x-0 bottom-0 h-1 bg-black/30"
      >
        <div
          class="h-full bg-[var(--md-sys-color-primary)] transition-all duration-300"
          :style="{ width: `${Math.max(2, progressPercent)}%` }"
        />
      </div>
    </div>

    <!-- Minimal info below cover -->
    <div class="mt-2.5 min-w-0 px-0.5">
      <button
        type="button"
        class="block w-full cursor-pointer truncate text-left text-sm font-semibold text-[var(--md-sys-color-on-surface)] transition-colors group-hover:text-[var(--md-sys-color-primary)]"
        :title="book.name"
        @click="emit('read', book)"
      >
        {{ book.name }}
      </button>
      <div class="mt-0.5 flex items-center justify-between gap-1.5 text-xs text-[var(--md-sys-color-on-surface-variant)]">
        <span class="truncate">{{ book.author || '佚名' }}</span>
        <span class="shrink-0 font-mono text-[11px] opacity-85">{{ chapterProgressText }}</span>
      </div>
    </div>

    <!-- Compact dual action buttons -->
    <div class="mt-2 grid grid-cols-2 gap-1.5">
      <button
        type="button"
        class="md-btn md-btn-sm md-btn-tonal md-state !h-7 !px-2 !text-[11px]"
        @click="emit('read', book)"
      >
        <LuBookOpen class="h-3 w-3 shrink-0" />
        <span>阅读</span>
      </button>
      <button
        type="button"
        class="md-btn md-btn-sm md-btn-outlined md-state !h-7 !px-2 !text-[11px]"
        @click="emit('download', book)"
      >
        <LuDownload class="h-3 w-3 shrink-0" />
        <span>下载</span>
      </button>
    </div>
  </article>
</template>
