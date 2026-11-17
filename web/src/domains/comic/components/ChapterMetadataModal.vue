<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useComicI18n as useI18n } from '@/domains/comic/i18n'
import AppDialog from '@/components/ui/AppDialog.vue'
import AppSelect from '@/components/ui/AppSelect.vue'
import { api } from '@/domains/comic/api'
import type { ChapterItemDto, ChapterKind } from '@/domains/comic/types'

const props = defineProps<{
  open: boolean
  chapter: ChapterItemDto | null
}>()

const emit = defineEmits<{
  close: []
  saved: []
}>()

const { t } = useI18n()
const kind = ref<ChapterKind>('chapter')
const number = ref('1')
const volumeNumber = ref('')
const title = ref('')
const summary = ref('')
const saving = ref(false)

watch(
  () => [props.open, props.chapter] as const,
  ([isOpen, ch]) => {
    if (isOpen && ch) {
      kind.value = ch.kind
      number.value = String(ch.number)
      volumeNumber.value = ch.volume_number != null ? String(ch.volume_number) : ''
      title.value = ch.title
      summary.value = ch.summary
    }
  },
  { immediate: true }
)

const kindOptions = computed(() => [
  { value: 'volume', label: t('metadata.chapterKindVolume') },
  { value: 'chapter', label: t('metadata.chapterKindChapter') },
  { value: 'special', label: t('metadata.chapterKindSpecial') },
])

async function onSave(): Promise<void> {
  if (!props.chapter) return
  saving.value = true
  try {
    const numVal = Number.parseFloat(number.value)
    const volVal = volumeNumber.value.trim()
      ? Number.parseFloat(volumeNumber.value.trim())
      : null
    await api.updateChapterMetadata(props.chapter.id, {
      kind: kind.value,
      number: Number.isFinite(numVal) ? numVal : props.chapter.number,
      volume_number: volVal !== null && Number.isFinite(volVal) ? volVal : null,
      title: title.value.trim(),
      summary: summary.value,
    })
    emit('saved')
    emit('close')
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <AppDialog
    :open="open"
    :title="t('metadata.editChapterTitle')"
    max-width-class="max-w-lg"
    @close="emit('close')"
  >
    <div class="space-y-4">
      <div>
        <label class="mb-1 block text-xs font-semibold text-[var(--text-primary)]">
          {{ t('metadata.chapterKind') }}
        </label>
        <AppSelect
          :model-value="kind"
          :options="kindOptions"
          @update:model-value="kind = $event as ChapterKind"
        />
      </div>

      <div class="grid grid-cols-2 gap-3">
        <div>
          <label class="mb-1 block text-xs font-semibold text-[var(--text-primary)]">
            {{ t('metadata.chapterNumber') }}
          </label>
          <input
            v-model="number"
            type="number"
            step="0.1"
            class="w-full rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-base)] px-3.5 py-2 text-sm text-[var(--text-primary)] focus:border-[var(--accent)] focus:outline-none"
          />
        </div>
        <div>
          <label class="mb-1 block text-xs font-semibold text-[var(--text-primary)]">
            {{ t('metadata.volumeNumber') }}
          </label>
          <input
            v-model="volumeNumber"
            type="number"
            step="0.1"
            class="w-full rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-base)] px-3.5 py-2 text-sm text-[var(--text-primary)] focus:border-[var(--accent)] focus:outline-none"
          />
        </div>
      </div>

      <div>
        <label class="mb-1 block text-xs font-semibold text-[var(--text-primary)]">
          {{ t('metadata.title') }}
        </label>
        <input
          v-model="title"
          type="text"
          class="w-full rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-base)] px-3.5 py-2 text-sm text-[var(--text-primary)] focus:border-[var(--accent)] focus:outline-none"
        />
      </div>

      <div>
        <label class="mb-1 block text-xs font-semibold text-[var(--text-primary)]">
          {{ t('metadata.summary') }}
        </label>
        <textarea
          v-model="summary"
          rows="3"
          class="w-full rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-base)] px-3.5 py-2 text-sm text-[var(--text-primary)] focus:border-[var(--accent)] focus:outline-none"
        />
      </div>
    </div>

    <template #footer>
      <button
        type="button"
        class="rounded-xl border border-[var(--border-subtle)] px-4 py-2 text-sm font-medium text-[var(--text-secondary)] hover:bg-[var(--surface-hover)]"
        @click="emit('close')"
      >
        {{ t('metadata.cancel') }}
      </button>
      <button
        type="button"
        :disabled="saving"
        class="rounded-xl bg-[var(--accent)] px-4 py-2 text-sm font-semibold text-[var(--accent-contrast)] hover:opacity-90 disabled:opacity-50"
        @click="onSave"
      >
        {{ t('metadata.save') }}
      </button>
    </template>
  </AppDialog>
</template>
