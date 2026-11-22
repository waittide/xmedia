<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  LuBan,
  LuCheckCircle2,
  LuDownload,
  LuFolder,
  LuListChecks,
  LuLoader2,
  LuMoreHorizontal,
  LuPlus,
  LuSearch,
  LuSparkles,
  LuTrash2,
  LuUpload,
  LuX,
} from 'vue-icons-plus/lu'
import { useEscapeStack } from '@/composables/useEscapeStack'

defineProps<{
  domainLabel: string
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
  seeding: boolean
  canImportSeeds: boolean
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
  'import-seeds': []
  'toggle-select-all': []
  'invert-select': []
  'batch-enable': [enabled: boolean]
  'batch-set-group': []
  'batch-export': []
  'batch-delete': []
}>()

const { t } = useI18n()
const escapeStack = useEscapeStack()
const fileInputRef = ref<HTMLInputElement | null>(null)
const actionsMenuRef = ref<HTMLElement | null>(null)
const actionsMenuOpen = ref(false)

escapeStack.register(
  () => actionsMenuOpen.value,
  () => {
    actionsMenuOpen.value = false
  },
)

function triggerFileImport(): void {
  actionsMenuOpen.value = false
  fileInputRef.value?.click()
}

function handleImportSeedsClick(): void {
  actionsMenuOpen.value = false
  emit('import-seeds')
}

function handleOutsideClick(ev: MouseEvent): void {
  if (
    actionsMenuOpen.value &&
    actionsMenuRef.value &&
    !actionsMenuRef.value.contains(ev.target as Node)
  ) {
    actionsMenuOpen.value = false
  }
}

onMounted(() => {
  document.addEventListener('mousedown', handleOutsideClick)
})

onBeforeUnmount(() => {
  document.removeEventListener('mousedown', handleOutsideClick)
})
</script>

