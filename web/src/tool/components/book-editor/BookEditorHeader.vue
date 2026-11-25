<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import {
  LuBookOpen,
  LuCheck,
  LuKeyboard,
  LuRefreshCw,
  LuSave,
  LuWand2,
  LuX,
} from 'vue-icons-plus/lu'
import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import MdButton from '@/components/ui/MdButton.vue'
import MdIconButton from '@/components/ui/MdIconButton.vue'

defineProps<{
  title: string
  docFormat: 'txt' | 'epub'
  chapterCount: number
  totalCharCount: number
  savedBadge: boolean
  hasUnsavedEdits: boolean
  saving: boolean
  converting: boolean
  polishing: boolean
  deleteSourceTxt: boolean
}>()

const emit = defineEmits<{
  'update:deleteSourceTxt': [val: boolean]
  save: []
  convertToEpub: []
  polish: []
  close: []
}>()

const { t } = useI18n()
</script>

<template>
  <header
    class="safe-pt safe-px flex min-h-14 shrink-0 items-center justify-between gap-3 border-b border-outline-variant/50 bg-surface-container-low px-4 py-1.5 select-none"
  >
    <div class="flex min-w-0 items-center gap-3">
      <div
        class="flex h-9 w-9 shrink-0 items-center justify-center rounded-corner-md bg-primary-container text-on-primary-container"
      >
        <LuBookOpen class="h-4 w-4" />
      </div>
      <div class="min-w-0">
        <div class="flex items-center gap-2">
          <h1 class="truncate text-title-small font-semibold text-on-surface">
            {{ title }}
          </h1>
          <span
            class="shrink-0 rounded-corner-xs bg-secondary-container px-1.5 py-0.5 font-mono text-label-small font-semibold uppercase text-on-secondary-container"
          >
            {{ docFormat }}
          </span>
          <span
            v-if="savedBadge"
            class="inline-flex shrink-0 items-center gap-1 rounded-corner-full bg-success-container px-2.5 py-0.5 text-label-small font-semibold text-on-success-container"
          >
            <LuCheck class="h-3.5 w-3.5" />
            <span>{{ t('tool.bookEditor.header.saved') }}</span>
          </span>
          <span
            v-else-if="hasUnsavedEdits"
            class="shrink-0 rounded-corner-full bg-warning-container px-2.5 py-0.5 text-label-small font-semibold text-on-warning-container"
          >
            {{ t('tool.bookEditor.header.unsaved') }}
          </span>
        </div>
        <p class="truncate text-label-small text-on-surface-variant">
          {{
            t('tool.bookEditor.header.stats', {
              chapters: chapterCount,
              chars: totalCharCount.toLocaleString(),
            })
          }}
        </p>
      </div>
    </div>

    <div class="flex shrink-0 items-center gap-2">
      <div class="group relative">
        <MdIconButton
          size="sm"
          :aria-label="t('tool.bookEditor.header.shortcutsAria')"
          :title="t('tool.bookEditor.header.shortcutsAria')"
        >
          <LuKeyboard class="h-4 w-4" />
        </MdIconButton>
        <div
          class="pointer-events-none invisible absolute top-full right-0 z-20 mt-1.5 w-64 rounded-corner-lg bg-surface-container-high p-3.5 text-body-small text-on-surface opacity-0 shadow-elevation-2 transition group-focus-within:visible group-focus-within:opacity-100 group-hover:visible group-hover:opacity-100"
        >
          <p class="mb-2 text-label-large font-semibold text-on-surface">
            {{ t('tool.bookEditor.header.shortcutsHeading') }}
          </p>
          <ul class="space-y-1.5 text-label-medium text-on-surface-variant">
            <li class="flex justify-between gap-2">
              <span>{{ t('tool.bookEditor.header.scSave') }}</span>
              <kbd class="font-mono text-on-surface">Ctrl+S</kbd>
            </li>
            <li class="flex justify-between gap-2">
              <span>{{ t('tool.bookEditor.header.scFind') }}</span>
              <kbd class="font-mono text-on-surface">Ctrl+F</kbd>
            </li>
            <li class="flex justify-between gap-2">
              <span>{{ t('tool.bookEditor.header.scSplit') }}</span>
              <kbd class="font-mono text-on-surface">Ctrl+Shift+Enter</kbd>
            </li>
            <li class="flex justify-between gap-2">
              <span>{{ t('tool.bookEditor.header.scNav') }}</span>
              <kbd class="font-mono text-on-surface">Alt+↑ / ↓</kbd>
            </li>
            <li class="flex justify-between gap-2">
              <span>{{ t('tool.bookEditor.header.scExit') }}</span>
              <kbd class="font-mono text-on-surface">Esc</kbd>
            </li>
          </ul>
        </div>
      </div>

      <template v-if="docFormat === 'txt'">
        <div class="hidden sm:block">
          <AppCheckbox
            :model-value="deleteSourceTxt"
            :label="t('tool.bookEditor.header.deleteTxtAfter')"
            @update:model-value="emit('update:deleteSourceTxt', $event)"
          />
        </div>

        <MdButton
          variant="outlined"
          size="sm"
          :disabled="saving || converting || chapterCount === 0"
          @click="emit('save')"
        >
          <LuSave class="h-4 w-4" />
          <span>{{ saving ? t('tool.actions.saving') : t('tool.bookEditor.header.saveTxt') }}</span>
        </MdButton>

        <MdButton
          variant="filled"
          size="sm"
          :disabled="converting || saving || chapterCount === 0"
          @click="emit('convertToEpub')"
        >
          <LuBookOpen class="h-4 w-4" />
          <span>{{ converting ? t('tool.bookEditor.header.packing') : t('tool.bookEditor.header.convertToEpub') }}</span>
        </MdButton>
      </template>

      <template v-else>
        <MdButton
          variant="tonal"
          size="sm"
          :disabled="polishing || saving || chapterCount === 0"
          @click="emit('polish')"
        >
          <LuRefreshCw v-if="polishing" class="h-4 w-4 animate-spin" />
          <LuWand2 v-else class="h-4 w-4" />
          <span class="hidden sm:inline">
            {{ polishing ? t('tool.bookEditor.header.polishing') : t('tool.bookEditor.header.polish') }}
          </span>
        </MdButton>

        <MdButton
          variant="filled"
          size="sm"
          :disabled="saving || polishing || chapterCount === 0"
          @click="emit('save')"
        >
          <LuSave class="h-4 w-4" />
          <span>{{ saving ? t('tool.actions.saving') : t('tool.bookEditor.header.saveCtrlS') }}</span>
        </MdButton>
      </template>

      <MdIconButton
        size="sm"
        :title="t('tool.bookEditor.header.exitWorkbench')"
        :aria-label="t('tool.bookEditor.header.exitWorkbench')"
        @click="emit('close')"
      >
        <LuX class="h-4 w-4" />
      </MdIconButton>
    </div>
  </header>
</template>
