<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { LuBookOpen, LuDownload, LuTrash2 } from 'vue-icons-plus/lu'
import MdSideSheet from '@/components/ui/MdSideSheet.vue'
import { buildOnlineCoverUrl, type OnlineShelfBookDto } from '../onlineApi'

const props = defineProps<{
  open: boolean
  book: OnlineShelfBookDto | null
}>()

const emit = defineEmits<{
  close: []
  read: [book: OnlineShelfBookDto]
  download: [book: OnlineShelfBookDto]
  remove: [book: OnlineShelfBookDto]
}>()

const coverFailed = ref(false)

watch(
  () => [props.book?.id, props.book?.cover_url],
  () => {
    coverFailed.value = false
  },
)

const coverSrc = computed(() =>
  !coverFailed.value && props.book?.cover_url ? buildOnlineCoverUrl(props.book.cover_url) : '',
)

const kindTags = computed(() => {
  const raw = props.book?.kind
  if (!raw) return []
  return raw
    .split(/[,，|/]/)
    .map((s) => s.trim())
    .filter(Boolean)
})

const progressPercent = computed(() => {
  if (!props.book || props.book.total_chapters <= 0) return 0
  return Math.min(
    100,
    Math.max(
      0,
      Math.round(((props.book.current_chapter_index + 1) / props.book.total_chapters) * 100),
    ),
  )
})

const chapterCountLabel = computed(() => {
  if (!props.book) return ''
  const current = props.book.current_chapter_index + 1
  if (props.book.total_chapters > 0) {
    return `${current} / ${props.book.total_chapters} 章`
  }
  return `第 ${current} 章`
})
</script>

<template>
  <MdSideSheet
    :open="open && book !== null"
    :title="book?.name || ''"
    :subtitle="book?.author || '佚名'"
    width-class="w-full max-w-md"
    @close="emit('close')"
  >
    <div v-if="book" class="space-y-5">
      <!-- Top Cover + Metadata Summary -->
      <div class="flex gap-4">
        <div
          class="book-shadow aspect-[2/3] w-28 shrink-0 overflow-hidden rounded-xl bg-[var(--md-sys-color-surface-container-high)] sm:w-32"
        >
          <img
            v-if="coverSrc"
            :src="coverSrc"
            :alt="book.name"
            class="h-full w-full object-cover"
            @error="coverFailed = true"
          />
          <div
            v-else
            class="flex h-full w-full flex-col justify-end bg-gradient-to-br from-[var(--md-sys-color-surface-container-high)] via-[var(--md-sys-color-surface-container)] to-[var(--md-sys-color-primary-container)]/35 p-3 text-left"
          >
            <p class="line-clamp-3 text-xs font-bold leading-snug text-[var(--md-sys-color-on-surface)]">
              {{ book.name }}
            </p>
            <p class="mt-1 truncate text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
              {{ book.author || '佚名' }}
            </p>
          </div>
        </div>

        <div class="flex min-w-0 flex-1 flex-col justify-between py-0.5">
          <div class="space-y-2.5">
            <!-- Source Badge -->
            <div class="flex flex-wrap items-center gap-1.5">
              <span
                class="inline-flex items-center rounded-full bg-[var(--md-sys-color-primary-container)] px-2.5 py-0.5 text-[11px] font-medium text-[var(--md-sys-color-on-primary-container)]"
              >
                {{ book.source_name }}
              </span>
            </div>

            <!-- Category Tags -->
            <div v-if="kindTags.length > 0" class="flex flex-wrap gap-1.5">
              <span
                v-for="(tag, idx) in kindTags"
                :key="`${tag}-${idx}`"
                class="rounded-md bg-[var(--md-sys-color-surface-container-high)] px-2 py-0.5 text-[11px] text-[var(--md-sys-color-on-surface-variant)]"
              >
                {{ tag }}
              </span>
            </div>
          </div>

          <!-- Reading Progress Summary -->
          <div class="space-y-1.5 pt-2">
            <div class="flex items-baseline justify-between gap-2 text-xs">
              <span class="font-medium text-[var(--md-sys-color-on-surface-variant)]">阅读进度</span>
              <span class="font-mono font-semibold text-[var(--md-sys-color-on-surface)]">
                {{ chapterCountLabel }}
                <template v-if="book.total_chapters > 0">
                  · {{ progressPercent }}%
                </template>
              </span>
            </div>
            <div
              v-if="book.total_chapters > 0"
              class="h-1.5 overflow-hidden rounded-full bg-[var(--md-sys-color-surface-container-highest)]"
            >
              <div
                class="h-full rounded-full bg-[var(--md-sys-color-primary)] transition-all duration-300"
                :style="{ width: `${Math.max(2, progressPercent)}%` }"
              />
            </div>
          </div>
        </div>
      </div>

      <!-- Current & Latest Chapter Info -->
      <div class="md-card-filled space-y-2 p-3.5 text-xs">
        <div class="flex items-start justify-between gap-3">
          <span class="shrink-0 text-[var(--md-sys-color-on-surface-variant)]">当前章节</span>
          <span class="min-w-0 truncate text-right font-medium text-[var(--md-sys-color-on-surface)]">
            {{ book.current_chapter_title || `第 ${book.current_chapter_index + 1} 章` }}
          </span>
        </div>
        <div v-if="book.last_chapter" class="flex items-start justify-between gap-3">
          <span class="shrink-0 text-[var(--md-sys-color-on-surface-variant)]">最新章节</span>
          <span class="min-w-0 truncate text-right text-[var(--md-sys-color-on-surface)]">
            {{ book.last_chapter }}
          </span>
        </div>
      </div>

      <!-- Full Intro -->
      <div class="space-y-2">
        <h4 class="text-xs font-semibold tracking-wide text-[var(--md-sys-color-on-surface-variant)]">
          简介
        </h4>
        <p
          v-if="book.intro && book.intro.trim()"
          class="whitespace-pre-line text-sm leading-relaxed text-[var(--md-sys-color-on-surface)]"
        >
          {{ book.intro.trim() }}
        </p>
        <p v-else class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
          暂无简介
        </p>
      </div>
    </div>

    <template v-if="book" #footer>
      <div class="flex w-full items-center gap-2">
        <button
          type="button"
          class="md-btn md-btn-sm md-btn-text md-state mr-auto !text-[var(--md-sys-color-error)]"
          @click="emit('remove', book)"
        >
          <LuTrash2 class="h-3.5 w-3.5" />
          <span>移出</span>
        </button>

        <button
          type="button"
          class="md-btn md-btn-sm md-btn-tonal md-state"
          @click="emit('download', book)"
        >
          <LuDownload class="h-3.5 w-3.5" />
          <span>下载</span>
        </button>

        <button
          type="button"
          class="md-btn md-btn-sm md-btn-filled md-state"
          @click="emit('read', book)"
        >
          <LuBookOpen class="h-3.5 w-3.5" />
          <span>阅读</span>
        </button>
      </div>
    </template>
  </MdSideSheet>
</template>
