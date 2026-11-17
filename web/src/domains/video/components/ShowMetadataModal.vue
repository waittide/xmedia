<script setup lang="ts">
import { ref, watch } from 'vue'
import { LuUpload } from 'vue-icons-plus/lu'
import AppDialog from '@/components/ui/AppDialog.vue'
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
  <AppDialog
    :open="open"
    :title="t('metadata.editShowTitle')"
    max-width-class="max-w-xl"
    @close="emit('close')"
  >
    <form class="space-y-4 text-xs" @submit.prevent="handleSave">
      <div>
        <label class="mb-1 block font-semibold text-[var(--ctp-subtext1)]">
          {{ t('metadata.fieldTitle') }}
        </label>
        <input
          v-model="title"
          type="text"
          required
          class="h-9 w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
        />
      </div>

      <div>
        <label class="mb-1 block font-semibold text-[var(--ctp-subtext1)]">
          {{ t('metadata.fieldYear') }}
        </label>
        <input
          v-model="yearInput"
          type="number"
          class="h-9 w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
        />
      </div>

      <div>
        <label class="mb-1 block font-semibold text-[var(--ctp-subtext1)]">
          {{ t('metadata.fieldGenres') }}
        </label>
        <input
          v-model="genresInput"
          type="text"
          class="h-9 w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
        />
      </div>

      <div>
        <label class="mb-1 block font-semibold text-[var(--ctp-subtext1)]">
          {{ t('metadata.fieldTags') }}
        </label>
        <input
          v-model="tagsInput"
          type="text"
          class="h-9 w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
        />
      </div>

      <div>
        <label class="mb-1 block font-semibold text-[var(--ctp-subtext1)]">
          {{ t('metadata.fieldSummary') }}
        </label>
        <textarea
          v-model="summary"
          rows="4"
          class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] p-3 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
        />
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
          class="inline-flex items-center gap-1.5 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-1.5 text-xs font-semibold text-[var(--ctp-subtext1)] transition hover:text-[var(--ctp-text)]"
          @click="posterInputRef?.click()"
        >
          <LuUpload class="h-3.5 w-3.5" />
          <span>{{ t('common.uploadPoster') }}</span>
        </button>
      </div>
    </form>

    <template #footer>
      <button
        type="button"
        class="rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-4 py-2 text-xs font-semibold text-[var(--ctp-subtext1)] hover:text-[var(--ctp-text)]"
        @click="emit('close')"
      >
        {{ t('common.cancel') }}
      </button>
      <button
        type="button"
        :disabled="saving || !title.trim()"
        class="rounded-xl bg-[var(--ctp-accent)] px-4 py-2 text-xs font-bold text-[var(--ctp-on-accent)] disabled:opacity-50"
        @click="handleSave"
      >
        {{ t('common.save') }}
      </button>
    </template>
  </AppDialog>
</template>
