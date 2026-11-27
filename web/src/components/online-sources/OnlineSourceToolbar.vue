<script setup lang="ts">
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  LuBan,
  LuCheckCircle2,
  LuChevronDown,
  LuClipboardPaste,
  LuDownload,
  LuFileJson,
  LuFolder,
  LuListChecks,
  LuLoader2,
  LuPlus,
  LuSearch,
  LuTrash2,
  LuUpload,
  LuX,
} from 'vue-icons-plus/lu'
import { MdMenu, MdMenuItem } from '@/components/ui'

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

const { t } = useI18n()
const fileInputRef = ref<HTMLInputElement | null>(null)
const importMenuOpen = ref(false)

function openPasteImportBox(): void {
  importMenuOpen.value = false
  emit('update:showImportBox', true)
}

function triggerFileImport(): void {
  importMenuOpen.value = false
  fileInputRef.value?.click()
}
</script>

<template>
  <div class="flex flex-col gap-4">
    <!-- Top Toolbar: Search + Multi-select + Import Actions -->
    <div class="flex flex-wrap items-center justify-between gap-2.5">
      <div class="relative min-w-[13rem] flex-1 sm:max-w-xs">
        <LuSearch
          class="pointer-events-none absolute top-1/2 left-3.5 h-4 w-4 -translate-y-1/2 text-on-surface-variant"
          aria-hidden="true"
        />
        <input
          :value="searchQuery"
          type="text"
          :placeholder="t('onlineSources.toolbar.searchPlaceholder', { domain: domainLabel })"
          :aria-label="t('onlineSources.toolbar.searchPlaceholder', { domain: domainLabel })"
          class="sm:text-body-small h-10 w-full rounded-corner-full bg-surface-container-high pr-9 pl-9 text-base text-on-surface placeholder:text-on-surface-variant caret-primary outline-none transition-shadow duration-150 ease-standard focus:ring-2 focus:ring-primary"
          @input="emit('update:searchQuery', ($event.target as HTMLInputElement).value)"
        />
        <button
          v-if="searchQuery"
          type="button"
          :aria-label="t('common.clear') || t('common.cancel')"
          class="md-state md-touch-target absolute top-1/2 right-1.5 flex h-6 w-6 -translate-y-1/2 cursor-pointer items-center justify-center rounded-corner-full text-on-surface-variant hover:text-on-surface"
          @click="emit('update:searchQuery', '')"
        >
          <LuX class="h-3.5 w-3.5" aria-hidden="true" />
        </button>
      </div>

      <div class="flex flex-wrap items-center gap-1.5">
        <button
          v-if="totalCount > 0 && !selectionMode"
          type="button"
          class="md-icon-btn md-state"
          :title="t('onlineSources.toolbar.multiSelect')"
          :aria-label="t('onlineSources.toolbar.multiSelect')"
          @click="emit('toggle-selection-mode')"
        >
          <LuListChecks class="h-[18px] w-[18px]" />
        </button>

        <button
          v-else-if="totalCount > 0"
          type="button"
          class="md-btn md-btn-tonal md-state"
          @click="emit('toggle-selection-mode')"
        >
          <LuListChecks class="h-4 w-4" />
          <span>{{ t('onlineSources.toolbar.exitMultiSelect') }}</span>
        </button>

        <input
          ref="fileInputRef"
          type="file"
          accept=".json,.js,.mjs,application/json,text/javascript"
          :aria-label="t('onlineSources.toolbar.importJsonFileOption')"
          class="hidden"
          @change="emit('file-selected', $event)"
        />

        <!-- Primary Import dropdown (MUST be first button matching /导入|import/i; hidden when import box is open) -->
        <MdMenu
          v-if="!showImportBox"
          v-model:open="importMenuOpen"
          placement="bottom-end"
          :min-width="192"
          :aria-label="t('onlineSources.toolbar.importJsonBtn')"
        >
          <template #trigger="{ open, toggle, triggerAttrs }">
            <button
              type="button"
              v-bind="triggerAttrs"
              class="md-btn md-btn-filled md-state"
              :title="t('onlineSources.toolbar.importJsonTitle', { domain: domainLabel })"
              @click="toggle"
            >
              <LuPlus class="h-4 w-4" />
              <span>{{ t('onlineSources.toolbar.importJsonBtn') }}</span>
              <LuChevronDown
                class="h-4 w-4 transition-transform duration-150 ease-standard"
                :class="open ? 'rotate-180' : ''"
                aria-hidden="true"
              />
            </button>
          </template>

          <MdMenuItem
            :icon="LuClipboardPaste"
            :label="t('onlineSources.toolbar.pasteJsonOption')"
            :title="t('onlineSources.toolbar.pasteJsonTitle', { domain: domainLabel })"
            @select="openPasteImportBox"
          />

          <MdMenuItem
            :icon="LuFileJson"
            :label="t('onlineSources.toolbar.importJsonFileOption')"
            :title="t('onlineSources.toolbar.uploadFileTitle', { domain: domainLabel })"
            @select="triggerFileImport"
          />
        </MdMenu>
      </div>
    </div>

    <!-- Filter Chips (Status + Groups; hidden during selectionMode so bulk Enable/Disable buttons do not collide) -->
    <div
      v-if="totalCount > 0 && !selectionMode"
      role="group"
      :aria-label="t('onlineSources.toolbar.searchPlaceholder', { domain: domainLabel })"
      class="flex flex-wrap items-center gap-1.5"
    >
      <button
        type="button"
        :aria-pressed="statusFilter === 'all' && groupFilter === '__all__'"
        class="md-chip md-state"
        :class="
          statusFilter === 'all' && groupFilter === '__all__'
            ? 'md-chip-selected'
            : 'border-transparent bg-surface-container-low text-on-surface-variant hover:text-on-surface'
        "
        @click="
          emit('update:statusFilter', 'all');
          emit('update:groupFilter', '__all__')
        "
      >
        {{ t('onlineSources.toolbar.allCount', { count: totalCount }) }}
      </button>
      <button
        type="button"
        :aria-pressed="statusFilter === 'enabled'"
        class="md-chip md-state"
        :class="
          statusFilter === 'enabled'
            ? 'md-chip-selected'
            : 'border-transparent bg-surface-container-low text-on-surface-variant hover:text-on-surface'
        "
        @click="emit('update:statusFilter', statusFilter === 'enabled' ? 'all' : 'enabled')"
      >
        {{ t('onlineSources.toolbar.enabledCount', { count: enabledCount }) }}
      </button>
      <button
        type="button"
        :aria-pressed="statusFilter === 'disabled'"
        class="md-chip md-state"
        :class="
          statusFilter === 'disabled'
            ? 'md-chip-selected'
            : 'border-transparent bg-surface-container-low text-on-surface-variant hover:text-on-surface'
        "
        @click="emit('update:statusFilter', statusFilter === 'disabled' ? 'all' : 'disabled')"
      >
        {{ t('onlineSources.toolbar.disabledCount', { count: disabledCount }) }}
      </button>

      <button
        v-for="grp in availableGroups"
        :key="grp"
        type="button"
        :aria-pressed="groupFilter === grp"
        class="md-chip md-state"
        :class="
          groupFilter === grp
            ? 'md-chip-selected'
            : 'border-transparent bg-surface-container-low text-on-surface-variant hover:text-on-surface'
        "
        @click="emit('update:groupFilter', groupFilter === grp ? '__all__' : grp)"
      >
        {{ grp }}
      </button>
    </div>

    <!-- Multi-Select Bulk Action Bar -->
    <div
      v-if="selectionMode"
      class="text-body-small flex flex-wrap items-center justify-between gap-2.5 rounded-corner-lg bg-secondary-container px-4 py-2.5 text-on-secondary-container"
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
          class="md-btn md-btn-text md-state px-3 text-on-secondary-container"
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
          class="md-btn md-btn-text md-state px-3 text-on-secondary-container"
          @click="emit('invert-select')"
        >
          {{ t('onlineSources.toolbar.invertSelect') }}
        </button>
      </div>

      <div class="flex flex-wrap items-center gap-1.5">
        <button
          type="button"
          :disabled="selectedCount === 0 || batchBusy"
          class="md-btn md-state bg-surface-container-lowest px-4 text-on-surface disabled:opacity-45"
          @click="emit('batch-enable', true)"
        >
          <LuCheckCircle2 class="h-4 w-4 text-success" />
          <span>{{ t('onlineSources.toolbar.enableBtn') }}</span>
        </button>

        <button
          type="button"
          :disabled="selectedCount === 0 || batchBusy"
          class="md-btn md-state bg-surface-container-lowest px-4 text-on-surface disabled:opacity-45"
          @click="emit('batch-enable', false)"
        >
          <LuBan class="h-4 w-4 text-on-surface-variant" />
          <span>{{ t('onlineSources.toolbar.disableBtn') }}</span>
        </button>

        <button
          type="button"
          :disabled="selectedCount === 0 || batchBusy"
          class="md-btn md-state bg-surface-container-lowest px-4 text-on-surface disabled:opacity-45"
          @click="emit('batch-set-group')"
        >
          <LuFolder class="h-4 w-4 text-primary" />
          <span>{{ t('onlineSources.toolbar.groupBtn') }}</span>
        </button>

        <button
          type="button"
          :disabled="selectedCount === 0 || batchBusy"
          class="md-btn md-state bg-surface-container-lowest px-4 text-on-surface disabled:opacity-45"
          @click="emit('batch-export')"
        >
          <LuDownload class="h-4 w-4" />
          <span>{{ t('onlineSources.toolbar.exportBtn') }}</span>
        </button>

        <button
          type="button"
          :disabled="selectedCount === 0 || batchBusy"
          class="md-btn md-state bg-error-container px-4 text-on-error-container disabled:opacity-45"
          @click="emit('batch-delete')"
        >
          <LuLoader2 v-if="batchBusy" class="h-4 w-4 animate-spin" />
          <LuTrash2 v-else class="h-4 w-4" />
          <span>{{ t('onlineSources.toolbar.deleteBtn') }}</span>
        </button>
      </div>
    </div>

    <!-- Paste JSON Import Form (matches `[role="dialog"], form, .space-y-4` deterministically) -->
    <form
      v-if="showImportBox"
      class="flex flex-col gap-3 rounded-corner-lg bg-surface-container-low p-4"
      @submit.prevent="emit('submit-import')"
    >
      <div
        v-if="importError"
        role="alert"
        class="text-body-small rounded-corner-md bg-error-container px-3 py-2 text-on-error-container"
      >
        {{ importError }}
      </div>
      <textarea
        :value="importJsonText"
        rows="7"
        :placeholder="t('onlineSources.toolbar.importPlaceholder', { domain: domainLabel })"
        :aria-label="t('onlineSources.toolbar.importPlaceholder', { domain: domainLabel })"
        class="sm:text-body-small w-full rounded-corner-xs bg-surface-container-highest px-3.5 py-2.5 font-mono text-base text-on-surface placeholder:text-on-surface-variant caret-primary outline-none focus:ring-2 focus:ring-primary"
        @input="emit('update:importJsonText', ($event.target as HTMLTextAreaElement).value)"
      />
      <div class="flex flex-wrap items-center justify-between gap-2">
        <button
          type="button"
          class="md-btn md-btn-text md-state"
          @click="triggerFileImport"
        >
          <LuUpload class="h-4 w-4" />
          <span>{{ t('onlineSources.toolbar.uploadFileBtn') }}</span>
        </button>

        <div class="flex items-center gap-2">
          <button
            type="button"
            class="md-btn md-btn-text md-state"
            @click="emit('update:showImportBox', false)"
          >
            {{ t('common.cancel') }}
          </button>
          <button
            type="submit"
            :disabled="!importJsonText.trim() || importing"
            class="md-btn md-btn-filled md-state"
          >
            <LuLoader2 v-if="importing" class="h-4 w-4 animate-spin" />
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
