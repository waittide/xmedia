<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useComicI18n as useI18n } from '@/domains/comic/i18n'
import MdDialog from '@/components/ui/MdDialog.vue'
import AppSelect from '@/components/ui/AppSelect.vue'
import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import { api } from '@/domains/comic/api'
import type { ReadingDirection, Series, SeriesStatus } from '@/domains/comic/types'

const props = defineProps<{
  open: boolean
  series: Series | null
}>()

const emit = defineEmits<{
  close: []
  saved: []
}>()

const { t } = useI18n()

const title = ref('')
const altTitles = ref('')
const writers = ref('')
const pencillers = ref('')
const publisher = ref('')
const status = ref<SeriesStatus>('ongoing')
const readingDirection = ref<ReadingDirection>('rtl')
const ageRating = ref('')
const year = ref('')
const genres = ref('')
const tags = ref('')
const summary = ref('')
const metadataLocked = ref(true)
const saving = ref(false)

watch(
  () => [props.open, props.series] as const,
  ([isOpen, s]) => {
    if (isOpen && s) {
      title.value = s.title
      altTitles.value = s.alt_titles.join(', ')
      writers.value = s.writers.join(', ')
      pencillers.value = s.pencillers.join(', ')
      publisher.value = s.publisher || ''
      status.value = s.status
      readingDirection.value = s.reading_direction
      ageRating.value = s.age_rating || ''
      year.value = s.year ? String(s.year) : ''
      genres.value = s.genres.join(', ')
      tags.value = s.tags.join(', ')
      summary.value = s.summary
      metadataLocked.value = s.metadata_locked
    }
  },
  { immediate: true }
)

const statusOptions = computed(() => [
  { value: 'ongoing', label: t('series.statusOngoing') },
  { value: 'completed', label: t('series.statusCompleted') },
  { value: 'hiatus', label: t('series.statusHiatus') },
])

const directionOptions = computed(() => [
  { value: 'rtl', label: t('series.directionRtl') },
  { value: 'ltr', label: t('series.directionLtr') },
  { value: 'webtoon', label: t('series.directionWebtoon') },
])

function splitCsv(raw: string): string[] {
  return raw
    .split(/[,，;；]/)
    .map((x) => x.trim())
    .filter(Boolean)
}

async function onSave(): Promise<void> {
  if (!props.series) return
  saving.value = true
  try {
    const parsedYear = year.value.trim() ? Number.parseInt(year.value.trim(), 10) : null
    await api.updateSeriesMetadata(props.series.id, {
      title: title.value.trim(),
      alt_titles: splitCsv(altTitles.value),
      writers: splitCsv(writers.value),
      pencillers: splitCsv(pencillers.value),
      publisher: publisher.value.trim() || null,
      status: status.value,
      reading_direction: readingDirection.value,
      age_rating: ageRating.value.trim() || null,
      year: Number.isFinite(parsedYear) ? parsedYear : null,
      genres: splitCsv(genres.value),
      tags: splitCsv(tags.value),
      summary: summary.value,
      metadata_locked: metadataLocked.value,
    })
    emit('saved')
    emit('close')
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <MdDialog
    :open="open"
    :title="t('metadata.editSeriesTitle')"
    max-width="max-w-2xl"
    @close="emit('close')"
  >
    <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
      <div class="sm:col-span-2">
        <label class="mb-1 block text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
          {{ t('metadata.title') }}
        </label>
        <input
          v-model="title"
          type="text"
          class="md-input h-10 w-full text-sm"
        />
      </div>

      <div class="sm:col-span-2">
        <label class="mb-1 block text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
          {{ t('metadata.altTitles') }}
        </label>
        <input
          v-model="altTitles"
          type="text"
          class="md-input h-10 w-full text-sm"
        />
      </div>

      <div>
        <label class="mb-1 block text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
          {{ t('metadata.writers') }}
        </label>
        <input
          v-model="writers"
          type="text"
          class="md-input h-10 w-full text-sm"
        />
      </div>

      <div>
        <label class="mb-1 block text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
          {{ t('metadata.pencillers') }}
        </label>
        <input
          v-model="pencillers"
          type="text"
          class="md-input h-10 w-full text-sm"
        />
      </div>

      <div>
        <label class="mb-1 block text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
          {{ t('metadata.status') }}
        </label>
        <AppSelect
          :model-value="status"
          :options="statusOptions"
          @update:model-value="status = $event as SeriesStatus"
        />
      </div>

      <div>
        <label class="mb-1 block text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
          {{ t('metadata.readingDirection') }}
        </label>
        <AppSelect
          :model-value="readingDirection"
          :options="directionOptions"
          @update:model-value="readingDirection = $event as ReadingDirection"
        />
      </div>

      <div>
        <label class="mb-1 block text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
          {{ t('metadata.publisher') }}
        </label>
        <input
          v-model="publisher"
          type="text"
          class="md-input h-10 w-full text-sm"
        />
      </div>

      <div class="grid grid-cols-2 gap-3">
        <div>
          <label class="mb-1 block text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
            {{ t('metadata.year') }}
          </label>
          <input
            v-model="year"
            type="number"
            class="md-input h-10 w-full text-sm"
          />
        </div>
        <div>
          <label class="mb-1 block text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
            {{ t('metadata.ageRating') }}
          </label>
          <input
            v-model="ageRating"
            type="text"
            class="md-input h-10 w-full text-sm"
          />
        </div>
      </div>

      <div>
        <label class="mb-1 block text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
          {{ t('metadata.genres') }}
        </label>
        <input
          v-model="genres"
          type="text"
          class="md-input h-10 w-full text-sm"
        />
      </div>

      <div>
        <label class="mb-1 block text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
          {{ t('metadata.tags') }}
        </label>
        <input
          v-model="tags"
          type="text"
          class="md-input h-10 w-full text-sm"
        />
      </div>

      <div class="sm:col-span-2">
        <label class="mb-1 block text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
          {{ t('metadata.summary') }}
        </label>
        <textarea
          v-model="summary"
          rows="4"
          class="md-input w-full py-2.5 text-sm"
        />
      </div>

      <div class="sm:col-span-2 pt-1">
        <AppCheckbox v-model="metadataLocked" :label="t('metadata.lockMetadata')" />
      </div>
    </div>

    <template #footer>
      <div class="ml-auto flex items-center gap-2">
        <button
          type="button"
          class="md-btn md-btn-text md-state"
          @click="emit('close')"
        >
          {{ t('metadata.cancel') }}
        </button>
        <button
          type="button"
          :disabled="saving"
          class="md-btn md-btn-filled md-state"
          @click="onSave"
        >
          {{ t('metadata.save') }}
        </button>
      </div>
    </template>
  </MdDialog>
</template>
