<script setup lang="ts">
import { ref, watch } from 'vue'
import { LuFilm } from 'vue-icons-plus/lu'
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
import type { VideoSummaryDto } from '../types'
import PosterUploadField from './PosterUploadField.vue'

const props = defineProps<{
  open: boolean
  video: VideoSummaryDto | null
}>()

const emit = defineEmits<{
  close: []
  saved: [updated: VideoSummaryDto]
}>()

const { t } = useI18n()
const videoStore = useVideoStore()
const snackbar = useSnackbarStore()

const title = ref('')
const summary = ref('')
const yearInput = ref('')
const seasonInput = ref('')
const episodeInput = ref('')
const genresInput = ref('')
const tagsInput = ref('')
const saving = ref(false)

const { pendingPosterFile, previewPosterUrl, clearPendingPoster, selectPosterFile } =
  usePosterUploadPreview(() => props.video?.poster_key)

watch(
  () => [props.open, props.video] as const,
  ([isOpen, v]) => {
    clearPendingPoster()
    if (isOpen && v) {
      title.value = v.title
      summary.value = v.summary
      yearInput.value = v.year != null ? String(v.year) : ''
      seasonInput.value = v.season_number != null ? String(v.season_number) : ''
      episodeInput.value = v.episode_number != null ? String(v.episode_number) : ''
      genresInput.value = v.genres.join(', ')
      tagsInput.value = v.tags.join(', ')
    }
  },
  { immediate: true },
)

async function handleSave(): Promise<void> {
  if (!props.video || !title.value.trim()) return
  saving.value = true
  try {
    let updated = await api.updateVideo(props.video.id, {
      title: title.value.trim(),
      summary: summary.value.trim(),
      year: parseOptionalInt(yearInput.value),
      season_number: parseOptionalInt(seasonInput.value),
      episode_number: parseOptionalInt(episodeInput.value),
      genres: parseCsvList(genresInput.value),
      tags: parseCsvList(tagsInput.value),
    })

    if (pendingPosterFile.value) {
      updated = await api.uploadVideoPoster(props.video.id, pendingPosterFile.value)
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
    :title="t('metadata.editVideoTitle')"
    size="md"
    @close="emit('close')"
  >
    <template #icon>
      <LuFilm class="h-5 w-5" aria-hidden="true" />
    </template>

    <form class="space-y-4 py-1" @submit.prevent="handleSave">
      <PosterUploadField
        :preview-src="previewPosterUrl"
        :alt="title || video?.title || ''"
        aspect-class="aspect-video"
        width-class="w-28"
        :fallback-icon="LuFilm"
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

      <div class="grid grid-cols-3 gap-3">
        <MdTextField
          v-model="yearInput"
          type="number"
          size="sm"
          :label="t('metadata.fieldYear')"
        />
        <MdTextField
          v-model="seasonInput"
          type="number"
          size="sm"
          :label="t('metadata.fieldSeason')"
        />
        <MdTextField
          v-model="episodeInput"
          type="number"
          size="sm"
          :label="t('metadata.fieldEpisode')"
        />
      </div>

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
        :rows="3"
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
