<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { LuTv, LuUpload } from 'vue-icons-plus/lu'
import { MdButton, MdDialog, MdTextField, MediaCoverImage } from '@/components/ui'
import { useSnackbarStore } from '@/stores/snackbar'
import { api, buildPosterUrl } from '../api'
import { useVideoI18n as useI18n } from '../i18n'
import { useVideoStore } from '../store'
import type { ShowSummaryDto } from '../types'

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
const pendingPosterFile = ref<File | null>(null)
const pendingPosterPreviewUrl = ref<string | null>(null)
const posterInputRef = ref<HTMLInputElement | null>(null)

function clearPendingPoster(): void {
  if (pendingPosterPreviewUrl.value) {
    URL.revokeObjectURL(pendingPosterPreviewUrl.value)
    pendingPosterPreviewUrl.value = null
  }
  pendingPosterFile.value = null
}

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

onBeforeUnmount(clearPendingPoster)

const previewPosterUrl = computed(
  () => pendingPosterPreviewUrl.value || buildPosterUrl(props.show?.poster_key, 320),
)

function parseCsv(raw: string): string[] {
  return raw
    .split(',')
    .map((s) => s.trim())
    .filter(Boolean)
}

function onPosterFileChange(ev: Event): void {
  const input = ev.target as HTMLInputElement
  const file = input.files?.[0]
  input.value = ''
  if (!file) return
  clearPendingPoster()
  pendingPosterFile.value = file
  pendingPosterPreviewUrl.value = URL.createObjectURL(file)
}

async function handleSave(): Promise<void> {
  if (!props.show || !title.value.trim()) return
  saving.value = true
  try {
    const yearNum = yearInput.value.trim() ? Number.parseInt(yearInput.value.trim(), 10) : null
    let updated = await api.updateShow(props.show.id, {
      title: title.value.trim(),
      summary: summary.value.trim(),
      year: Number.isFinite(yearNum) ? yearNum : null,
      genres: parseCsv(genresInput.value),
      tags: parseCsv(tagsInput.value),
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
      <div class="flex items-center gap-4">
        <div class="w-16 shrink-0 overflow-hidden rounded-corner-sm bg-surface-container-high">
          <MediaCoverImage
            :src="previewPosterUrl"
            :alt="title || show?.title || ''"
            aspect-class="aspect-[2/3]"
            rounded="rounded-corner-sm"
            :fallback-icon="LuTv"
            fallback-icon-class="h-6 w-6 text-on-surface-variant/50"
          />
        </div>
        <div class="space-y-1.5">
          <input
            ref="posterInputRef"
            type="file"
            accept="image/*"
            class="hidden"
            @change="onPosterFileChange"
          />
          <MdButton
            variant="tonal"
            size="sm"
            :icon="LuUpload"
            @click="posterInputRef?.click()"
          >
            {{ t('common.uploadPoster') }}
          </MdButton>
          <p v-if="pendingPosterFile" class="text-body-small truncate text-on-surface-variant">
            {{ pendingPosterFile.name }}
          </p>
        </div>
      </div>

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
