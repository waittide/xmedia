<script setup lang="ts">
import { onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { LuBookOpen, LuHeart, LuSearch, LuTrash2 } from 'vue-icons-plus/lu'
import { api } from '@/api/client'
import { useAuthStore } from '@/stores/auth'
import { useEventsStore } from '@/stores/events'
import type { BookSummaryDto } from '@/types/api'

const { t } = useI18n()
const router = useRouter()
const auth = useAuthStore()
const events = useEventsStore()

const books = ref<BookSummaryDto[]>([])
const keyword = ref('')
const formatFilter = ref('')
const favOnly = ref(false)
const loading = ref(false)

async function fetchBooks(): Promise<void> {
  loading.value = true
  try {
    const params: Record<string, string> = { page_size: '100' }
    if (keyword.value.trim()) params.keyword = keyword.value.trim()
    if (formatFilter.value) params.format = formatFilter.value
    if (favOnly.value) params.favorites_only = 'true'
    const res = await api.listBooks(params)
    books.value = res.items
  } finally {
    loading.value = false
  }
}

onMounted(fetchBooks)
watch(() => events.changeTick.book, fetchBooks)

async function toggleFav(b: BookSummaryDto, ev: Event): Promise<void> {
  ev.stopPropagation()
  const next = !b.is_favorite
  await api.setBookFavorite(b.id, next)
  b.is_favorite = next
}

async function removeBook(b: BookSummaryDto, ev: Event): Promise<void> {
  ev.stopPropagation()
  await api.deleteBook(b.id)
  await fetchBooks()
}
</script>

<template>
  <div class="mx-auto max-w-7xl space-y-6 p-6">
    <!-- Filter Bar -->
    <div class="flex flex-wrap items-center justify-between gap-3">
      <div class="relative flex-1 min-w-60 max-w-md">
        <LuSearch class="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-[var(--ctp-subtext0)]" />
        <input
          v-model="keyword"
          :placeholder="t('common.search')"
          class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] pl-9 pr-4 py-2 text-sm outline-none focus:border-[var(--ctp-accent)]"
          @keyup.enter="fetchBooks"
        />
      </div>

      <div class="flex items-center gap-2 text-xs">
        <select
          v-model="formatFilter"
          class="rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] px-3 py-2"
          @change="fetchBooks"
        >
          <option value="">{{ t('common.all') }} (EPUB / PDF / TXT)</option>
          <option value="epub">EPUB</option>
          <option value="pdf">PDF</option>
          <option value="txt">TXT</option>
        </select>

        <button
          type="button"
          class="inline-flex items-center gap-1.5 rounded-xl border px-3 py-2 transition"
          :class="
            favOnly
              ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)] font-semibold'
              : 'border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] text-[var(--ctp-subtext1)]'
          "
          @click="favOnly = !favOnly; fetchBooks()"
        >
          <LuHeart class="h-3.5 w-3.5" /> {{ t('common.favorites') }}
        </button>
      </div>
    </div>

    <div v-if="loading" class="py-16 text-center text-sm text-[var(--ctp-subtext0)]">
      {{ t('common.loading') }}
    </div>
    <div v-else-if="books.length === 0" class="py-20 text-center text-sm text-[var(--ctp-subtext0)]">
      {{ t('common.noItems') }}
    </div>

    <div v-else class="grid grid-cols-2 gap-5 sm:grid-cols-3 md:grid-cols-5 lg:grid-cols-6">
      <div
        v-for="b in books"
        :key="b.id"
        class="group cursor-pointer space-y-2"
        @click="router.push(`/books/read/${b.id}`)"
      >
        <div class="relative aspect-[2/3] overflow-hidden rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] media-shadow">
          <img
            v-if="b.has_cover"
            :src="api.bookCoverUrl(b.id)"
            alt=""
            class="h-full w-full object-cover transition group-hover:scale-105"
          />
          <div v-else class="flex h-full w-full items-center justify-center">
            <LuBookOpen class="h-10 w-10 text-[var(--ctp-subtext0)]" />
          </div>

          <span class="absolute left-2 top-2 rounded bg-black/70 px-1.5 py-0.5 text-[10px] font-bold uppercase text-white">
            {{ b.format }}
          </span>

          <div class="absolute right-2 top-2 flex gap-1 opacity-0 transition group-hover:opacity-100">
            <button
              type="button"
              class="rounded-full bg-black/70 p-1.5 text-white hover:text-[var(--ctp-red)]"
              @click="toggleFav(b, $event)"
            >
              <LuHeart class="h-3.5 w-3.5" :class="b.is_favorite ? 'fill-current text-[var(--ctp-red)]' : ''" />
            </button>
            <button
              v-if="auth.isAdmin && b.is_managed"
              type="button"
              class="rounded-full bg-black/70 p-1.5 text-white hover:text-[var(--ctp-red)]"
              @click="removeBook(b, $event)"
            >
              <LuTrash2 class="h-3.5 w-3.5" />
            </button>
          </div>

          <div
            v-if="b.progress && b.progress.progress_percent > 0"
            class="absolute inset-x-0 bottom-0 h-1 bg-black/50"
          >
            <div
              class="h-full bg-[var(--ctp-accent)]"
              :style="{ width: `${Math.min(100, b.progress.progress_percent)}%` }"
            />
          </div>
        </div>

        <div class="truncate text-xs font-semibold">{{ b.metadata.title }}</div>
        <div class="truncate text-[11px] text-[var(--ctp-subtext0)]">
          {{ b.metadata.authors.join(', ') || 'Unknown Author' }}
        </div>
      </div>
    </div>
  </div>
</template>
