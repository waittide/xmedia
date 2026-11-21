<script setup lang="ts">
import { ref } from 'vue'
import {
  LuBan,
  LuCheckCircle2,
  LuDownload,
  LuFolder,
  LuListChecks,
  LuLoader2,
  LuPlus,
  LuSearch,
  LuTrash2,
  LuUpload,
  LuX,
} from 'vue-icons-plus/lu'

defineProps<{
  searchQuery: string
  statusFilter: 'all' | 'enabled' | 'disabled'
  groupFilter: string
  availableGroups: string[]
  totalCount: number
  enabledCount: number
  disabledCount: number
  filteredCount: number
  selectionMode: boolean
  selectedCount: number
  allFilteredSelected: boolean
  canReorder: boolean
  batchBusy: boolean
  showImportBox: boolean
  importJsonText: string
  importing: boolean
  importError: string | null
}>()

const emit = defineEmits<{
  'update:searchQuery': [value: string]
  'update:statusFilter': [value: 'all' | 'enabled' | 'disabled']
  'update:groupFilter': [value: string]
  'update:showImportBox': [value: boolean]
  'update:importJsonText': [value: string]
  'toggle-selection-mode': []
  'file-selected': [ev: Event]
  'submit-import': []
  'toggle-select-all': []
  'invert-select': []
  'batch-enable': [enabled: boolean]
  'batch-set-group': []
  'batch-export': []
  'batch-delete': []
}>()

const fileInputRef = ref<HTMLInputElement | null>(null)

function triggerFileImport(): void {
  fileInputRef.value?.click()
}
</script>

