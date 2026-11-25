<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  LuArrowDown,
  LuArrowLeft,
  LuArrowUp,
  LuBookOpen,
  LuCheckCircle2,
  LuChevronDown,
  LuClapperboard,
  LuDisc3,
  LuLayers,
  LuPanelLeft,
  LuSlidersHorizontal,
  LuX,
} from 'vue-icons-plus/lu'
import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import MdButton from '@/components/ui/MdButton.vue'
import MdIconButton from '@/components/ui/MdIconButton.vue'
import MdMenu from '@/components/ui/MdMenu.vue'
import MdMenuItem from '@/components/ui/MdMenuItem.vue'
import MdSegmentedButton from '@/components/ui/MdSegmentedButton.vue'
import MdTextField from '@/components/ui/MdTextField.vue'
import { buildArtworkUrl } from '../../api'
import { entryCompositeKey, isEntryComplete } from '../../completeness'
import type { MediaFileDetail, MediaFileEntry } from '../../types'

type DropdownFilter = 'all' | 'incomplete' | 'missing_cover' | 'saved'

const props = defineProps<{
  detail: MediaFileDetail | null
  entries: MediaFileEntry[]
  savedKeys: Set<string>
  artworkVersion: number
  artworkVersions?: Record<string, number>
  sidebarCollapsed: boolean
  showCompactQueueDropdown: boolean
  queueDropdownOpen: boolean
  hasPrev: boolean
  hasNext: boolean
  positionLabel: string
  isCurrentDirty: boolean
  autoSearchOnSwitch: boolean
}>()

const emit = defineEmits<{
  exit: []
  toggleSidebar: []
  step: [delta: -1 | 1]
  selectEntry: [entry: MediaFileEntry]
  'update:queueDropdownOpen': [value: boolean]
  'update:autoSearchOnSwitch': [value: boolean]
}>()

const { t } = useI18n()
const dropdownQuery = ref('')
const dropdownFilter = ref<DropdownFilter>('all')

const dropdownFilterOptions = computed(() => [
  { value: 'all' as const, label: t('tool.workbench.queue.tabAll') },
  { value: 'incomplete' as const, label: t('tool.workbench.queue.tabIncomplete') },
  { value: 'missing_cover' as const, label: t('tool.row.missingCover') },
  { value: 'saved' as const, label: t('tool.workbench.queue.tabSaved') },
])

watch(
  () => props.showCompactQueueDropdown,
  (visible) => {
    if (!visible) emit('update:queueDropdownOpen', false)
  },
)

function resolveArtworkVersion(rootId: string, relPath: string): number {
  return props.artworkVersion + (props.artworkVersions?.[`${rootId}::${relPath}`] ?? 0)
}

const dropdownEntries = computed(() => {
  const q = dropdownQuery.value.trim().toLowerCase()
  return props.entries.filter((e) => {
    const key = entryCompositeKey(e)
    if (dropdownFilter.value === 'incomplete' && isEntryComplete(e)) return false
    if (dropdownFilter.value === 'missing_cover' && e.completeness.has_cover) return false
    if (dropdownFilter.value === 'saved' && !props.savedKeys.has(key)) return false
    if (q) {
      const hay = `${e.display_title} ${e.file_name} ${e.display_subtitle}`.toLowerCase()
      if (!hay.includes(q)) return false
    }
    return true
  })
})

const savedSessionCount = computed(() => {
  let n = 0
  for (const e of props.entries) {
    if (props.savedKeys.has(entryCompositeKey(e))) n += 1
  }
  return n
})
</script>

