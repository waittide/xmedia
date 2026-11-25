<script setup lang="ts">
import { ref, watch } from 'vue'
import { LuTv } from 'vue-icons-plus/lu'
import { MdButton, MdDialog, MdTextField } from '@/components/ui'
import { useSnackbarStore } from '@/stores/snackbar'
import { api } from '../api'
import {
  parseCsvList,
  parseOptionalInt,
  usePosterUploadPreview,
} from '../composables/usePosterUploadPreview'
import { useVideoI18n as useI18n } from '../i18n'
import { useVideoStore } from '../store'
import type { ShowSummaryDto } from '../types'
import PosterUploadField from './PosterUploadField.vue'

const props = defineProps<{
  open: boolean
  show: ShowSummaryDto | null
}>()

const emit = defineEmits<{
  close: []
  saved: [updated: ShowSummaryDto]
}>()

const { t } = useI18n()
const videoStore = useVideoStore()
const snackbar = useSnackbarStore()

const title = ref('')
const summary = ref('')
const yearInput = ref('')
const genresInput = ref('')
const tagsInput = ref('')
const saving = ref(false)

const { pendingPosterFile, previewPosterUrl, clearPendingPoster, selectPosterFile } =
  usePosterUploadPreview(() => props.show?.poster_key)

watch(
  () => [props.open, props.show] as const,
  ([isOpen, s]) => {
    clearPendingPoster()
    if (isOpen && s) {
      title.value = s.title
      summary.value = s.summary
      yearInput.value = s.year != null ? String(s.year) : ''
      genresInput.value = s.genres.join(', ')
      tagsInput.value = s.tags.join(', ')
    }
  },
  { immediate: true },
)

async function handleSave(): Promise<void> {
  if (!props.show || !title.value.trim()) return
  saving.value = true
  try {
    let updated = await api.updateShow(props.show.id, {
      title: title.value.trim(),
      summary: summary.value.trim(),
      year: parseOptionalInt(yearInput.value),
      genres: parseCsvList(genresInput.value),
      tags: parseCsvList(tagsInput.value),
    })

    if (pendingPosterFile.value) {
      updated = await api.uploadShowPoster(props.show.id, pendingPosterFile.value)
    }

    clearPendingPoster()
    await videoStore.refreshSidebar(true)
    videoStore.bumpCatalogVersion()
    emit('saved', updated)
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
    :title="t('metadata.editShowTitle')"
    size="md"
    @close="emit('close')"
  >
    <template #icon>
      <LuTv class="h-5 w-5" aria-hidden="true" />
    </template>

    <form class="space-y-4 py-1" @submit.prevent="handleSave">
      <PosterUploadField
        :preview-src="previewPosterUrl"
        :alt="title || show?.title || ''"
        aspect-class="aspect-[2/3]"
        width-class="w-16"
        :fallback-icon="LuTv"
        :button-label="t('common.uploadPoster')"
        :pending-file-name="pendingPosterFile?.name ?? null"
        @select-file="selectPosterFile"
      />

      <MdTextField
        v-model="title"
        size="sm"
        required
        :label="t('metadata.fieldTitle')"
      />

      <MdTextField
        v-model="yearInput"
        type="number"
        size="sm"
        :label="t('metadata.fieldYear')"
      />

      <MdTextField
        v-model="genresInput"
        size="sm"
        :label="t('metadata.fieldGenres')"
      />

      <MdTextField
        v-model="tagsInput"
        size="sm"
        :label="t('metadata.fieldTags')"
      />

      <MdTextField
        v-model="summary"
        multiline
        :rows="4"
        :label="t('metadata.fieldSummary')"
      />
    </form>

    <template #footer>
      <MdButton variant="text" :disabled="saving" @click="emit('close')">
        {{ t('common.cancel') }}
      </MdButton>
      <MdButton
        variant="filled"
        :loading="saving"
        :disabled="!title.trim()"
        @click="handleSave"
      >
        {{ t('common.save') }}
      </MdButton>
    </template>
  </MdDialog>
</template>
