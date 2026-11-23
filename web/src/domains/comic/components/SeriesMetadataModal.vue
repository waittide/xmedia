<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { LuChevronDown, LuChevronUp } from 'vue-icons-plus/lu'
import { AppCheckbox, MdButton, MdDialog, MdSelect, MdTextField } from '@/components/ui'
import { api } from '@/domains/comic/api'
import { useComicI18n as useI18n } from '@/domains/comic/i18n'
import type { ReadingDirection, Series, SeriesStatus } from '@/domains/comic/types'
import { useSnackbarStore } from '@/stores/snackbar'

const props = defineProps<{
  open: boolean
  series: Series | null
}>()

const emit = defineEmits<{
  close: []
  saved: []
}>()

const { t } = useI18n()
const snackbar = useSnackbarStore()

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
const showAdvanced = ref(false)
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
      showAdvanced.value = false
    }
  },
  { immediate: true },
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

const titleError = computed(() => (!title.value.trim() ? t('metadata.titleRequired') : false))
const canSave = computed(() => Boolean(props.series && title.value.trim() && !saving.value))

function splitCsv(raw: string): string[] {
  return raw
    .split(/[,，;；]/)
    .map((x) => x.trim())
    .filter(Boolean)
}

async function onSave(): Promise<void> {
  if (!props.series || !title.value.trim() || saving.value) return
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
      year: parsedYear !== null && Number.isFinite(parsedYear) ? parsedYear : null,
      genres: splitCsv(genres.value),
      tags: splitCsv(tags.value),
      summary: summary.value,
      metadata_locked: metadataLocked.value,
    })
    emit('saved')
    emit('close')
  } catch (err) {
    snackbar.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <MdDialog
    :open="open"
    :title="t('metadata.editSeriesTitle')"
    size="lg"
    @close="emit('close')"
  >
    <form class="space-y-5" @submit.prevent="onSave">
      <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
        <div class="sm:col-span-2">
          <MdTextField
            v-model="title"
            :label="t('metadata.title')"
            :error="titleError"
            required
          />
        </div>

        <MdTextField v-model="writers" :label="t('metadata.writers')" />
        <MdTextField v-model="pencillers" :label="t('metadata.pencillers')" />

        <div class="space-y-1.5">
          <span class="text-label-medium block text-on-surface-variant">
            {{ t('metadata.status') }}
          </span>
          <MdSelect
            :model-value="status"
            :options="statusOptions"
            @update:model-value="status = $event as SeriesStatus"
          />
        </div>

        <div class="space-y-1.5">
          <span class="text-label-medium block text-on-surface-variant">
            {{ t('metadata.readingDirection') }}
          </span>
          <MdSelect
            :model-value="readingDirection"
            :options="directionOptions"
            @update:model-value="readingDirection = $event as ReadingDirection"
          />
        </div>

        <div class="sm:col-span-2">
          <MdTextField
            v-model="summary"
            multiline
            :rows="4"
            :label="t('metadata.summary')"
          />
        </div>
      </div>

      <!-- Collapsible Publishing & Classification Group -->
      <div class="border-t border-outline-variant pt-3">
        <button
          type="button"
          :aria-expanded="showAdvanced"
          class="md-state text-title-small inline-flex cursor-pointer items-center gap-2 rounded-corner-full px-3 py-1.5 text-on-surface-variant hover:text-on-surface"
          @click="showAdvanced = !showAdvanced"
        >
          <span>{{ t('metadata.advancedSection') }}</span>
          <LuChevronUp v-if="showAdvanced" class="h-4 w-4" aria-hidden="true" />
          <LuChevronDown v-else class="h-4 w-4" aria-hidden="true" />
        </button>

        <div v-if="showAdvanced" class="mt-3 grid grid-cols-1 gap-4 sm:grid-cols-2">
          <div class="sm:col-span-2">
            <MdTextField v-model="altTitles" :label="t('metadata.altTitles')" />
          </div>
          <MdTextField v-model="publisher" :label="t('metadata.publisher')" />
          <div class="grid grid-cols-2 gap-3">
            <MdTextField v-model="year" type="number" :label="t('metadata.year')" />
            <MdTextField v-model="ageRating" :label="t('metadata.ageRating')" />
          </div>
          <MdTextField v-model="genres" :label="t('metadata.genres')" />
          <MdTextField v-model="tags" :label="t('metadata.tags')" />
        </div>
      </div>

      <AppCheckbox v-model="metadataLocked" :label="t('metadata.lockMetadata')" />
    </form>

    <template #footer>
      <MdButton variant="text" :disabled="saving" @click="emit('close')">
        {{ t('metadata.cancel') }}
      </MdButton>
      <MdButton variant="filled" :loading="saving" :disabled="!canSave" @click="onSave">
        {{ t('metadata.save') }}
      </MdButton>
    </template>
  </MdDialog>
</template>