<template>
  <div
    class="relative z-20 flex shrink-0 flex-wrap items-center justify-between gap-2 border-b border-outline-variant/50 bg-surface-container-low px-3.5 py-2"
  >
    <!-- Left: Exit & Sidebar Queue Toggle -->
    <div class="flex shrink-0 items-center gap-1.5">
      <MdButton variant="tonal" size="sm" :icon="LuArrowLeft" @click="emit('exit')">
        {{ t('tool.workbench.topBar.exitScrape') }}
      </MdButton>

      <MdIconButton
        size="sm"
        class="hidden sm:inline-flex"
        :icon="LuPanelLeft"
        :tooltip="
          sidebarCollapsed
            ? t('tool.workbench.topBar.expandQueue')
            : t('tool.workbench.topBar.collapseQueue')
        "
        :aria-label="
          sidebarCollapsed
            ? t('tool.workbench.topBar.expandQueue')
            : t('tool.workbench.topBar.collapseQueue')
        "
        @click="emit('toggleSidebar')"
      />
    </div>

    <!-- Center: Stepper + Current Entry Indicator -->
    <div class="relative flex min-w-0 flex-1 items-center justify-center gap-1.5">
      <MdIconButton
        size="sm"
        :disabled="!hasPrev"
        :icon="LuArrowUp"
        :tooltip="t('tool.workbench.queue.prevItem')"
        :aria-label="t('tool.workbench.queue.prevItem')"
        @click="emit('step', -1)"
      />

      <MdMenu
        v-if="showCompactQueueDropdown"
        :open="queueDropdownOpen"
        placement="bottom-start"
        :min-width="320"
        :max-width="440"
        :aria-label="t('tool.workbench.topBar.quickSwitchLabel', { count: entries.length })"
        @update:open="emit('update:queueDropdownOpen', $event)"
      >
        <template #trigger="{ open, toggle, triggerAttrs }">
          <button
            type="button"
            v-bind="triggerAttrs"
            class="md-state text-label-medium flex h-8 max-w-[min(420px,55vw)] min-w-0 cursor-pointer items-center gap-2 rounded-corner-full bg-surface px-3 text-on-surface transition hover:bg-surface-container-high"
            :title="t('tool.workbench.topBar.quickSwitchTitle')"
            @click="toggle"
          >
            <span
              v-if="positionLabel"
              class="text-label-small shrink-0 rounded-corner-full bg-primary-container px-1.5 py-0.5 font-mono text-on-primary-container"
            >
              {{ positionLabel }}
            </span>
            <span class="min-w-0 flex-1 truncate">
              {{ detail?.entry.file_name || t('tool.workbench.topBar.noSelection') }}
            </span>
            <span
              v-if="isCurrentDirty"
              class="text-label-small shrink-0 rounded-corner-full bg-warning-container px-1.5 py-0.5 text-on-warning-container"
            >
              {{ t('tool.inspector.unsaved') }}
            </span>
            <LuChevronDown
              class="h-3.5 w-3.5 shrink-0 opacity-70 transition"
              :class="open ? 'rotate-180' : ''"
              aria-hidden="true"
            />
          </button>
        </template>

        <template #default="{ close }">
          <div class="flex max-h-[64vh] w-[min(420px,88vw)] flex-col overflow-hidden">
            <div class="shrink-0 space-y-2 border-b border-outline-variant/45 p-2.5">
              <div class="flex items-center justify-between gap-2">
                <span class="text-label-large text-on-surface">
                  {{ t('tool.workbench.topBar.quickSwitchLabel', { count: entries.length }) }}
                </span>
                <MdIconButton
                  size="sm"
                  :icon="LuX"
                  :aria-label="t('tool.workbench.topBar.closeMenu')"
                  @click="close"
                />
              </div>

              <MdTextField
                v-model="dropdownQuery"
                size="sm"
                type="search"
                clearable
                :placeholder="t('tool.workbench.topBar.searchPlaceholder')"
                :aria-label="t('tool.workbench.topBar.searchPlaceholder')"
              />

              <MdSegmentedButton
                v-model="dropdownFilter"
                :options="dropdownFilterOptions"
                size="sm"
              />
            </div>

            <div class="min-h-0 flex-1 space-y-1 overflow-y-auto p-1.5">
              <div
                v-if="dropdownEntries.length === 0"
                class="text-body-small py-8 text-center text-on-surface-variant"
              >
                {{ t('tool.workbench.queue.emptyFiltered') }}
              </div>
              <button
                v-for="entry in dropdownEntries"
                :key="entryCompositeKey(entry)"
                type="button"
                role="menuitem"
                class="md-state flex w-full cursor-pointer items-center gap-2.5 rounded-corner-md px-2.5 py-2 text-left transition"
                :class="
                  detail && entryCompositeKey(detail.entry) === entryCompositeKey(entry)
                    ? 'bg-secondary-container text-on-secondary-container'
                    : 'text-on-surface hover:bg-surface-container-high'
                "
                @click="close(); emit('selectEntry', entry)"
              >
                <div
                  class="shrink-0 overflow-hidden rounded-corner-xs bg-surface-container-high"
                  :class="entry.media_kind === 'music' ? 'h-9 w-9' : 'h-10 w-8'"
                >
                  <img
                    v-if="entry.completeness.has_cover"
                    :src="
                      buildArtworkUrl(
                        entry.root_id,
                        entry.rel_path,
                        entry.media_kind,
                        resolveArtworkVersion(entry.root_id, entry.rel_path),
                        entry.media_kind === 'video' ? 'poster' : 'cover',
                      )
                    "
                    alt=""
                    loading="lazy"
                    referrerpolicy="no-referrer"
                    class="h-full w-full object-cover"
                  />
                  <div
                    v-else
                    class="flex h-full w-full items-center justify-center text-on-surface-variant/50"
                  >
                    <LuBookOpen v-if="entry.media_kind === 'book'" class="h-3.5 w-3.5" aria-hidden="true" />
                    <LuLayers v-else-if="entry.media_kind === 'comic'" class="h-3.5 w-3.5" aria-hidden="true" />
                    <LuDisc3 v-else-if="entry.media_kind === 'music'" class="h-3.5 w-3.5" aria-hidden="true" />
                    <LuClapperboard v-else class="h-3.5 w-3.5" aria-hidden="true" />
                  </div>
                </div>
                <div class="min-w-0 flex-1">
                  <div class="text-label-large truncate">
                    {{ entry.display_title || entry.file_name }}
                  </div>
                  <div class="text-label-small truncate font-mono opacity-70">
                    {{ entry.file_name }}
                  </div>
                </div>
                <span
                  v-if="savedKeys.has(entryCompositeKey(entry))"
                  class="inline-flex shrink-0 items-center text-success"
                  :aria-label="t('tool.workbench.queue.sessionSaved')"
                >
                  <LuCheckCircle2 class="h-3.5 w-3.5" aria-hidden="true" />
                </span>
                <span
                  v-else-if="!entry.completeness.has_cover"
                  class="text-label-small shrink-0 rounded-corner-full bg-warning-container px-1.5 py-0.5 text-on-warning-container"
                >
                  {{ t('tool.workbench.queue.badgeMissingCover') }}
                </span>
              </button>
            </div>
          </div>
        </template>
      </MdMenu>

      <div
        v-else
        class="text-label-medium flex h-8 max-w-[min(440px,48vw)] min-w-0 items-center gap-2 rounded-corner-full bg-surface-container px-3 text-on-surface"
      >
        <span
          v-if="positionLabel"
          class="text-label-small shrink-0 rounded-corner-full bg-primary-container px-1.5 py-0.5 font-mono text-on-primary-container"
        >
          {{ positionLabel }}
        </span>
        <span class="min-w-0 flex-1 truncate">
          {{ detail?.entry.file_name || t('tool.workbench.topBar.noSelection') }}
        </span>
        <span
          v-if="isCurrentDirty"
          class="text-label-small shrink-0 rounded-corner-full bg-warning-container px-1.5 py-0.5 text-on-warning-container"
        >
          {{ t('tool.inspector.unsaved') }}
        </span>
      </div>

      <MdIconButton
        size="sm"
        :disabled="!hasNext"
        :icon="LuArrowDown"
        :tooltip="t('tool.workbench.queue.nextItem')"
        :aria-label="t('tool.workbench.queue.nextItem')"
        @click="emit('step', 1)"
      />
    </div>

    <!-- Right: Session Saved Counter & Strategy Menu -->
    <div class="flex shrink-0 items-center gap-2">
      <span
        class="text-label-small hidden rounded-corner-full bg-surface-container-high px-2.5 py-1 font-mono text-on-surface-variant sm:inline-flex"
      >
        {{ t('tool.workbench.topBar.savedCounter', { saved: savedSessionCount, total: entries.length }) }}
      </span>

      <div class="hidden xl:inline-flex">
        <AppCheckbox
          :model-value="autoSearchOnSwitch"
          :label="t('tool.workbench.topBar.autoSearchShort')"
          @update:model-value="emit('update:autoSearchOnSwitch', $event)"
        />
      </div>

      <MdMenu
        placement="bottom-end"
        :min-width="220"
        :aria-label="t('tool.workbench.topBar.optionsMenu')"
        class="xl:hidden"
      >
        <template #trigger="{ toggle, triggerAttrs }">
          <MdIconButton
            size="sm"
            :icon="LuSlidersHorizontal"
            :aria-label="t('tool.workbench.topBar.optionsMenu')"
            :tooltip="t('tool.workbench.topBar.optionsMenu')"
            v-bind="triggerAttrs"
            @click="toggle"
          />
        </template>
        <MdMenuItem
          :label="t('tool.workbench.topBar.autoSearchFull')"
          :selected="autoSearchOnSwitch"
          @select="emit('update:autoSearchOnSwitch', !autoSearchOnSwitch)"
        />
      </MdMenu>
    </div>
  </div>
</template>