<template>
  <div class="space-y-4">
    <!-- Top Toolbar: Search + Multi-select + Import Actions -->
    <div class="flex flex-wrap items-center justify-between gap-2.5">
      <div class="relative min-w-[13rem] flex-1 sm:max-w-xs">
        <LuSearch
          class="pointer-events-none absolute top-1/2 left-3.5 h-4 w-4 -translate-y-1/2 text-[var(--md-sys-color-on-surface-variant)]"
        />
        <input
          :value="searchQuery"
          type="text"
          placeholder="搜索书源名称、分组或网址…"
          class="md-input w-full py-1.5 pr-8 pl-9 text-xs"
          @input="emit('update:searchQuery', ($event.target as HTMLInputElement).value)"
        />
        <button
          v-if="searchQuery"
          type="button"
          class="absolute top-1/2 right-2 -translate-y-1/2 rounded-full p-0.5 text-[var(--md-sys-color-on-surface-variant)] hover:bg-[var(--md-sys-color-surface-container-highest)]"
          @click="emit('update:searchQuery', '')"
        >
          <LuX class="h-3.5 w-3.5" />
        </button>
      </div>

      <div class="flex flex-wrap items-center gap-2">
        <button
          v-if="totalCount > 0"
          type="button"
          class="md-btn md-btn-sm md-state"
          :class="selectionMode ? 'md-btn-tonal' : 'md-btn-outlined'"
          @click="emit('toggle-selection-mode')"
        >
          <LuListChecks class="h-3.5 w-3.5" />
          <span>{{ selectionMode ? '退出多选' : '多选' }}</span>
        </button>

        <input
          ref="fileInputRef"
          type="file"
          accept=".json,application/json"
          class="hidden"
          @change="emit('file-selected', $event)"
        />
        <button
          type="button"
          class="md-btn md-btn-sm md-btn-tonal md-state"
          title="从本地 JSON 文件导入书源"
          @click="triggerFileImport"
        >
          <LuUpload class="h-3.5 w-3.5" />
          <span>导入</span>
        </button>
        <button
          type="button"
          class="md-btn md-btn-sm md-btn-filled md-state"
          title="粘贴书源 JSON 文本导入"
          @click="emit('update:showImportBox', !showImportBox)"
        >
          <LuPlus class="h-3.5 w-3.5" />
          <span>粘贴</span>
        </button>
      </div>
    </div>

    <!-- Filter Chips (Status + Groups + Reorder Hint) -->
    <div v-if="totalCount > 0" class="flex flex-wrap items-center justify-between gap-2">
      <div class="flex flex-wrap items-center gap-1.5">
        <button
          type="button"
          class="md-chip md-state !h-7 !rounded-full !px-3 !text-xs"
          :class="statusFilter === 'all' && groupFilter === '__all__' ? 'md-chip-selected' : ''"
          @click="
            emit('update:statusFilter', 'all');
            emit('update:groupFilter', '__all__')
          "
        >
          全部 ({{ totalCount }})
        </button>
        <button
          type="button"
          class="md-chip md-state !h-7 !rounded-full !px-3 !text-xs"
          :class="statusFilter === 'enabled' ? 'md-chip-selected' : ''"
          @click="emit('update:statusFilter', statusFilter === 'enabled' ? 'all' : 'enabled')"
        >
          已启用 ({{ enabledCount }})
        </button>
        <button
          type="button"
          class="md-chip md-state !h-7 !rounded-full !px-3 !text-xs"
          :class="statusFilter === 'disabled' ? 'md-chip-selected' : ''"
          @click="emit('update:statusFilter', statusFilter === 'disabled' ? 'all' : 'disabled')"
        >
          已禁用 ({{ disabledCount }})
        </button>

        <span
          v-if="availableGroups.length > 0"
          class="mx-1 h-3.5 w-px bg-[var(--md-sys-color-outline-variant)]"
        />

        <button
          v-for="grp in availableGroups"
          :key="grp"
          type="button"
          class="md-chip md-state !h-7 !rounded-full !px-3 !text-xs"
          :class="groupFilter === grp ? 'md-chip-selected' : ''"
          @click="emit('update:groupFilter', groupFilter === grp ? '__all__' : grp)"
        >
          {{ grp }}
        </button>
      </div>

      <span
        v-if="!selectionMode && totalCount > 1"
        class="text-[11px] text-[var(--md-sys-color-on-surface-variant)]/80"
      >
        {{ canReorder ? '拖动左侧手柄可调整顺序' : '清除筛选后可拖动排序' }}
      </span>
      <span
        v-else-if="selectionMode"
        class="text-[11px] text-[var(--md-sys-color-on-surface-variant)]/80"
      >
        点击条目勾选，按住 Shift 可连续区间选择
      </span>
    </div>

    <!-- Multi-Select Bulk Action Bar -->
    <div
      v-if="selectionMode"
      class="flex flex-wrap items-center justify-between gap-2.5 rounded-2xl bg-[var(--md-sys-color-secondary-container)] px-4 py-2.5 text-xs text-[var(--md-sys-color-on-secondary-container)]"
    >
      <div class="flex flex-wrap items-center gap-2">
        <span class="font-semibold">
          已选 {{ selectedCount }} / {{ filteredCount }} 项
        </span>
        <button
          type="button"
          class="md-btn md-btn-sm md-btn-text md-state !px-2 !text-[var(--md-sys-color-on-secondary-container)]"
          @click="emit('toggle-select-all')"
        >
          {{ allFilteredSelected ? '取消全选' : '全选当前' }}
        </button>
        <button
          type="button"
          class="md-btn md-btn-sm md-btn-text md-state !px-2 !text-[var(--md-sys-color-on-secondary-container)]"
          @click="emit('invert-select')"
        >
          反选
        </button>
      </div>

      <div class="flex flex-wrap items-center gap-1.5">
        <button
          type="button"
          :disabled="selectedCount === 0 || batchBusy"
          class="md-btn md-btn-sm md-state bg-[var(--md-sys-color-surface)] text-[var(--md-sys-color-on-surface)] shadow-sm disabled:opacity-45"
          @click="emit('batch-enable', true)"
        >
          <LuCheckCircle2 class="h-3.5 w-3.5 text-[var(--md-sys-color-success)]" />
          <span>启用</span>
        </button>

        <button
          type="button"
          :disabled="selectedCount === 0 || batchBusy"
          class="md-btn md-btn-sm md-state bg-[var(--md-sys-color-surface)] text-[var(--md-sys-color-on-surface)] shadow-sm disabled:opacity-45"
          @click="emit('batch-enable', false)"
        >
          <LuBan class="h-3.5 w-3.5 text-[var(--md-sys-color-on-surface-variant)]" />
          <span>禁用</span>
        </button>

        <button
          type="button"
          :disabled="selectedCount === 0 || batchBusy"
          class="md-btn md-btn-sm md-state bg-[var(--md-sys-color-surface)] text-[var(--md-sys-color-on-surface)] shadow-sm disabled:opacity-45"
          @click="emit('batch-set-group')"
        >
          <LuFolder class="h-3.5 w-3.5 text-[var(--md-sys-color-primary)]" />
          <span>分组</span>
        </button>

        <button
          type="button"
          :disabled="selectedCount === 0 || batchBusy"
          class="md-btn md-btn-sm md-state bg-[var(--md-sys-color-surface)] text-[var(--md-sys-color-on-surface)] shadow-sm disabled:opacity-45"
          @click="emit('batch-export')"
        >
          <LuDownload class="h-3.5 w-3.5" />
          <span>导出</span>
        </button>

        <button
          type="button"
          :disabled="selectedCount === 0 || batchBusy"
          class="md-btn md-btn-sm md-state bg-[var(--md-sys-color-error-container)] text-[var(--md-sys-color-on-error-container)] shadow-sm disabled:opacity-45"
          @click="emit('batch-delete')"
        >
          <LuLoader2 v-if="batchBusy" class="h-3.5 w-3.5 animate-spin" />
          <LuTrash2 v-else class="h-3.5 w-3.5" />
          <span>删除</span>
        </button>
      </div>
    </div>

    <!-- Paste Import Box -->
    <div v-if="showImportBox" class="md-card-filled space-y-3 p-4">
      <div
        v-if="importError"
        class="rounded-xl bg-[var(--md-sys-color-error-container)] px-3 py-2 text-xs text-[var(--md-sys-color-on-error-container)]"
      >
        {{ importError }}
      </div>
      <textarea
        :value="importJsonText"
        rows="7"
        placeholder="粘贴单条或多条 Legado 书源 JSON（[...] 或 {...}）…"
        class="md-input w-full py-2.5 font-mono text-xs"
        @input="emit('update:importJsonText', ($event.target as HTMLTextAreaElement).value)"
      />
      <div class="flex items-center justify-end gap-2">
        <button
          type="button"
          class="md-btn md-btn-sm md-btn-text md-state"
          @click="emit('update:showImportBox', false)"
        >
          取消
        </button>
        <button
          type="button"
          :disabled="!importJsonText.trim() || importing"
          class="md-btn md-btn-sm md-btn-filled md-state"
          @click="emit('submit-import')"
        >
          <LuLoader2 v-if="importing" class="h-3.5 w-3.5 animate-spin" />
          <span>{{ importing ? '导入中…' : '确认导入' }}</span>
        </button>
      </div>
    </div>
  </div>
</template>
