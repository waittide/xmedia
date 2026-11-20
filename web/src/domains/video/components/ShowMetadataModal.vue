<script setup lang="ts">
import { ref, watch } from 'vue'
import { LuTv, LuUpload } from 'vue-icons-plus/lu'
import MdDialog from '@/components/ui/MdDialog.vue'
import { api } from '../api'
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

const title = ref('')
const summary = ref('')
const yearInput = ref('')
const genresInput = ref('')
const tagsInput = ref('')
const saving = ref(false)
const posterInputRef = ref<HTMLInputElement | null>(null)

watch(
  () => [props.open, props.show] as const,
  ([isOpen, s]) => {
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

function parseCsv(raw: string): string[] {
  return raw
    .split(',')
    .map((s) => s.trim())
    .filter(Boolean)
}

async function handleSave(): Promise<void> {
  if (!props.show || !title.value.trim()) return
  saving.value = true
  try {
    const yearNum = yearInput.value.trim() ? Number.parseInt(yearInput.value.trim(), 10) : null
    const updated = await api.updateShow(props.show.id, {
      title: title.value.trim(),
      summary: summary.value.trim(),
      year: Number.isFinite(yearNum) ? yearNum : null,
      genres: parseCsv(genresInput.value),
      tags: parseCsv(tagsInput.value),
    })
    await videoStore.refreshSidebar()
    videoStore.bumpCatalogVersion()
    emit('saved', updated)
    emit('close')
  } finally {
    saving.value = false
  }
}

async function onPosterFileChange(ev: Event): Promise<void> {
  const input = ev.target as HTMLInputElement
  const file = input.files?.[0]
  if (!file || !props.show) return
  saving.value = true
  try {
    const updated = await api.uploadShowPoster(props.show.id, file)
    videoStore.bumpCatalogVersion()
    emit('saved', updated)
  } finally {
    saving.value = false
    input.value = ''
  }
}
</script>

<template>
  <MdDialog
    :open="open"
    :title="t('metadata.editShowTitle')"
    max-width="max-w-xl"
    @close="emit('close')"
  >
    <template #icon>
      <LuTv class="h-5 w-5" />
    </template>

    <form class="space-y-4 py-1 text-xs" @submit.prevent="handleSave">
      <div>
        <label class="mb-1.5 block font-semibold text-[var(--md-sys-color-on-surface-variant)]">
          {{ t('metadata.fieldTitle') }}
        </label>
        <input v-model="title" type="text" required class="md-input h-9 w-full text-xs" />
      </div>

      <div>
        <label class="mb-1.5 block font-semibold text-[var(--md-sys-color-on-surface-variant)]">
          {{ t('metadata.fieldYear') }}
        </label>
        <input v-model="yearInput" type="number" class="md-input h-9 w-full text-xs" />
      </div>

      <div>
        <label class="mb-1.5 block font-semibold text-[var(--md-sys-color-on-surface-variant)]">
          {{ t('metadata.fieldGenres') }}
        </label>
        <input v-model="genresInput" type="text" class="md-input h-9 w-full text-xs" />
      </div>

      <div>
        <label class="mb-1.5 block font-semibold text-[var(--md-sys-color-on-surface-variant)]">
          {{ t('metadata.fieldTags') }}
        </label>
        <input v-model="tagsInput" type="text" class="md-input h-9 w-full text-xs" />
      </div>

      <div>
        <label class="mb-1.5 block font-semibold text-[var(--md-sys-color-on-surface-variant)]">
          {{ t('metadata.fieldSummary') }}
        </label>
        <textarea v-model="summary" rows="4" class="md-input w-full py-2.5 text-xs" />
      </div>

      <div>
        <input
          ref="posterInputRef"
          type="file"
          accept="image/*"
          class="hidden"
          @change="onPosterFileChange"
        />
        <button
          type="button"
          class="md-btn md-btn-sm md-btn-tonal md-state"
          @click="posterInputRef?.click()"
        >
          <LuUpload class="h-3.5 w-3.5" />
          <span>{{ t('common.uploadPoster') }}</span>
        </button>
      </div>
    </form>

    <template #footer>
      <div class="ml-auto flex items-center gap-2">
        <button
          type="button"
          class="md-btn md-btn-sm md-btn-text md-state"
          @click="emit('close')"
        >
          {{ t('common.cancel') }}
        </button>
        <button
          type="button"
          :disabled="saving || !title.trim()"
          class="md-btn md-btn-sm md-btn-filled md-state"
          @click="handleSave"
        >
          {{ t('common.save') }}
        </button>
      </div>
    </template>
  </MdDialog>
</template>
