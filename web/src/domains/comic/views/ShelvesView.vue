<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useComicI18n as useI18n } from '@/domains/comic/i18n'
import { LuBookmark, LuPencil, LuPlus, LuTrash2 } from 'vue-icons-plus/lu'
import { api } from '@/domains/comic/api'
import { useDialogStore } from '@/stores/dialog'
import { useSidebarStore } from '@/domains/comic/store'
import MdDialog from '@/components/ui/MdDialog.vue'
import RefreshButton from '@/components/ui/RefreshButton.vue'
import SeriesCard from '@/domains/comic/components/SeriesCard.vue'
import type { ShelfDetailDto } from '@/domains/comic/types'

const { t } = useI18n()
const dialog = useDialogStore()
const sidebarStore = useSidebarStore()

const shelves = ref<ShelfDetailDto[]>([])
const modalOpen = ref(false)
const editingId = ref<string | null>(null)
const nameInput = ref('')
const descInput = ref('')

async function loadShelves(): Promise<void> {
  shelves.value = await api.listShelves()
}

async function refreshPage(): Promise<void> {
  await Promise.all([loadShelves(), sidebarStore.refresh()])
}

onMounted(loadShelves)

function openCreate(): void {
  editingId.value = null
  nameInput.value = ''
  descInput.value = ''
  modalOpen.value = true
}

function openEdit(item: ShelfDetailDto): void {
  editingId.value = item.shelf.id
  nameInput.value = item.shelf.name
  descInput.value = item.shelf.description
  modalOpen.value = true
}

async function saveShelf(): Promise<void> {
  if (!nameInput.value.trim()) return
  if (editingId.value) {
    await api.updateShelf(editingId.value, {
      name: nameInput.value,
      description: descInput.value,
    })
  } else {
    await api.createShelf({
      name: nameInput.value,
      description: descInput.value,
    })
  }
  modalOpen.value = false
  await Promise.all([loadShelves(), sidebarStore.refresh()])
}

async function removeShelf(item: ShelfDetailDto): Promise<void> {
  const ok = await dialog.confirm({
    title: t('shelves.deleteShelf'),
    description: item.shelf.name,
    danger: true,
    confirmText: t('common.delete'),
  })
  if (!ok) return
  await api.deleteShelf(item.shelf.id)
  await Promise.all([loadShelves(), sidebarStore.refresh()])
}
</script>

<template>
  <div class="mx-auto max-w-7xl space-y-8 px-6 py-7 sm:px-8">
    <div class="flex items-center justify-between gap-4">
      <div>
        <h2 class="text-lg font-bold text-[var(--md-sys-color-on-surface)]">
          {{ t('shelves.title') }}
        </h2>
        <p class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
          {{ t('shelves.subtitle') }}
        </p>
      </div>
      <div class="flex items-center gap-2">
        <RefreshButton size="md" :action="refreshPage" />
        <button
          type="button"
          class="md-btn md-btn-sm md-btn-filled md-state"
          @click="openCreate"
        >
          <LuPlus class="h-4 w-4" />
          <span>{{ t('shelves.createShelf') }}</span>
        </button>
      </div>
    </div>

    <div
      v-if="shelves.length === 0"
      class="md-card-outlined rounded-3xl border-dashed py-20 text-center text-sm text-[var(--md-sys-color-on-surface-variant)]"
    >
      {{ t('shelves.emptyShelves') }}
    </div>

    <section
      v-for="item in shelves"
      :key="item.shelf.id"
      class="md-card-filled space-y-4 rounded-3xl border border-[var(--md-sys-color-outline-variant)]/50 p-6"
    >
      <div class="flex items-center justify-between gap-3">
        <div class="flex items-center gap-2.5">
          <LuBookmark class="h-5 w-5 text-[var(--md-sys-color-primary)]" />
          <div>
            <h3 class="text-base font-bold text-[var(--md-sys-color-on-surface)]">
              {{ item.shelf.name }}
            </h3>
            <p
              v-if="item.shelf.description"
              class="text-xs text-[var(--md-sys-color-on-surface-variant)]"
            >
              {{ item.shelf.description }}
            </p>
          </div>
        </div>
        <div class="flex items-center gap-1">
          <button
            type="button"
            class="md-icon-btn md-icon-btn-sm md-state"
            @click="openEdit(item)"
          >
            <LuPencil class="h-4 w-4" />
          </button>
          <button
            type="button"
            class="md-icon-btn md-icon-btn-sm md-state hover:!text-[var(--md-sys-color-error)]"
            @click="removeShelf(item)"
          >
            <LuTrash2 class="h-4 w-4" />
          </button>
        </div>
      </div>

      <p
        v-if="item.series.length === 0"
        class="text-xs text-[var(--md-sys-color-on-surface-variant)]"
      >
        {{ t('shelves.emptySeriesInShelf') }}
      </p>
      <div
        v-else
        class="grid grid-cols-2 gap-5 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-6"
      >
        <SeriesCard v-for="s in item.series" :key="s.id" :series="s" />
      </div>
    </section>

    <MdDialog
      :open="modalOpen"
      :title="editingId ? t('shelves.editShelf') : t('shelves.createShelf')"
      max-width="max-w-md"
      @close="modalOpen = false"
    >
      <div class="space-y-4">
        <div>
          <label class="mb-1 block text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
            {{ t('shelves.shelfName') }}
          </label>
          <input
            v-model="nameInput"
            type="text"
            class="md-input h-10 w-full text-sm"
          />
        </div>
        <div>
          <label class="mb-1 block text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
            {{ t('shelves.shelfDesc') }}
          </label>
          <textarea
            v-model="descInput"
            rows="3"
            class="md-input w-full py-2.5 text-sm"
          />
        </div>
      </div>
      <template #footer>
        <div class="ml-auto flex items-center gap-2">
          <button
            type="button"
            class="md-btn md-btn-text md-state"
            @click="modalOpen = false"
          >
            {{ t('common.cancel') }}
          </button>
          <button
            type="button"
            class="md-btn md-btn-filled md-state"
            @click="saveShelf"
          >
            {{ t('common.save') }}
          </button>
        </div>
      </template>
    </MdDialog>
  </div>
</template>
