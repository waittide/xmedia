<script setup lang="ts">
import { computed, ref, watch } from 'vue'
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
  LuSearch,
  LuSlidersHorizontal,
  LuX,
} from 'vue-icons-plus/lu'
import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import MdIconButton from '@/components/ui/MdIconButton.vue'
import MdMenu from '@/components/ui/MdMenu.vue'
import MdMenuItem from '@/components/ui/MdMenuItem.vue'
import MdSegmentedButton from '@/components/ui/MdSegmentedButton.vue'
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

const dropdownQuery = ref('')
const dropdownFilter = ref<DropdownFilter>('all')

const dropdownFilterOptions = [
  { value: 'all' as const, label: '全部' },
  { value: 'incomplete' as const, label: '未完善' },
  { value: 'missing_cover' as const, label: '缺封面' },
  { value: 'saved' as const, label: '已存' },
]

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
    class="relative z-20 flex shrink-0 flex-wrap items-center justify-between gap-2 border-b border-[var(--md-sys-color-outline-variant)]/50 bg-[var(--md-sys-color-surface-container-low)] px-3.5 py-2"
  >
    <!-- Left: Exit & Sidebar Queue Toggle -->
    <div class="flex shrink-0 items-center gap-1.5">
      <button
        type="button"
        class="md-btn md-btn-sm md-btn-tonal md-state"
        @click="emit('exit')"
      >
        <LuArrowLeft class="h-3.5 w-3.5" />
        <span>退出刮削</span>
      </button>

      <button
        type="button"
        class="md-icon-btn md-icon-btn-sm md-state hidden sm:inline-flex"
        :title="sidebarCollapsed ? '展开左侧队列列表' : '收起左侧队列列表（最大化三栏宽度）'"
        :aria-label="sidebarCollapsed ? '展开左侧队列列表' : '收起左侧队列列表'"
        @click="emit('toggleSidebar')"
      >
        <LuPanelLeft class="h-4 w-4" />
      </button>
    </div>

    <!-- Center: Stepper + Current Entry Indicator -->
    <div class="relative flex min-w-0 flex-1 items-center justify-center gap-1.5">
      <button
        type="button"
        :disabled="!hasPrev"
        class="md-icon-btn md-icon-btn-sm md-state !h-8 !w-8 shrink-0 disabled:opacity-35"
        title="上一项 (Alt+↑)"
        aria-label="上一项"
        @click="emit('step', -1)"
      >
        <LuArrowUp class="h-4 w-4" />
      </button>

      <MdMenu
        v-if="showCompactQueueDropdown"
        :open="queueDropdownOpen"
        placement="bottom-start"
        :min-width="320"
        :max-width="440"
        aria-label="快速切换队列条目"
        @update:open="emit('update:queueDropdownOpen', $event)"
      >
        <template #trigger="{ open, toggle, triggerAttrs }">
          <button
            type="button"
            v-bind="triggerAttrs"
            class="md-state flex h-8 max-w-[min(420px,55vw)] min-w-0 items-center gap-2 rounded-corner-full border border-outline-variant/70 bg-surface px-3 text-label-medium text-on-surface transition hover:border-primary"
            title="点击展开队列并快速跳转"
            @click="toggle"
          >
            <span
              v-if="positionLabel"
              class="shrink-0 rounded-corner-full bg-primary-container px-1.5 py-0.5 font-mono text-label-small text-on-primary-container"
            >
              {{ positionLabel }}
            </span>
            <span class="min-w-0 flex-1 truncate">
              {{ detail?.entry.file_name || '未选择条目' }}
            </span>
            <span
              v-if="isCurrentDirty"
              class="shrink-0 rounded-corner-full bg-warning-container px-1.5 py-0.5 text-label-small text-on-warning-container"
            >
              未保存
            </span>
            <LuChevronDown
              class="h-3.5 w-3.5 shrink-0 opacity-70 transition"
              :class="open ? 'rotate-180' : ''"
            />
          </button>
        </template>

        <template #default="{ close }">
          <div class="flex max-h-[64vh] w-[min(420px,88vw)] flex-col overflow-hidden">
            <div class="shrink-0 space-y-2 border-b border-outline-variant/45 p-2.5">
              <div class="flex items-center justify-between gap-2">
                <span class="text-label-large text-on-surface">
                  快速切换队列条目（{{ entries.length }} 项）
                </span>
                <MdIconButton
                  size="sm"
                  :icon="LuX"
                  aria-label="关闭队列切换菜单"
                  @click="close"
                />
              </div>

              <div class="flex h-8 items-center gap-1.5 rounded-corner-full bg-surface px-2.5">
                <LuSearch class="h-3.5 w-3.5 shrink-0 text-on-surface-variant" />
                <input
                  v-model="dropdownQuery"
                  type="text"
                  aria-label="搜索队列条目"
                  placeholder="搜索标题或文件名…"
                  class="min-w-0 flex-1 bg-transparent text-body-small text-on-surface outline-none"
                />
              </div>

              <MdSegmentedButton
                v-model="dropdownFilter"
                :options="dropdownFilterOptions"
                size="sm"
              />
            </div>

            <div class="min-h-0 flex-1 space-y-1 overflow-y-auto p-1.5">
              <div
                v-if="dropdownEntries.length === 0"
                class="py-8 text-center text-body-small text-on-surface-variant"
              >
                无匹配条目
              </div>
              <button
                v-for="entry in dropdownEntries"
                :key="entryCompositeKey(entry)"
                type="button"
                role="menuitem"
                class="md-state flex w-full items-center gap-2.5 rounded-corner-md px-2.5 py-2 text-left transition"
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
                    <LuBookOpen v-if="entry.media_kind === 'book'" class="h-3.5 w-3.5" />
                    <LuLayers v-else-if="entry.media_kind === 'comic'" class="h-3.5 w-3.5" />
                    <LuDisc3 v-else-if="entry.media_kind === 'music'" class="h-3.5 w-3.5" />
                    <LuClapperboard v-else class="h-3.5 w-3.5" />
                  </div>
                </div>
                <div class="min-w-0 flex-1">
                  <div class="truncate text-label-large">
                    {{ entry.display_title || entry.file_name }}
                  </div>
                  <div class="truncate font-mono text-label-small opacity-70">
                    {{ entry.file_name }}
                  </div>
                </div>
                <span
                  v-if="savedKeys.has(entryCompositeKey(entry))"
                  class="inline-flex shrink-0 items-center text-success"
                  aria-label="本次会话已保存"
                >
                  <LuCheckCircle2 class="h-3.5 w-3.5" aria-hidden="true" />
                </span>
                <span
                  v-else-if="!entry.completeness.has_cover"
                  class="shrink-0 rounded-corner-full bg-warning-container px-1.5 py-0.5 text-label-small text-on-warning-container"
                >
                  缺封
                </span>
              </button>
            </div>
          </div>
        </template>
      </MdMenu>

      <div
        v-else
        class="flex h-8 max-w-[min(440px,48vw)] min-w-0 items-center gap-2 rounded-corner-full bg-surface-container px-3 text-label-medium text-on-surface"
      >
        <span
          v-if="positionLabel"
          class="shrink-0 rounded-corner-full bg-primary-container px-1.5 py-0.5 font-mono text-label-small text-on-primary-container"
        >
          {{ positionLabel }}
        </span>
        <span class="min-w-0 flex-1 truncate">
          {{ detail?.entry.file_name || '未选择条目' }}
        </span>
        <span
          v-if="isCurrentDirty"
          class="shrink-0 rounded-corner-full bg-warning-container px-1.5 py-0.5 text-label-small text-on-warning-container"
        >
          未保存
        </span>
      </div>

      <button
        type="button"
        :disabled="!hasNext"
        class="md-icon-btn md-icon-btn-sm md-state shrink-0 disabled:opacity-35"
        title="下一项 (Alt+↓)"
        aria-label="下一项"
        @click="emit('step', 1)"
      >
        <LuArrowDown class="h-4 w-4" />
      </button>
    </div>

    <!-- Right: Session Saved Counter & Strategy Menu -->
    <div class="flex shrink-0 items-center gap-2">
      <span
        class="hidden rounded-corner-full bg-surface-container-high px-2.5 py-1 font-mono text-label-small text-on-surface-variant sm:inline-flex"
      >
        已存 {{ savedSessionCount }} / {{ entries.length }}
      </span>

      <div class="hidden xl:inline-flex">
        <AppCheckbox
          :model-value="autoSearchOnSwitch"
          label="切项自动搜"
          @update:model-value="emit('update:autoSearchOnSwitch', $event)"
        />
      </div>

      <MdMenu placement="bottom-end" :min-width="210" aria-label="工作台选项" class="xl:hidden">
        <template #trigger="{ toggle, triggerAttrs }">
          <MdIconButton
            size="sm"
            :icon="LuSlidersHorizontal"
            aria-label="工作台选项"
            tooltip="工作台选项"
            v-bind="triggerAttrs"
            @click="toggle"
          />
        </template>
        <MdMenuItem
          label="切换条目时自动搜索"
          :selected="autoSearchOnSwitch"
          @select="emit('update:autoSearchOnSwitch', !autoSearchOnSwitch)"
        />
      </MdMenu>
    </div>
  </div>
</template>
