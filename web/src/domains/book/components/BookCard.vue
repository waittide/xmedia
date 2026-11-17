<script setup lang="ts">
import { useBookI18n as useI18n } from '@/domains/book/i18n'
import { LuHeart } from 'vue-icons-plus/lu'
import { api } from '@/domains/book/api'
import type { BookSummaryDto } from '@/domains/book/types'

defineProps<{
  book: BookSummaryDto
  showProgressText?: boolean
}>()

const { t } = useI18n()
</script>

<template>
  <RouterLink
    :to="`/book/items/${book.id}`"
    class="group flex flex-col outline-none"
  >
    <div class="book-shadow relative aspect-[2/3] w-full overflow-hidden rounded-xl bg-[var(--ctp-mantle)] transition duration-200 group-hover:-translate-y-1">
      <img
        v-if="book.has_cover"
        :src="api.coverUrl(book.id, book.updated_at)"
        :alt="book.metadata.title"
        loading="lazy"
        class="h-full w-full object-cover transition duration-300 group-hover:scale-[1.03]"
      />
      <div
        v-else
        class="flex h-full w-full flex-col justify-between p-4 text-left"
      >
        <span class="self-end rounded-md bg-[var(--ctp-accent-soft)] px-2 py-0.5 text-[10px] font-bold uppercase text-[var(--ctp-accent)]">
          {{ book.format }}
        </span>
        <div>
          <p class="line-clamp-3 text-sm font-bold leading-snug text-[var(--ctp-text)]">
            {{ book.metadata.title }}
          </p>
          <p class="mt-1 truncate text-xs text-[var(--ctp-subtext0)]">
            {{ book.metadata.authors[0] || t('catalog.unknownAuthor') }}
          </p>
        </div>
      </div>

      <!-- Favorite ribbon indicator -->
      <div
        v-if="book.is_favorite"
        class="absolute right-2 top-2 flex h-6 w-6 items-center justify-center rounded-full bg-black/45 text-[var(--ctp-red)] backdrop-blur-sm"
      >
        <LuHeart class="h-3.5 w-3.5 fill-current" />
      </div>

      <!-- Format pill on hover -->
      <span
        class="absolute left-2 top-2 rounded-md bg-black/55 px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wide text-white opacity-0 backdrop-blur-sm transition group-hover:opacity-100"
      >
        {{ book.format }}
      </span>

      <!-- Missing file badge -->
      <span
        v-if="book.is_missing"
        class="absolute inset-x-2 bottom-2 rounded-md bg-[var(--ctp-red)] px-2 py-0.5 text-center text-[10px] font-semibold text-white"
      >
        {{ t('catalog.missingBadge') }}
      </span>

      <!-- Subtle bottom reading progress bar -->
      <div
        v-else-if="book.progress && book.progress.progress_percent > 0"
        class="absolute inset-x-0 bottom-0 h-1 bg-black/30"
      >
        <div
          class="h-full bg-[var(--ctp-accent)]"
          :style="{ width: `${Math.min(100, book.progress.progress_percent)}%` }"
        />
      </div>
    </div>

    <div class="mt-2.5 min-w-0 px-0.5">
      <h3 class="line-clamp-1 text-sm font-semibold text-[var(--ctp-text)] group-hover:text-[var(--ctp-accent)] transition-colors">
        {{ book.metadata.title }}
      </h3>
      <p class="mt-0.5 truncate text-xs text-[var(--ctp-subtext0)]">
        <template v-if="showProgressText && book.progress">
          {{ Math.round(book.progress.progress_percent) }}%
        </template>
        <template v-else>
          {{ book.metadata.authors.join(', ') || t('catalog.unknownAuthor') }}
        </template>
      </p>
    </div>
  </RouterLink>
</template>
