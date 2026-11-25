<script setup lang="ts">
import { ref } from 'vue'
import { LuPlus, LuTrash2 } from 'vue-icons-plus/lu'
import { MdButton, MdIconButton, MdTextField } from '@/components/ui'
import { useBookI18n as useI18n } from '@/domains/book/i18n'
import type { Bookmark } from '@/domains/book/types'

defineProps<{
  bookmarks: Bookmark[]
}>()

const emit = defineEmits<{
  add: [label: string]
  select: [locator: string, scrollRatio: number]
  remove: [bookmarkId: string]
}>()

const { t } = useI18n()
const bookmarkLabel = ref('')

function handleSubmit(): void {
  const label = bookmarkLabel.value.trim()
  bookmarkLabel.value = ''
  emit('add', label)
}
</script>

<template>
  <div class="space-y-4">
    <form class="space-y-2.5" @submit.prevent="handleSubmit">
      <MdTextField
        v-model="bookmarkLabel"
        size="sm"
        :placeholder="t('reader.bookmarkTitlePlaceholder')"
      />
      <MdButton type="submit" variant="filled" :icon="LuPlus" block>
        {{ t('reader.addBookmark') }}
      </MdButton>
    </form>

    <p
      v-if="bookmarks.length === 0"
      class="text-body-small py-6 text-center text-on-surface-variant"
    >
      {{ t('reader.noBookmarks') }}
    </p>

    <div v-else class="space-y-1.5">
      <div
        v-for="bm in bookmarks"
        :key="bm.id"
        class="flex items-center justify-between gap-2 rounded-corner-md bg-surface-container px-3 py-2.5"
      >
        <button
          type="button"
          class="md-state min-w-0 flex-1 cursor-pointer rounded-corner-sm text-left"
          @click="emit('select', bm.location.locator, bm.location.scroll_ratio ?? 0)"
        >
          <div class="text-title-small truncate text-on-surface">{{ bm.title }}</div>
          <div class="text-label-small mt-0.5 tabular-nums text-primary">
            {{ Math.round(bm.location.progress_percent) }}%
          </div>
        </button>
        <MdIconButton
          size="sm"
          variant="standard"
          :icon="LuTrash2"
          :aria-label="t('common.delete')"
          @click="emit('remove', bm.id)"
        />
      </div>
    </div>
  </div>
</template>
