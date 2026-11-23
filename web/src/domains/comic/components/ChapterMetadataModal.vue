<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { MdButton, MdDialog, MdSelect, MdTextField } from '@/components/ui'
import { api } from '@/domains/comic/api'
import { useComicI18n as useI18n } from '@/domains/comic/i18n'
import type { ChapterItemDto, ChapterKind } from '@/domains/comic/types'
import { useSnackbarStore } from '@/stores/snackbar'

const props = defineProps<{
  open: boolean
  chapter: ChapterItemDto | null
}>()

const emit = defineEmits<{
  close: []
  saved: []
}>()

const { t } = useI18n()
const snackbar = useSnackbarStore()

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
  { immediate: true },
)

const kindOptions = computed(() => [
  { value: 'volume', label: t('metadata.chapterKindVolume') },
  { value: 'chapter', label: t('metadata.chapterKindChapter') },
  { value: 'special', label: t('metadata.chapterKindSpecial') },
])

const parsedNumber = computed(() => {
  const raw = number.value.trim()
  if (!raw) return Number.NaN
  return Number.parseFloat(raw)
})

const numberError = computed(() =>
  !Number.isFinite(parsedNumber.value) ? t('metadata.invalidChapterNumber') : false,
)

const canSave = computed(() => Boolean(props.chapter && Number.isFinite(parsedNumber.value) && !saving.value))

async function onSave(): Promise<void> {
  if (!props.chapter || !Number.isFinite(parsedNumber.value) || saving.value) return
  saving.value = true
  try {
    const volVal = volumeNumber.value.trim()
      ? Number.parseFloat(volumeNumber.value.trim())
      : null
    await api.updateChapterMetadata(props.chapter.id, {
      kind: kind.value,
      number: parsedNumber.value,
      volume_number: volVal !== null && Number.isFinite(volVal) ? volVal : null,
      title: title.value.trim(),
      summary: summary.value,
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
    :title="t('metadata.editChapterTitle')"
    size="md"
    @close="emit('close')"
  >
    <form class="space-y-4" @submit.prevent="onSave">
      <div class="space-y-1.5">
        <span class="text-label-medium block text-on-surface-variant">
          {{ t('metadata.chapterKind') }}
        </span>
        <MdSelect
          :model-value="kind"
          :options="kindOptions"
          @update:model-value="kind = $event as ChapterKind"
        />
      </div>

      <div class="grid grid-cols-2 gap-3">
        <MdTextField
          v-model="number"
          type="number"
          :label="t('metadata.chapterNumber')"
          :error="numberError"
          required
        />
        <MdTextField
          v-model="volumeNumber"
          type="number"
          :label="t('metadata.volumeNumber')"
        />
      </div>

      <MdTextField v-model="title" :label="t('metadata.title')" />

      <MdTextField
        v-model="summary"
        multiline
        :rows="3"
        :label="t('metadata.summary')"
      />
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
