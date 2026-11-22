<script setup lang="ts">
import { ref, watch } from 'vue'
import { LuPencil } from 'vue-icons-plus/lu'
import { MdButton, MdDialog, MdTextField } from '@/components/ui'
import { api } from '@/domains/book/api'
import { useBookI18n as useI18n } from '@/domains/book/i18n'
import type { BookSummaryDto } from '@/domains/book/types'

const props = defineProps<{
  open: boolean
  book: BookSummaryDto | null
}>()

const emit = defineEmits<{
  close: []
  saved: [book: BookSummaryDto]
}>()

const { t } = useI18n()

const saving = ref(false)
const errorMsg = ref('')

const formTitle = ref('')
const formAuthors = ref('')
const formTags = ref('')
const formIsbn = ref('')
const formLanguage = ref('')
const formPublisher = ref('')
const formPublishedAt = ref('')
const formDescription = ref('')

watch(
  () => [props.open, props.book?.id] as const,
  ([isOpen]) => {
    if (!isOpen || !props.book) return
    errorMsg.value = ''
    const m = props.book.metadata
    formTitle.value = m.title
    formAuthors.value = m.authors.join(', ')
    formTags.value = m.tags.join(', ')
    formIsbn.value = m.isbn ?? ''
    formLanguage.value = m.language ?? ''
    formPublisher.value = m.publisher ?? ''
    formPublishedAt.value = m.published_at ?? ''
    formDescription.value = m.description ?? ''
  },
  { immediate: true },
)

function splitCsv(val: string): string[] {
  return val
    .split(/[,，]/)
    .map((s) => s.trim())
    .filter(Boolean)
}

async function handleSave(): Promise<void> {
  if (!props.book || saving.value) return
  saving.value = true
  errorMsg.value = ''
  try {
    const updated = await api.updateBook(props.book.id, {
      title: formTitle.value.trim(),
      authors: splitCsv(formAuthors.value),
      tags: splitCsv(formTags.value),
      isbn: formIsbn.value,
      language: formLanguage.value,
      publisher: formPublisher.value,
      published_at: formPublishedAt.value,
      description: formDescription.value,
    })
    emit('saved', updated)
    emit('close')
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <MdDialog
    :open="open && book !== null"
    :title="t('detail.editMetadata')"
    size="md"
    @close="emit('close')"
  >
    <template #icon>
      <LuPencil class="h-5 w-5" aria-hidden="true" />
    </template>

    <div
      v-if="errorMsg"
      role="alert"
      class="text-body-small mb-4 rounded-corner-md bg-error-container px-3.5 py-2.5 text-on-error-container"
    >
      {{ errorMsg }}
    </div>

    <form id="book-metadata-edit-form" class="space-y-4 py-1" @submit.prevent="handleSave">
      <MdTextField v-model="formTitle" :label="t('detail.bookTitle')" required />

      <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
        <MdTextField v-model="formAuthors" :label="t('detail.authors')" />
        <MdTextField v-model="formTags" :label="t('detail.tags')" />
      </div>

      <div class="grid grid-cols-2 gap-3 sm:grid-cols-4">
        <MdTextField
          v-model="formLanguage"
          size="sm"
          :label="t('detail.language')"
          placeholder="zh-CN / en"
        />
        <MdTextField v-model="formIsbn" size="sm" :label="t('detail.isbn')" />
        <MdTextField v-model="formPublisher" size="sm" :label="t('detail.publisher')" />
        <MdTextField
          v-model="formPublishedAt"
          size="sm"
          :label="t('detail.publishedAt')"
          placeholder="2024"
        />
      </div>

      <label class="flex flex-col gap-1">
        <span class="text-label-medium text-on-surface-variant">
          {{ t('detail.description') }}
        </span>
        <textarea
          v-model="formDescription"
          rows="5"
          class="text-body-medium w-full rounded-corner-md border border-outline-variant bg-surface-container-lowest px-3 py-2 text-on-surface outline-none transition-colors duration-150 ease-standard focus:border-primary focus:ring-1 focus:ring-primary"
        />
      </label>
    </form>

    <template #footer>
      <MdButton variant="text" :disabled="saving" @click="emit('close')">
        {{ t('detail.cancel') }}
      </MdButton>
      <MdButton
        type="submit"
        form="book-metadata-edit-form"
        variant="filled"
        :loading="saving"
      >
        {{ t('detail.saveChanges') }}
      </MdButton>
    </template>
  </MdDialog>
</template>
