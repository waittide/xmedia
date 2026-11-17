<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { useComicI18n as useI18n } from '@/domains/comic/i18n'
import { LuFolderOpen, LuPlus, LuRefreshCw, LuTrash2 } from 'vue-icons-plus/lu'
import { api } from '@/domains/comic/api'
import { useAuthStore } from '@/stores/auth'
import { useDialogStore } from '@/stores/dialog'
import { useUploadStore } from '@/stores/uploads'
import AppDialog from '@/components/ui/AppDialog.vue'
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
    await api.scanLibrary(id)
    upload.notifyCatalogChanged()
    await loadLibraries()
  } catch (e) {
    errorMsg.value = e instanceof Error ? e.message : String(e)
  } finally {
    scanningIds.value[id] = false
  }
}

async function scanAll(): Promise<void> {
  scanningAll.value = true
  try {
    await api.scanAllLibraries()
    upload.notifyCatalogChanged()
    await loadLibraries()
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
  <div class="mx-auto max-w-5xl space-y-6 p-6 sm:p-8">
    <div class="flex flex-wrap items-center justify-between gap-4">
      <div>
        <h2 class="text-lg font-bold text-[var(--text-primary)]">{{ t('libraries.title') }}</h2>
        <p class="text-xs text-[var(--text-secondary)]">{{ t('libraries.subtitle') }}</p>
      </div>

      <div v-if="auth.isAdmin" class="flex items-center gap-2.5">
        <button
          type="button"
          :disabled="scanningAll"
          class="inline-flex items-center gap-2 rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-surface)] px-3.5 py-2 text-xs font-semibold text-[var(--text-primary)] hover:bg-[var(--surface-hover)] disabled:opacity-50"
          @click="scanAll"
        >
          <LuRefreshCw class="h-3.5 w-3.5" :class="{ 'animate-spin': scanningAll }" />
          <span>{{ t('libraries.scanAll') }}</span>
        </button>

        <button
          type="button"
          class="inline-flex items-center gap-1.5 rounded-xl bg-[var(--accent)] px-4 py-2 text-xs font-semibold text-[var(--accent-contrast)] hover:opacity-90"
          @click="createOpen = true"
        >
          <LuPlus class="h-4 w-4" />
          <span>{{ t('libraries.addLibrary') }}</span>
        </button>
      </div>
    </div>

    <p v-if="errorMsg" class="rounded-xl bg-red-500/10 px-4 py-2.5 text-xs text-red-500">
      {{ errorMsg }}
    </p>

    <div class="grid grid-cols-1 gap-4 md:grid-cols-2">
      <div
        v-for="lib in libraries"
        :key="lib.id"
        class="flex flex-col justify-between gap-4 rounded-2xl border border-[var(--border-subtle)] bg-[var(--bg-surface)] p-5"
      >
        <div class="space-y-2">
          <div class="flex items-center justify-between gap-2">
            <button
              type="button"
              class="flex items-center gap-2.5 text-left text-base font-bold text-[var(--text-primary)] hover:text-[var(--accent)]"
              @click="router.push({ path: '/comic/series', query: { library_id: lib.id } })"
            >
              <LuFolderOpen class="h-5 w-5 text-[var(--accent)]" />
              <span>{{ lib.name }}</span>
            </button>
            <span class="rounded-full bg-[var(--bg-mantle)] px-2.5 py-0.5 text-[11px] font-medium text-[var(--text-secondary)]">
              {{ lib.kind === 'managed_upload' ? t('libraries.kindManaged') : t('libraries.kindLocal') }}
            </span>
          </div>

          <div class="truncate font-mono text-xs text-[var(--text-secondary)]">
            {{ lib.root_path }}
          </div>

          <div class="text-xs text-[var(--text-secondary)]">
            <span v-if="lib.scan_state.state === 'scanning'" class="text-[var(--accent)]">
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
            <span v-else-if="lib.scan_state.state === 'failed'" class="text-red-500">
              {{ t('libraries.stateFailed', { error: lib.scan_state.error }) }}
            </span>
            <span v-else>{{ t('libraries.stateIdle') }}</span>
          </div>
        </div>

        <div v-if="auth.isAdmin" class="flex items-center justify-end gap-2 border-t border-[var(--border-subtle)] pt-3">
          <button
            type="button"
            :disabled="scanningIds[lib.id]"
            class="inline-flex items-center gap-1.5 rounded-xl bg-[var(--accent)]/12 px-3 py-1.5 text-xs font-semibold text-[var(--accent)] hover:bg-[var(--accent)]/20 disabled:opacity-50"
            @click="scanSingle(lib.id)"
          >
            <LuRefreshCw class="h-3.5 w-3.5" :class="{ 'animate-spin': scanningIds[lib.id] }" />
            <span>{{ scanningIds[lib.id] ? t('libraries.scanning') : t('libraries.scanNow') }}</span>
          </button>

          <button
            v-if="lib.kind !== 'managed_upload'"
            type="button"
            class="rounded-xl p-1.5 text-[var(--text-secondary)] hover:bg-red-500/10 hover:text-red-500"
            @click="removeLibrary(lib)"
          >
            <LuTrash2 class="h-4 w-4" />
          </button>
        </div>
      </div>
    </div>

    <AppDialog
      :open="createOpen"
      :title="t('libraries.addLibrary')"
      max-width-class="max-w-md"
      @close="createOpen = false"
    >
      <div class="space-y-4">
        <div>
          <label class="mb-1 block text-xs font-semibold text-[var(--text-primary)]">
            {{ t('libraries.nameLabel') }}
          </label>
          <input
            v-model="newName"
            type="text"
            :placeholder="t('libraries.namePlaceholder')"
            class="w-full rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-base)] px-3.5 py-2 text-sm text-[var(--text-primary)] focus:border-[var(--accent)] focus:outline-none"
          />
        </div>
        <div>
          <label class="mb-1 block text-xs font-semibold text-[var(--text-primary)]">
            {{ t('libraries.pathLabel') }}
          </label>
          <input
            v-model="newPath"
            type="text"
            :placeholder="t('libraries.pathPlaceholder')"
            class="w-full rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-base)] px-3.5 py-2 font-mono text-sm text-[var(--text-primary)] focus:border-[var(--accent)] focus:outline-none"
          />
        </div>
      </div>
      <template #footer>
        <button
          type="button"
          class="rounded-xl border border-[var(--border-subtle)] px-4 py-2 text-sm text-[var(--text-secondary)]"
          @click="createOpen = false"
        >
          {{ t('common.cancel') }}
        </button>
        <button
          type="button"
          class="rounded-xl bg-[var(--accent)] px-4 py-2 text-sm font-semibold text-[var(--accent-contrast)]"
          @click="createLibrary"
        >
          {{ t('common.save') }}
        </button>
      </template>
    </AppDialog>
  </div>
</template>
