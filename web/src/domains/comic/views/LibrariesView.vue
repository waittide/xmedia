<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { useComicI18n as useI18n } from '@/domains/comic/i18n'
import { LuFolderOpen, LuPlus, LuRefreshCw, LuTrash2 } from 'vue-icons-plus/lu'
import { api } from '@/domains/comic/api'
import { useAuthStore } from '@/stores/auth'
import { useDialogStore } from '@/stores/dialog'
import { useUploadStore } from '@/stores/uploads'
import MdDialog from '@/components/ui/MdDialog.vue'
import RefreshButton from '@/components/ui/RefreshButton.vue'
import type { Library } from '@/domains/comic/types'

const { t } = useI18n()
const router = useRouter()
const auth = useAuthStore()
const dialog = useDialogStore()
const upload = useUploadStore()

const libraries = ref<Library[]>([])
const scanningIds = ref<Record<string, boolean>>({})
const scanningAll = ref(false)
const createOpen = ref(false)
const newName = ref('')
const newPath = ref('')
const errorMsg = ref('')

async function loadLibraries(): Promise<void> {
  libraries.value = await api.listLibraries()
}

onMounted(loadLibraries)

async function createLibrary(): Promise<void> {
  errorMsg.value = ''
  try {
    const created = await api.createLibrary({
      name: newName.value,
      root_path: newPath.value,
    })
    createOpen.value = false
    newName.value = ''
    newPath.value = ''
    upload.notifyCatalogChanged()
    await loadLibraries()
    await scanSingle(created.id)
  } catch (e) {
    errorMsg.value = e instanceof Error ? e.message : String(e)
  }
}

async function scanSingle(id: string): Promise<void> {
  scanningIds.value[id] = true
  try {
    await Promise.all([
      (async () => {
        await api.scanLibrary(id)
        upload.notifyCatalogChanged()
        await loadLibraries()
      })(),
      new Promise((resolve) => setTimeout(resolve, 450)),
    ])
  } catch (e) {
    errorMsg.value = e instanceof Error ? e.message : String(e)
  } finally {
    scanningIds.value[id] = false
  }
}

async function scanAll(): Promise<void> {
  scanningAll.value = true
  try {
    await Promise.all([
      (async () => {
        await api.scanAllLibraries()
        upload.notifyCatalogChanged()
        await loadLibraries()
      })(),
      new Promise((resolve) => setTimeout(resolve, 450)),
    ])
  } finally {
    scanningAll.value = false
  }
}

async function removeLibrary(lib: Library): Promise<void> {
  const ok = await dialog.confirm({
    title: t('common.delete'),
    description: t('libraries.deleteConfirm', { name: lib.name }),
    danger: true,
    confirmText: t('common.delete'),
  })
  if (!ok) return
  await api.deleteLibrary(lib.id)
  upload.notifyCatalogChanged()
  await loadLibraries()
}
</script>

