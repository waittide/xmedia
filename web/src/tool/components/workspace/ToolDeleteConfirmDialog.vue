<script setup lang="ts">
import { LuFolder, LuLoader2, LuTrash2 } from 'vue-icons-plus/lu'
import MdDialog from '@/components/ui/MdDialog.vue'

defineProps<{
  open: boolean
  title: string
  subtitle: string
  paths: string[]
  folderCount: number
  deleting: boolean
  error: string | null
  currentFolderMap: Map<string, { name: string; media_file_count: number; child_dir_count: number }>
  parseSelectionKey: (key: string) => { rootId: string; relPath: string }
}>()

const emit = defineEmits<{
  close: []
  confirm: []
}>()
</script>

<template>
  <MdDialog
    :open="open"
    :title="title"
    :subtitle="subtitle"
    max-width="max-w-lg"
    @close="!deleting && emit('close')"
  >
    <template #icon>
      <LuTrash2 class="h-5 w-5 text-[var(--md-sys-color-error)]" />
    </template>

    <div class="space-y-3 py-1">
      <div
        v-if="error"
        class="rounded-2xl bg-[var(--md-sys-color-error-container)] px-3.5 py-2.5 text-xs text-[var(--md-sys-color-on-error-container)]"
      >
        {{ error }}
      </div>

      <p class="text-xs leading-relaxed text-[var(--md-sys-color-on-surface-variant)]">
        即将从磁盘<strong class="text-[var(--md-sys-color-error)]">永久删除</strong>以下
        <strong class="font-mono text-[var(--md-sys-color-on-surface)]">{{ paths.length }}</strong>
        {{ folderCount > 0 ? '项（含文件夹及其内部全部文件）' : '个文件' }}，此操作不可撤销：
      </p>

      <div
        class="max-h-48 space-y-1.5 overflow-y-auto rounded-2xl border border-[var(--md-sys-color-outline-variant)]/50 bg-[var(--md-sys-color-surface-container)]/60 p-2.5 font-mono text-[11px] text-[var(--md-sys-color-on-surface)]"
      >
        <div
          v-for="p in paths.slice(0, 12)"
          :key="p"
          class="flex items-center gap-2"
          :title="parseSelectionKey(p).relPath"
        >
          <LuFolder
            v-if="currentFolderMap.has(p)"
            class="h-3.5 w-3.5 shrink-0 text-[var(--md-sys-color-warning)]"
          />
          <span class="min-w-0 flex-1 truncate">{{ parseSelectionKey(p).relPath }}</span>
          <span
            v-if="currentFolderMap.has(p)"
            class="shrink-0 rounded-full bg-[var(--md-sys-color-error-container)] px-1.5 py-0.5 font-sans text-[10px] font-medium text-[var(--md-sys-color-on-error-container)]"
          >
            文件夹 · {{ currentFolderMap.get(p)!.media_file_count }} 项
          </span>
        </div>
        <div
          v-if="paths.length > 12"
          class="pt-1 text-[10px] text-[var(--md-sys-color-on-surface-variant)]"
        >
          ……以及其他 {{ paths.length - 12 }} 项
        </div>
      </div>
    </div>

    <template #footer>
      <div class="flex items-center justify-end gap-2">
        <button
          type="button"
          :disabled="deleting"
          class="md-btn md-btn-sm md-btn-text md-state"
          @click="emit('close')"
        >
          取消
        </button>
        <button
          type="button"
          :disabled="deleting"
          class="md-btn md-btn-sm md-state bg-[var(--md-sys-color-error)] text-[var(--md-sys-color-on-error)] hover:opacity-95"
          @click="emit('confirm')"
        >
          <LuLoader2 v-if="deleting" class="h-3.5 w-3.5 animate-spin" />
          <LuTrash2 v-else class="h-3.5 w-3.5" />
          <span>{{ deleting ? '正在删除…' : `确认永久删除 (${paths.length})` }}</span>
        </button>
      </div>
    </template>
  </MdDialog>
</template>
