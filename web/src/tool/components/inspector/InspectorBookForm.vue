<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import { LuFileText, LuLock, LuUnlock } from 'vue-icons-plus/lu'
import MdButton from '@/components/ui/MdButton.vue'
import MdIconButton from '@/components/ui/MdIconButton.vue'
import MdTextField from '@/components/ui/MdTextField.vue'
import type { BookInspectorForm } from './types'

const props = withDefaults(
  defineProps<{
    form: BookInspectorForm
    bookFormat: string
    writable: boolean
    isEpub: boolean
    hasCover: boolean
    polishingBook: boolean
    hideLocks?: boolean
  }>(),
  {
    hideLocks: false,
  },
)

defineEmits<{
  polish: []
}>()

const { t } = useI18n()

function isBookFieldLocked(field: string): boolean {
  return props.form.lockedFields.includes(field)
}

function toggleBookFieldLock(field: string): void {
  if (props.form.lockedFields.includes(field)) {
    props.form.lockedFields = props.form.lockedFields.filter((f) => f !== field)
  } else {
    props.form.lockedFields = [...props.form.lockedFields, field]
  }
}

function lockFilledBookFields(): void {
  const next = new Set(props.form.lockedFields)
  if (props.form.title.trim()) next.add('title')
  if (props.form.authors.trim()) next.add('authors')
  if (props.form.series.trim()) next.add('series')
  if (props.form.subseries.trim()) next.add('subseries')
  if (props.form.seriesIndex.trim()) next.add('series_index')
  if (props.form.isbn.trim()) next.add('isbn')
  if (props.form.publisher.trim()) next.add('publisher')
  if (props.form.publishedAt.trim()) next.add('published_at')
  if (props.form.language.trim()) next.add('language')
  if (props.form.tags.trim()) next.add('tags')
  if (props.form.description.trim()) next.add('description')
  if (props.hasCover) next.add('cover')
  props.form.lockedFields = Array.from(next)
}
</script>

