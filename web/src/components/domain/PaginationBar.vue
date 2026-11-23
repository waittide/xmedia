<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { LuChevronDown, LuChevronLeft, LuChevronRight } from 'vue-icons-plus/lu'
import { MdButton, MdIconButton } from '@/components/ui'

export type PaginationMode = 'pages' | 'load-more'

type PageToken = number | 'ellipsis-start' | 'ellipsis-end'

const props = withDefaults(
  defineProps<{
    page?: number
    pageSize?: number
    total?: number | null
    totalPages?: number | null
    hasMore?: boolean
    mode?: PaginationMode
    disabled?: boolean
    loading?: boolean
    showSummary?: boolean
    compact?: boolean
    loadMoreLabel?: string
  }>(),
  {
    page: 1,
    pageSize: 24,
    total: null,
    totalPages: null,
    hasMore: false,
    mode: 'pages',
    disabled: false,
    loading: false,
    showSummary: true,
    compact: false,
    loadMoreLabel: '',
  },
)

const emit = defineEmits<{
  'update:page': [page: number]
  change: [page: number]
  'load-more': []
}>()

const { t } = useI18n()

const currentPage = computed(() => Math.max(1, Math.floor(props.page || 1)))

const computedTotalPages = computed<number | null>(() => {
  if (props.totalPages !== null && props.totalPages !== undefined && props.totalPages >= 0) {
    return Math.max(1, Math.floor(props.totalPages))
  }
  if (props.total !== null && props.total !== undefined && props.total >= 0) {
    const size = Math.max(1, Math.floor(props.pageSize || 24))
    return Math.max(1, Math.ceil(props.total / size))
  }
  return null
})

const canPrev = computed(() => !props.disabled && !props.loading && currentPage.value > 1)
const canNext = computed(() => {
  if (props.disabled || props.loading) return false
  if (computedTotalPages.value !== null) {
    return currentPage.value < computedTotalPages.value
  }
  return props.hasMore
})

const shouldRender = computed(() => {
  if (props.mode === 'load-more') {
    return props.hasMore || props.loading
  }
  if (computedTotalPages.value !== null) {
    return computedTotalPages.value > 1
  }
  return currentPage.value > 1 || props.hasMore
})

const pageTokens = computed<PageToken[]>(() => {
  const total = computedTotalPages.value
  if (total === null || total <= 1) return []
  if (total <= 7) {
    return Array.from({ length: total }, (_, idx) => idx + 1)
  }

  const cur = Math.min(total, currentPage.value)
  const tokens: PageToken[] = [1]
  const start = Math.max(2, cur - 1)
  const end = Math.min(total - 1, cur + 1)

  if (start > 2) {
    tokens.push('ellipsis-start')
  }
  for (let p = start; p <= end; p += 1) {
    tokens.push(p)
  }
  if (end < total - 1) {
    tokens.push('ellipsis-end')
  }
  tokens.push(total)
  return tokens
})

const navLabel = computed(() => t('pagination.navLabel'))
const prevLabel = computed(() => t('pagination.prev'))
const nextLabel = computed(() => t('pagination.next'))
const resolvedLoadMoreLabel = computed(() => {
  if (props.loadMoreLabel) return props.loadMoreLabel
  return t('common.loadMore')
})

const summaryText = computed(() => {
  const totalPagesVal = computedTotalPages.value
  if (totalPagesVal !== null) {
    if (props.total !== null && props.total !== undefined) {
      return t('pagination.pageOfTotalWithItems', {
        page: currentPage.value,
        totalPages: totalPagesVal,
        total: props.total,
      })
    }
    return t('pagination.pageOfTotal', {
      page: currentPage.value,
      totalPages: totalPagesVal,
    })
  }
  return t('pagination.pageSingle', { page: currentPage.value })
})

function goToPage(nextPage: number): void {
  if (props.disabled || props.loading) return
  const clamped =
    computedTotalPages.value !== null
      ? Math.max(1, Math.min(computedTotalPages.value, Math.floor(nextPage)))
      : Math.max(1, Math.floor(nextPage))
  if (clamped === currentPage.value) return
  emit('update:page', clamped)
  emit('change', clamped)
}
</script>

<template>
  <div v-if="shouldRender && mode === 'load-more'" class="flex justify-center pt-2">
    <MdButton
      variant="tonal"
      :icon="LuChevronDown"
      :loading="loading"
      :disabled="disabled || !hasMore"
      @click="emit('load-more')"
    >
      {{ resolvedLoadMoreLabel }}
    </MdButton>
  </div>

  <nav
    v-else-if="shouldRender"
    :aria-label="navLabel"
    class="flex flex-wrap items-center gap-3 pt-2"
    :class="showSummary && !compact ? 'justify-between' : 'justify-center'"
  >
    <span
      v-if="showSummary && !compact"
      class="text-body-small tabular-nums text-on-surface-variant"
    >
      {{ summaryText }}
    </span>

    <div class="flex items-center gap-1">
      <MdIconButton
        variant="standard"
        size="sm"
        :icon="LuChevronLeft"
        :aria-label="prevLabel"
        :disabled="!canPrev"
        @click="goToPage(currentPage - 1)"
      />

      <!-- Compact / mobile numeric indicator -->
      <span
        class="text-label-medium px-2 tabular-nums text-on-surface-variant"
        :class="compact || pageTokens.length === 0 ? 'inline-flex' : 'inline-flex sm:hidden'"
      >
        {{ computedTotalPages !== null ? `${currentPage} / ${computedTotalPages}` : currentPage }}
      </span>

      <!-- Desktop page number pills -->
      <div v-if="!compact && pageTokens.length > 0" class="hidden items-center gap-1 sm:flex">
        <template v-for="token in pageTokens" :key="String(token)">
          <span
            v-if="typeof token !== 'number'"
            class="text-label-medium inline-flex h-8 w-8 items-center justify-center text-on-surface-variant select-none"
            aria-hidden="true"
          >
            …
          </span>
          <button
            v-else
            type="button"
            :disabled="disabled || loading"
            :aria-current="token === currentPage ? 'page' : undefined"
            class="md-state md-touch-target text-label-medium inline-flex h-8 min-w-8 cursor-pointer items-center justify-center rounded-corner-full px-2 tabular-nums transition-colors duration-150 ease-standard disabled:cursor-not-allowed disabled:opacity-38"
            :class="
              token === currentPage
                ? 'bg-secondary-container text-on-secondary-container'
                : 'text-on-surface-variant hover:text-on-surface'
            "
            @click="goToPage(token)"
          >
            {{ token }}
          </button>
        </template>
      </div>

      <MdIconButton
        variant="standard"
        size="sm"
        :icon="LuChevronRight"
        :aria-label="nextLabel"
        :disabled="!canNext"
        @click="goToPage(currentPage + 1)"
      />
    </div>
  </nav>
</template>
