<script setup lang="ts">
import { ref, watch } from 'vue'
import { LuRepeat } from 'vue-icons-plus/lu'
import { AppCheckbox, MdCircularProgress, MdDialog, MdEmptyState } from '@/components/ui'
import { useBookI18n as useI18n } from '@/domains/book/i18n'
import { onlineBookApi, type AggregatedSearchItemDto } from '@/domains/book/onlineApi'
import { useSnackbarStore } from '@/stores/snackbar'

const props = withDefaults(
  defineProps<{
    open: boolean
    bookTitle: string
    currentSourceId?: string | null
    currentBookUrl?: string | null
    showClearCacheOption?: boolean
    switchingKey?: string | null
    candidates?: readonly AggregatedSearchItemDto[]
    searching?: boolean
  }>(),
  {
    currentSourceId: null,
    currentBookUrl: null,
    showClearCacheOption: false,
    switchingKey: null,
    candidates: undefined,
    searching: undefined,
  },
)

const emit = defineEmits<{
  close: []
  select: [item: AggregatedSearchItemDto, clearCache: boolean]
}>()

const { t } = useI18n()
const snackbar = useSnackbarStore()

const internalSearching = ref(false)
const internalCandidates = ref<AggregatedSearchItemDto[]>([])
const clearCache = ref(false)
let searchSeq = 0

async function searchAlternatives(title: string): Promise<void> {
  const kw = title.trim()
  if (!kw) {
    internalCandidates.value = []
    return
  }
  const seq = ++searchSeq
  internalSearching.value = true
  internalCandidates.value = []
  try {
    const res = await onlineBookApi.search(kw, undefined, 1)
    if (seq !== searchSeq) return
    internalCandidates.value = res.items
  } catch (err) {
    if (seq !== searchSeq) return
    snackbar.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    if (seq === searchSeq) {
      internalSearching.value = false
    }
  }
}

watch(
  () => [props.open, props.bookTitle] as const,
  ([isOpen, title]) => {
    if (!isOpen) {
      searchSeq += 1
      internalSearching.value = false
      return
    }
    clearCache.value = false
    if (props.candidates === undefined) {
      void searchAlternatives(title)
    }
  },
  { immediate: true },
)

function itemKey(item: AggregatedSearchItemDto): string {
  return `${item.source_id}:${item.book.book_url}`
}

function isCurrent(item: AggregatedSearchItemDto): boolean {
  return (
    Boolean(props.currentSourceId) &&
    item.source_id === props.currentSourceId &&
    (!props.currentBookUrl || item.book.book_url === props.currentBookUrl)
  )
}
</script>

<template>
  <MdDialog
    :open="open"
    :title="t('online.switchSourceTitle', { title: bookTitle })"
    :subtitle="t('online.switchSourceSubtitle')"
    size="md"
    @close="emit('close')"
  >
    <template #icon>
      <LuRepeat class="h-5 w-5" aria-hidden="true" />
    </template>

    <div class="space-y-3 py-1">
      <div
        v-if="showClearCacheOption"
        class="rounded-corner-md bg-surface-container px-3.5 py-2.5"
      >
        <AppCheckbox v-model="clearCache" :label="t('online.clearCacheOnSwitch')" />
      </div>

      <div
        v-if="searching !== undefined ? searching : internalSearching"
        class="flex flex-col items-center justify-center gap-2.5 py-10 text-on-surface-variant"
      >
        <MdCircularProgress indeterminate :size="28" :stroke-width="3" />
        <span class="text-body-small">{{ t('online.searchingSources') }}</span>
      </div>

      <MdEmptyState
        v-else-if="(candidates ?? internalCandidates).length === 0"
        compact
        :icon="LuRepeat"
        :title="t('online.noAltSources')"
      />

      <div v-else class="space-y-2">
        <button
          v-for="(item, idx) in candidates ?? internalCandidates"
          :key="`alt-${item.source_id}-${idx}`"
          type="button"
          :disabled="switchingKey !== null"
          class="md-state flex w-full cursor-pointer items-center justify-between gap-3 rounded-corner-md bg-surface-container px-3.5 py-3 text-left transition-colors duration-150 ease-standard hover:bg-surface-container-high disabled:cursor-not-allowed disabled:opacity-50"
          :class="isCurrent(item) ? 'ring-1 ring-primary' : ''"
          @click="emit('select', item, clearCache)"
        >
          <div class="min-w-0 flex-1">
            <div class="flex flex-wrap items-center gap-2">
              <span class="text-title-small truncate text-on-surface">
                {{ item.book.name }}
              </span>
              <span
                class="text-label-small shrink-0 rounded-corner-full bg-primary-container px-2 py-0.5 text-on-primary-container"
              >
                {{ item.source_name }}
              </span>
            </div>
            <p class="text-body-small mt-0.5 truncate text-on-surface-variant">
              {{ item.book.author || t('catalog.unknownAuthor') }}
              <span v-if="item.book.last_chapter"> · {{ item.book.last_chapter }}</span>
            </p>
          </div>

          <MdCircularProgress
            v-if="switchingKey === itemKey(item)"
            indeterminate
            :size="20"
            :stroke-width="2.5"
          />
        </button>
      </div>
    </div>
  </MdDialog>
</template>
