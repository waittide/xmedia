<script setup lang="ts">
import { ref, watch } from 'vue'
import { MdButton, MdDialog, MdTextField } from '@/components/ui'
import { api } from '@/domains/comic/api'
import { useComicI18n } from '@/domains/comic/i18n'
import { useSidebarStore } from '@/domains/comic/store'
import type { ShelfDetailDto } from '@/domains/comic/types'
import { useSnackbarStore } from '@/stores/snackbar'

const props = defineProps<{
  open: boolean
  editingShelf: ShelfDetailDto | null
}>()

const emit = defineEmits<{
  close: []
}>()

const { t } = useComicI18n()
const sidebarStore = useSidebarStore()
const snackbarStore = useSnackbarStore()

const shelfNameInput = ref('')
const shelfDescInput = ref('')
const savingShelf = ref(false)

watch(
  () => [props.open, props.editingShelf] as const,
  ([isOpen, target]) => {
    if (!isOpen) return
    shelfNameInput.value = target?.shelf.name ?? ''
    shelfDescInput.value = target?.shelf.description ?? ''
  },
  { immediate: true },
)

async function saveShelf(): Promise<void> {
  const trimmed = shelfNameInput.value.trim()
  if (!trimmed || savingShelf.value) return
  savingShelf.value = true
  try {
    if (props.editingShelf) {
      await api.updateShelf(props.editingShelf.shelf.id, {
        name: trimmed,
        description: shelfDescInput.value.trim(),
      })
    } else {
      await api.createShelf({
        name: trimmed,
        description: shelfDescInput.value.trim(),
      })
    }
    emit('close')
    await sidebarStore.refresh()
  } catch (err) {
    snackbarStore.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    savingShelf.value = false
  }
}
</script>

<template>
  <MdDialog
    :open="open"
    :title="editingShelf ? t('shelves.editShelf') : t('shelves.createShelf')"
    size="sm"
    @close="emit('close')"
  >
    <form class="space-y-4" @submit.prevent="saveShelf">
      <MdTextField
        v-model="shelfNameInput"
        :label="t('shelves.shelfName')"
        :placeholder="t('shelves.shelfNamePlaceholder')"
        required
      />
      <MdTextField v-model="shelfDescInput" :label="t('shelves.shelfDesc')" />
    </form>
    <template #footer>
      <MdButton variant="text" :disabled="savingShelf" @click="emit('close')">
        {{ t('common.cancel') }}
      </MdButton>
      <MdButton
        variant="filled"
        :loading="savingShelf"
        :disabled="!shelfNameInput.trim() || savingShelf"
        @click="saveShelf"
      >
        {{ t('common.save') }}
      </MdButton>
    </template>
  </MdDialog>
</template>