<template>
  <div class="mx-auto max-w-7xl space-y-6 px-6 py-7 sm:px-8">
    <div class="flex flex-wrap items-center justify-between gap-4">
      <div>
        <h2 class="text-lg font-bold text-[var(--md-sys-color-on-surface)]">
          {{ t('libraries.title') }}
        </h2>
        <p class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
          {{ t('libraries.subtitle') }}
        </p>
      </div>

      <div class="flex items-center gap-2.5">
        <RefreshButton size="md" :action="loadLibraries" />

        <template v-if="auth.isAdmin">
          <button
            type="button"
            :disabled="scanningAll"
            class="md-btn md-btn-sm md-btn-tonal md-state"
            @click="scanAll"
          >
            <LuRefreshCw class="h-3.5 w-3.5" :class="{ 'animate-spin': scanningAll }" />
            <span>{{ t('libraries.scanAll') }}</span>
          </button>

          <button
            type="button"
            class="md-btn md-btn-sm md-btn-filled md-state"
            @click="createOpen = true"
          >
            <LuPlus class="h-4 w-4" />
            <span>{{ t('libraries.addLibrary') }}</span>
          </button>
        </template>
      </div>
    </div>

    <p
      v-if="errorMsg"
      class="rounded-2xl bg-[var(--md-sys-color-error-container)] px-4 py-2.5 text-xs font-medium text-[var(--md-sys-color-on-error-container)]"
    >
      {{ errorMsg }}
    </p>

    <div class="grid grid-cols-1 gap-4 md:grid-cols-2">
      <div
        v-for="lib in libraries"
        :key="lib.id"
        class="md-card-filled flex flex-col justify-between gap-4 border border-[var(--md-sys-color-outline-variant)]/50 p-5"
      >
        <div class="space-y-2">
          <div class="flex items-center justify-between gap-2">
            <button
              type="button"
              class="flex items-center gap-2.5 text-left text-base font-bold text-[var(--md-sys-color-on-surface)] transition hover:text-[var(--md-sys-color-primary)]"
              @click="router.push({ path: '/comic/series', query: { library_id: lib.id } })"
            >
              <LuFolderOpen class="h-5 w-5 text-[var(--md-sys-color-primary)]" />
              <span>{{ lib.name }}</span>
            </button>
            <span
              class="rounded-full bg-[var(--md-sys-color-surface-container-high)] px-2.5 py-0.5 text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]"
            >
              {{ lib.kind === 'managed_upload' ? t('libraries.kindManaged') : t('libraries.kindLocal') }}
            </span>
          </div>

          <div class="truncate font-mono text-xs text-[var(--md-sys-color-on-surface-variant)]">
            {{ lib.root_path }}
          </div>

          <div class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
            <span
              v-if="lib.scan_state.state === 'scanning'"
              class="font-medium text-[var(--md-sys-color-primary)]"
            >
              {{ t('libraries.stateScanning') }}
            </span>
            <span v-else-if="lib.scan_state.state === 'completed'">
              {{
                t('libraries.stateCompleted', {
                  series: lib.scan_state.series_count,
                  chapters: lib.scan_state.chapter_count,
                })
              }}
            </span>
            <span
              v-else-if="lib.scan_state.state === 'failed'"
              class="text-[var(--md-sys-color-error)]"
            >
              {{ t('libraries.stateFailed', { error: lib.scan_state.error }) }}
            </span>
            <span v-else>{{ t('libraries.stateIdle') }}</span>
          </div>
        </div>

        <div
          v-if="auth.isAdmin && lib.kind !== 'managed_upload'"
          class="flex items-center justify-end gap-2 border-t border-[var(--md-sys-color-outline-variant)]/50 pt-3"
        >
          <button
            type="button"
            :disabled="scanningIds[lib.id]"
            class="md-btn md-btn-sm md-btn-tonal md-state"
            @click="scanSingle(lib.id)"
          >
            <LuRefreshCw class="h-3.5 w-3.5" :class="{ 'animate-spin': scanningIds[lib.id] }" />
            <span>{{ scanningIds[lib.id] ? t('libraries.scanning') : t('libraries.scanNow') }}</span>
          </button>

          <button
            type="button"
            class="md-icon-btn md-icon-btn-sm md-state hover:!text-[var(--md-sys-color-error)]"
            @click="removeLibrary(lib)"
          >
            <LuTrash2 class="h-4 w-4" />
          </button>
        </div>
      </div>
    </div>

    <MdDialog
      :open="createOpen"
      :title="t('libraries.addLibrary')"
      max-width="max-w-md"
      @close="createOpen = false"
    >
      <div class="space-y-4">
        <div>
          <label class="mb-1 block text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
            {{ t('libraries.nameLabel') }}
          </label>
          <input
            v-model="newName"
            type="text"
            :placeholder="t('libraries.namePlaceholder')"
            class="md-input h-10 w-full text-sm"
          />
        </div>
        <div>
          <label class="mb-1 block text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
            {{ t('libraries.pathLabel') }}
          </label>
          <input
            v-model="newPath"
            type="text"
            :placeholder="t('libraries.pathPlaceholder')"
            class="md-input h-10 w-full font-mono text-sm"
          />
        </div>
      </div>
      <template #footer>
        <div class="ml-auto flex items-center gap-2">
          <button
            type="button"
            class="md-btn md-btn-text md-state"
            @click="createOpen = false"
          >
            {{ t('common.cancel') }}
          </button>
          <button
            type="button"
            class="md-btn md-btn-filled md-state"
            @click="createLibrary"
          >
            {{ t('common.save') }}
          </button>
        </div>
      </template>
    </MdDialog>
  </div>
</template>