<template>
  <div class="space-y-3">
    <div
      v-if="bookFormat !== 'epub'"
      class="text-body-small flex items-center gap-2 rounded-corner-md bg-primary-container/75 px-3 py-2 text-on-primary-container"
    >
      <LuFileText class="h-4 w-4 shrink-0" aria-hidden="true" />
      <span>{{ t('tool.forms.book.sidecarNotice', { format: bookFormat.toUpperCase() }) }}</span>
    </div>

    <div
      v-if="writable && !hideLocks"
      class="text-label-medium flex flex-wrap items-center justify-between gap-2 rounded-corner-md bg-surface px-3 py-1.5"
    >
      <span class="flex items-center gap-1.5 text-on-surface-variant">
        <LuLock class="h-3.5 w-3.5 text-warning" aria-hidden="true" />
        <span>{{ t('tool.forms.common.fieldLocks', { count: form.lockedFields.length }) }}</span>
      </span>
      <div class="flex items-center gap-1">
        <MdButton variant="text" @click="lockFilledBookFields">
          {{ t('tool.forms.common.lockFilled') }}
        </MdButton>
        <MdButton
          v-if="form.lockedFields.length > 0"
          variant="text"
          @click="form.lockedFields = []"
        >
          {{ t('tool.forms.common.unlockAll') }}
        </MdButton>
      </div>
    </div>

    <div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
      <div class="sm:col-span-2">
        <MdTextField
          v-model="form.title"
          size="sm"
          :disabled="!writable"
          :label="t('tool.forms.book.title')"
        >
          <template v-if="writable && !hideLocks" #trailing>
            <MdIconButton
              size="sm"
              class="-mr-2"
              :selected="isBookFieldLocked('title')"
              :icon="isBookFieldLocked('title') ? LuLock : LuUnlock"
              :aria-label="t('tool.forms.common.toggleLock', { field: t('tool.forms.book.title') })"
              @click="toggleBookFieldLock('title')"
            />
          </template>
        </MdTextField>
      </div>

      <div class="sm:col-span-2">
        <MdTextField
          v-model="form.authors"
          size="sm"
          :disabled="!writable"
          :label="t('tool.forms.book.authors')"
        >
          <template v-if="writable && !hideLocks" #trailing>
            <MdIconButton
              size="sm"
              class="-mr-2"
              :selected="isBookFieldLocked('authors')"
              :icon="isBookFieldLocked('authors') ? LuLock : LuUnlock"
              :aria-label="t('tool.forms.common.toggleLock', { field: t('tool.forms.book.authors') })"
              @click="toggleBookFieldLock('authors')"
            />
          </template>
        </MdTextField>
      </div>

      <div class="grid grid-cols-1 gap-2.5 sm:col-span-2 sm:grid-cols-12">
        <div class="sm:col-span-5">
          <MdTextField
            v-model="form.series"
            size="sm"
            :disabled="!writable"
            :label="t('tool.forms.book.series')"
            :placeholder="t('tool.forms.book.seriesPlaceholder')"
          >
            <template v-if="writable && !hideLocks" #trailing>
              <MdIconButton
                size="sm"
                class="-mr-2"
                :selected="isBookFieldLocked('series')"
                :icon="isBookFieldLocked('series') ? LuLock : LuUnlock"
                :aria-label="t('tool.forms.common.toggleLock', { field: t('tool.forms.book.series') })"
                @click="toggleBookFieldLock('series')"
              />
            </template>
          </MdTextField>
        </div>

        <div class="sm:col-span-4">
          <MdTextField
            v-model="form.subseries"
            size="sm"
            :disabled="!writable"
            :label="t('tool.forms.book.subseries')"
            :placeholder="t('tool.forms.book.subseriesPlaceholder')"
          >
            <template v-if="writable && !hideLocks" #trailing>
              <MdIconButton
                size="sm"
                class="-mr-2"
                :selected="isBookFieldLocked('subseries')"
                :icon="isBookFieldLocked('subseries') ? LuLock : LuUnlock"
                :aria-label="t('tool.forms.common.toggleLock', { field: t('tool.forms.book.subseries') })"
                @click="toggleBookFieldLock('subseries')"
              />
            </template>
          </MdTextField>
        </div>

        <div class="sm:col-span-3">
          <MdTextField
            v-model="form.seriesIndex"
            size="sm"
            :disabled="!writable"
            :label="t('tool.forms.book.seriesIndex')"
            :placeholder="t('tool.forms.book.seriesIndexPlaceholder')"
          >
            <template v-if="writable && !hideLocks" #trailing>
              <MdIconButton
                size="sm"
                class="-mr-2"
                :selected="isBookFieldLocked('series_index')"
                :icon="isBookFieldLocked('series_index') ? LuLock : LuUnlock"
                :aria-label="t('tool.forms.common.toggleLock', { field: t('tool.forms.book.seriesIndex') })"
                @click="toggleBookFieldLock('series_index')"
              />
            </template>
          </MdTextField>
        </div>
      </div>

      <MdTextField
        v-model="form.isbn"
        size="sm"
        :disabled="!writable"
        :label="t('tool.forms.book.isbn')"
      >
        <template v-if="writable && !hideLocks" #trailing>
          <MdIconButton
            size="sm"
            class="-mr-2"
            :selected="isBookFieldLocked('isbn')"
            :icon="isBookFieldLocked('isbn') ? LuLock : LuUnlock"
            :aria-label="t('tool.forms.common.toggleLock', { field: t('tool.forms.book.isbn') })"
            @click="toggleBookFieldLock('isbn')"
          />
        </template>
      </MdTextField>

      <MdTextField
        v-model="form.publisher"
        size="sm"
        :disabled="!writable"
        :label="t('tool.forms.book.publisher')"
      >
        <template v-if="writable && !hideLocks" #trailing>
          <MdIconButton
            size="sm"
            class="-mr-2"
            :selected="isBookFieldLocked('publisher')"
            :icon="isBookFieldLocked('publisher') ? LuLock : LuUnlock"
            :aria-label="t('tool.forms.common.toggleLock', { field: t('tool.forms.book.publisher') })"
            @click="toggleBookFieldLock('publisher')"
          />
        </template>
      </MdTextField>

      <MdTextField
        v-model="form.publishedAt"
        size="sm"
        :disabled="!writable"
        :label="t('tool.forms.book.publishedAt')"
        placeholder="YYYY-MM-DD"
      >
        <template v-if="writable && !hideLocks" #trailing>
          <MdIconButton
            size="sm"
            class="-mr-2"
            :selected="isBookFieldLocked('published_at')"
            :icon="isBookFieldLocked('published_at') ? LuLock : LuUnlock"
            :aria-label="t('tool.forms.common.toggleLock', { field: t('tool.forms.book.publishedAt') })"
            @click="toggleBookFieldLock('published_at')"
          />
        </template>
      </MdTextField>

      <MdTextField
        v-model="form.language"
        size="sm"
        :disabled="!writable"
        :label="t('tool.forms.book.language')"
        placeholder="zh / en / ja"
      >
        <template v-if="writable && !hideLocks" #trailing>
          <MdIconButton
            size="sm"
            class="-mr-2"
            :selected="isBookFieldLocked('language')"
            :icon="isBookFieldLocked('language') ? LuLock : LuUnlock"
            :aria-label="t('tool.forms.common.toggleLock', { field: t('tool.forms.book.language') })"
            @click="toggleBookFieldLock('language')"
          />
        </template>
      </MdTextField>

      <div class="sm:col-span-2">
        <MdTextField
          v-model="form.tags"
          size="sm"
          :disabled="!writable"
          :label="t('tool.forms.book.tags')"
        >
          <template v-if="writable && !hideLocks" #trailing>
            <MdIconButton
              size="sm"
              class="-mr-2"
              :selected="isBookFieldLocked('tags')"
              :icon="isBookFieldLocked('tags') ? LuLock : LuUnlock"
              :aria-label="t('tool.forms.common.toggleLock', { field: t('tool.forms.book.tags') })"
              @click="toggleBookFieldLock('tags')"
            />
          </template>
        </MdTextField>
      </div>

      <div class="sm:col-span-2">
        <MdTextField
          v-model="form.description"
          multiline
          :rows="5"
          :disabled="!writable"
          :label="t('tool.forms.book.description')"
        >
          <template v-if="writable && !hideLocks" #trailing>
            <MdIconButton
              size="sm"
              class="-mr-2"
              :selected="isBookFieldLocked('description')"
              :icon="isBookFieldLocked('description') ? LuLock : LuUnlock"
              :aria-label="t('tool.forms.common.toggleLock', { field: t('tool.forms.book.description') })"
              @click="toggleBookFieldLock('description')"
            />
          </template>
        </MdTextField>
      </div>
    </div>
  </div>
</template>
