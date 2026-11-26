<script setup lang="ts">
import { ref, watch } from 'vue'
import { LuFolderHeart, LuPlus } from 'vue-icons-plus/lu'
import { MdButton, MdDialog, MdEmptyState, MdListItem, MdTextField } from '@/components/ui'
import { useSnackbarStore } from '@/stores/snackbar'
import { api } from '../api'
import { useVideoI18n as useI18n } from '../i18n'
import { useVideoStore } from '../store'

const props = defineProps<{
  open: boolean
  videoId: string | null
  videoTitle?: string
}>()

const emit = defineEmits<{
  close: []
  added: [shelfId: string]
}>()

const { t } = useI18n()
const videoStore = useVideoStore()
const snackbar = useSnackbarStore()

const newShelfName = ref('')
const busyShelfId = ref<string | null>(null)
const creating = ref(false)

watch(
  () => props.open,
  (isOpen) => {
    if (isOpen) {
      newShelfName.value = ''
      void videoStore.refreshSidebar(false)
    }
  },
)

async function handleSelectShelf(shelfId: string, shelfName: string): Promise<void> {
  if (!props.videoId || busyShelfId.value) return
  busyShelfId.value = shelfId
  try {
    await videoStore.addVideoToShelf(shelfId, props.videoId)
    snackbar.show({ message: t('shelf.addedToShelf', { name: shelfName }) })
    emit('added', shelfId)
    emit('close')
  } catch (err) {
    snackbar.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    busyShelfId.value = null
  }
}

async function handleCreateAndAdd(): Promise<void> {
  const trimmed = newShelfName.value.trim()
  if (!trimmed || !props.videoId || creating.value) return
  creating.value = true
  try {
    const created = await api.createShelf({ name: trimmed, video_ids: [props.videoId] })
    await videoStore.refreshSidebar(true)
    snackbar.show({ message: t('shelf.addedToShelf', { name: created.shelf.name }) })
    emit('added', created.shelf.id)
    emit('close')
  } catch (err) {
    snackbar.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    creating.value = false
  }
}
</script>

<template>
  <MdDialog
    :open="open"
    :title="t('shelf.pickerTitle')"
    :subtitle="videoTitle"
    size="sm"
    @close="emit('close')"
  >
    <div class="space-y-4">
      <form class="flex items-center gap-2" @submit.prevent="handleCreateAndAdd">
        <div class="min-w-0 flex-1">
          <MdTextField
            v-model="newShelfName"
            size="sm"
            :placeholder="t('nav.shelfNamePlaceholder')"
          />
        </div>
        <MdButton
          type="submit"
          variant="tonal"
          :icon="LuPlus"
          :loading="creating"
          :disabled="!newShelfName.trim() || !videoId"
        >
          {{ t('shelf.createBtn') }}
        </MdButton>
      </form>

      <div
        v-if="videoStore.shelves.length > 0"
        class="max-h-72 space-y-1 overflow-y-auto rounded-corner-md bg-surface-container-low p-1.5"
      >
        <MdListItem
          v-for="sh in videoStore.shelves"
          :key="sh.id"
          :headline="sh.name"
          :trailing-supporting-text="t('common.videosCount', { count: sh.video_count })"
          :lines="1"
          clickable
          :disabled="busyShelfId !== null"
          @click="handleSelectShelf(sh.id, sh.name)"
        >
          <template #leading>
            <div
              class="flex h-9 w-9 items-center justify-center rounded-corner-sm bg-primary-container text-on-primary-container"
            >
              <LuFolderHeart class="h-4 w-4" aria-hidden="true" />
            </div>
          </template>
        </MdListItem>
      </div>

      <MdEmptyState
        v-else
        compact
        :icon="LuFolderHeart"
        :title="t('shelf.emptyShelves')"
        :description="t('shelf.emptyShelvesDesc')"
      />
    </div>

    <template #footer>
      <MdButton variant="text" @click="emit('close')">
        {{ t('common.cancel') }}
      </MdButton>
    </template>
  </MdDialog>
</template>