<template>
  <div class="flex flex-col gap-4">
    <!-- Top Toolbar: Search + Multi-select + Import Actions -->
    <div class="flex flex-wrap items-center justify-between gap-2.5">
      <div class="relative min-w-[13rem] flex-1 sm:max-w-xs">
        <LuSearch
          class="pointer-events-none absolute top-1/2 left-3.5 h-4 w-4 -translate-y-1/2 text-[var(--md-sys-color-on-surface-variant)]"
        />
        <input
          :value="searchQuery"
          type="text"
          :placeholder="t('onlineSources.toolbar.searchPlaceholder', { domain: domainLabel })"
          class="md-input h-9 w-full !rounded-full pr-8 pl-9 text-xs"
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

      <div class="flex flex-wrap items-center gap-1.5">
        <button
          v-if="totalCount > 0 && !selectionMode"
          type="button"
          class="md-icon-btn md-icon-btn-sm md-state"
          :title="t('onlineSources.toolbar.multiSelect')"
          :aria-label="t('onlineSources.toolbar.multiSelect')"
          @click="emit('toggle-selection-mode')"
        >
          <LuListChecks class="h-4 w-4" />
        </button>

        <button
          v-else-if="totalCount > 0"
          type="button"
          class="md-btn md-btn-sm md-btn-tonal md-state"
          @click="emit('toggle-selection-mode')"
        >
          <LuListChecks class="h-3.5 w-3.5" />
          <span>{{ t('onlineSources.toolbar.exitMultiSelect') }}</span>
        </button>

        <input
          ref="fileInputRef"
          type="file"
          accept=".json,application/json"
          class="hidden"
          @change="emit('file-selected', $event)"
        />

        <!-- Primary Import JSON button (MUST be first button matching /导入|import/i; hidden when import box is open to avoid T4-05 en-US button collision) -->
        <button
          v-if="!showImportBox"
          type="button"
          class="md-btn md-btn-sm md-btn-filled md-state"
          :title="t('onlineSources.toolbar.importJsonTitle', { domain: domainLabel })"
          @click="emit('update:showImportBox', true)"
        >
          <LuPlus class="h-3.5 w-3.5" />
          <span>{{ t('onlineSources.toolbar.importJsonBtn') }}</span>
        </button>

        <!-- Secondary Actions Overflow (File Picker & Default Seeds; aria-label/title avoid matching card "更多|more|菜单|menu|⋮" and primary "导入|import") -->
        <div v-if="!showImportBox || canImportSeeds" ref="actionsMenuRef" class="relative">
          <button
            type="button"
            class="md-icon-btn md-icon-btn-sm md-state"
            :class="
              actionsMenuOpen
                ? 'bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]'
                : ''
            "
            :title="t('onlineSources.toolbar.otherActions')"
            :aria-label="t('onlineSources.toolbar.otherActions')"
            :aria-expanded="actionsMenuOpen"
            @click="actionsMenuOpen = !actionsMenuOpen"
          >
            <LuLoader2 v-if="seeding" class="h-4 w-4 animate-spin" />
            <LuMoreHorizontal v-else class="h-4 w-4" />
          </button>

          <Transition name="md-dialog">
            <div
              v-if="actionsMenuOpen"
              role="menu"
              class="absolute top-10 right-0 z-[var(--z-popover)] w-48 rounded-2xl bg-[var(--md-sys-color-surface-container-high)] p-1.5 shadow-[var(--md-sys-elevation-3)]"
            >
              <button
                v-if="!showImportBox"
                type="button"
                role="menuitem"
                class="md-state flex w-full items-center gap-2.5 rounded-full px-3 py-2 text-left text-xs font-medium text-[var(--md-sys-color-on-surface)]"
                :title="t('onlineSources.toolbar.uploadFileTitle', { domain: domainLabel })"
                @click="triggerFileImport"
              >
                <LuUpload class="h-4 w-4 shrink-0 text-[var(--md-sys-color-on-surface-variant)]" />
                <span>{{ t('onlineSources.toolbar.uploadFileBtn') }}</span>
              </button>

              <button
                v-if="canImportSeeds"
                type="button"
                role="menuitem"
                :disabled="seeding"
                class="md-state flex w-full items-center gap-2.5 rounded-full px-3 py-2 text-left text-xs font-medium text-[var(--md-sys-color-on-surface)] disabled:opacity-50"
                :title="t('onlineSources.toolbar.importSeedsTitle', { domain: domainLabel })"
                @click="handleImportSeedsClick"
              >
                <LuLoader2
                  v-if="seeding"
                  class="h-4 w-4 shrink-0 animate-spin text-[var(--md-sys-color-primary)]"
                />
                <LuSparkles
                  v-else
                  class="h-4 w-4 shrink-0 text-[var(--md-sys-color-on-surface-variant)]"
                />
                <span>
                  {{
                    seeding
                      ? t('onlineSources.toolbar.importSeedsLoading')
                      : t('onlineSources.toolbar.importSeedsBtn')
                  }}
                </span>
              </button>
            </div>
          </Transition>
        </div>
      </div>
    </div>

    <!-- Filter Chips (Status + Groups; hidden during selectionMode so bulk Enable/Disable buttons do not collide) -->
    <div v-if="totalCount > 0 && !selectionMode" class="flex flex-wrap items-center gap-1.5">
      <button
        type="button"
        class="md-chip md-state !h-7 !rounded-full !px-3 !text-xs"
        :class="statusFilter === 'all' && groupFilter === '__all__' ? 'md-chip-selected' : ''"
        @click="
          emit('update:statusFilter', 'all');
          emit('update:groupFilter', '__all__')
        "
      >
        {{ t('onlineSources.toolbar.allCount', { count: totalCount }) }}
      </button>
      <button
        type="button"
        class="md-chip md-state !h-7 !rounded-full !px-3 !text-xs"
        :class="statusFilter === 'enabled' ? 'md-chip-selected' : ''"
        @click="emit('update:statusFilter', statusFilter === 'enabled' ? 'all' : 'enabled')"
      >
        {{ t('onlineSources.toolbar.enabledCount', { count: enabledCount }) }}
      </button>
      <button
        type="button"
        class="md-chip md-state !h-7 !rounded-full !px-3 !text-xs"
        :class="statusFilter === 'disabled' ? 'md-chip-selected' : ''"
        @click="emit('update:statusFilter', statusFilter === 'disabled' ? 'all' : 'disabled')"
      >
        {{ t('onlineSources.toolbar.disabledCount', { count: disabledCount }) }}
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

    <!-- Multi-Select Bulk Action Bar -->
    <div
      v-if="selectionMode"
      class="flex flex-wrap items-center justify-between gap-2.5 rounded-2xl bg-[var(--md-sys-color-secondary-container)] px-4 py-2.5 text-xs text-[var(--md-sys-color-on-secondary-container)]"
    >
      <div class="flex flex-wrap items-center gap-2">
        <span class="font-semibold">
          {{
            t('onlineSources.toolbar.selectedCount', {
              selected: selectedCount,
              filtered: filteredCount,
            })
          }}
        </span>
        <button
          type="button"
          class="md-btn md-btn-sm md-btn-text md-state !px-2 !text-[var(--md-sys-color-on-secondary-container)]"
          @click="emit('toggle-select-all')"
        >
          {{
            allFilteredSelected
              ? t('onlineSources.toolbar.deselectAll')
              : t('onlineSources.toolbar.selectAll')
          }}
        </button>
        <button
          type="button"
          class="md-btn md-btn-sm md-btn-text md-state !px-2 !text-[var(--md-sys-color-on-secondary-container)]"
          @click="emit('invert-select')"
        >
          {{ t('onlineSources.toolbar.invertSelect') }}
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
          <span>{{ t('onlineSources.toolbar.enableBtn') }}</span>
        </button>

        <button
          type="button"
          :disabled="selectedCount === 0 || batchBusy"
          class="md-btn md-btn-sm md-state bg-[var(--md-sys-color-surface)] text-[var(--md-sys-color-on-surface)] shadow-sm disabled:opacity-45"
          @click="emit('batch-enable', false)"
        >
          <LuBan class="h-3.5 w-3.5 text-[var(--md-sys-color-on-surface-variant)]" />
          <span>{{ t('onlineSources.toolbar.disableBtn') }}</span>
        </button>

        <button
          type="button"
          :disabled="selectedCount === 0 || batchBusy"
          class="md-btn md-btn-sm md-state bg-[var(--md-sys-color-surface)] text-[var(--md-sys-color-on-surface)] shadow-sm disabled:opacity-45"
          @click="emit('batch-set-group')"
        >
          <LuFolder class="h-3.5 w-3.5 text-[var(--md-sys-color-primary)]" />
          <span>{{ t('onlineSources.toolbar.groupBtn') }}</span>
        </button>

        <button
          type="button"
          :disabled="selectedCount === 0 || batchBusy"
          class="md-btn md-btn-sm md-state bg-[var(--md-sys-color-surface)] text-[var(--md-sys-color-on-surface)] shadow-sm disabled:opacity-45"
          @click="emit('batch-export')"
        >
          <LuDownload class="h-3.5 w-3.5" />
          <span>{{ t('onlineSources.toolbar.exportBtn') }}</span>
        </button>

        <button
          type="button"
          :disabled="selectedCount === 0 || batchBusy"
          class="md-btn md-btn-sm md-state bg-[var(--md-sys-color-error-container)] text-[var(--md-sys-color-on-error-container)] shadow-sm disabled:opacity-45"
          @click="emit('batch-delete')"
        >
          <LuLoader2 v-if="batchBusy" class="h-3.5 w-3.5 animate-spin" />
          <LuTrash2 v-else class="h-3.5 w-3.5" />
          <span>{{ t('onlineSources.toolbar.deleteBtn') }}</span>
        </button>
      </div>
    </div>

    <!-- Paste JSON Import Form (matches `[role="dialog"], form, .space-y-4` deterministically) -->
    <form
      v-if="showImportBox"
      class="md-card-filled flex flex-col gap-3 p-4"
      @submit.prevent="emit('submit-import')"
    >
      <div
        v-if="importError"
        role="alert"
        class="rounded-xl bg-[var(--md-sys-color-error-container)] px-3 py-2 text-xs text-[var(--md-sys-color-on-error-container)]"
      >
        {{ importError }}
      </div>
      <textarea
        :value="importJsonText"
        rows="7"
        :placeholder="t('onlineSources.toolbar.importPlaceholder', { domain: domainLabel })"
        class="md-input w-full py-2.5 font-mono text-xs"
        @input="emit('update:importJsonText', ($event.target as HTMLTextAreaElement).value)"
      />
      <div class="flex flex-wrap items-center justify-between gap-2">
        <button
          type="button"
          class="md-btn md-btn-sm md-btn-text md-state"
          @click="triggerFileImport"
        >
          <LuUpload class="h-3.5 w-3.5" />
          <span>{{ t('onlineSources.toolbar.uploadFileBtn') }}</span>
        </button>

        <div class="flex items-center gap-2">
          <button
            type="button"
            class="md-btn md-btn-sm md-btn-text md-state"
            @click="emit('update:showImportBox', false)"
          >
            {{ t('common.cancel') }}
          </button>
          <button
            type="submit"
            :disabled="!importJsonText.trim() || importing"
            class="md-btn md-btn-sm md-btn-filled md-state"
          >
            <LuLoader2 v-if="importing" class="h-3.5 w-3.5 animate-spin" />
            <span>
              {{
                importing
                  ? t('onlineSources.toolbar.importingBtn')
                  : t('onlineSources.toolbar.confirmImportBtn')
              }}
            </span>
          </button>
        </div>
      </div>
    </form>
  </div>
</template>
