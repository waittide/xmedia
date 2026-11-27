<script setup lang="ts">
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  LuArrowDown,
  LuArrowLeft,
  LuArrowUp,
  LuBookOpen,
  LuCheckCircle2,
  LuClapperboard,
  LuDisc3,
  LuLayers,
  LuList,
  LuLoader2,
} from 'vue-icons-plus/lu'
import MdButton from '@/components/ui/MdButton.vue'
import MdChip from '@/components/ui/MdChip.vue'
import MdIconButton from '@/components/ui/MdIconButton.vue'
import MdTextField from '@/components/ui/MdTextField.vue'
import { useEscapeStack } from '@/composables/useEscapeStack'
import { useFocusTrap } from '@/composables/useFocusTrap'
import { buildArtworkUrl } from '../../api'
import { entryCompositeKey, isEntryComplete } from '../../completeness'
import { useToolTaskStore, type ScrapeSubItem } from '../../tasks'
import type { MediaFileEntry } from '../../types'

type QueueFilter = 'all' | 'incomplete' | 'missing_cover' | 'saved'

const props = defineProps<{
  mobileMenuOpen: boolean
  collapsed: boolean
  isCompact: boolean
  entries: MediaFileEntry[]
  activeKey: string | null
  savedKeys: Set<string>
  artworkVersion: number
  artworkVersions?: Record<string, number>
  hasPrev: boolean
  hasNext: boolean
}>()

const emit = defineEmits<{
  closeMobile: []
  exit: []
  toggleCollapse: []
  selectEntry: [entry: MediaFileEntry]
  step: [delta: -1 | 1]
}>()

const { t } = useI18n()
const filter = ref<QueueFilter>('all')
const query = ref('')
const taskStore = useToolTaskStore()
const asideRef = ref<HTMLElement | null>(null)
const escapeStack = useEscapeStack()
escapeStack.register(
  () => props.mobileMenuOpen,
  () => emit('closeMobile'),
)
useFocusTrap(asideRef, () => props.mobileMenuOpen)

function entryTaskStatus(e: MediaFileEntry): ScrapeSubItem['status'] | undefined {
  return taskStore.fileStatusMap.get(entryCompositeKey(e))
}

const activeIndex = computed(() => {
  if (!props.activeKey) return -1
  return props.entries.findIndex((e) => entryCompositeKey(e) === props.activeKey)
})

const counts = computed(() => {
  let incomplete = 0
  let missingCover = 0
  let saved = 0
  for (const e of props.entries) {
    if (!isEntryComplete(e)) incomplete += 1
    if (!e.completeness.has_cover) missingCover += 1
    if (props.savedKeys.has(entryCompositeKey(e))) saved += 1
  }
  return {
    all: props.entries.length,
    incomplete,
    missingCover,
    saved,
  }
})

const filterTabs = computed(() => [
  { id: 'all' as const, label: `${t('tool.workbench.queue.tabAll')} ${counts.value.all}` },
  {
    id: 'incomplete' as const,
    label: `${t('tool.workbench.queue.tabIncomplete')} ${counts.value.incomplete}`,
  },
  {
    id: 'missing_cover' as const,
    label: `${t('tool.workbench.queue.tabMissingCover')} ${counts.value.missingCover}`,
  },
  { id: 'saved' as const, label: `${t('tool.workbench.queue.tabSaved')} ${counts.value.saved}` },
])

const filteredEntries = computed(() => {
  const q = query.value.trim().toLowerCase()
  return props.entries.filter((e) => {
    if (filter.value === 'incomplete' && isEntryComplete(e)) return false
    if (filter.value === 'missing_cover' && e.completeness.has_cover) return false
    if (filter.value === 'saved' && !props.savedKeys.has(entryCompositeKey(e))) return false
    if (q) {
      const hay = `${e.display_title} ${e.file_name} ${e.display_subtitle}`.toLowerCase()
      if (!hay.includes(q)) return false
    }
    return true
  })
})
</script>

<template>
  <Transition name="md-fade">
    <div
      v-if="mobileMenuOpen"
      class="fixed inset-0 z-30 bg-scrim/32 sm:hidden"
      @click="emit('closeMobile')"
    />
  </Transition>

  <aside
    ref="asideRef"
    :role="mobileMenuOpen ? 'dialog' : undefined"
    :aria-modal="mobileMenuOpen ? 'true' : undefined"
    :aria-label="t('tool.workbench.queue.ariaLabel')"
    class="fixed top-[calc(4rem+var(--safe-area-top))] bottom-0 left-0 z-40 flex shrink-0 flex-col bg-surface-container pb-[var(--safe-area-bottom)] transition-[width,transform] duration-200 ease-standard select-none sm:static sm:pb-0"
    :class="[
      mobileMenuOpen
        ? 'w-72 translate-x-0 rounded-r-corner-lg bg-surface-container-low shadow-elevation-1 sm:rounded-none sm:bg-surface-container sm:shadow-none'
        : '-translate-x-full sm:translate-x-0',
      collapsed ? 'sm:w-20' : 'sm:w-64',
    ]"
  >
    <!-- COMPACT RAIL MODE (80px) -->
    <nav
      v-if="isCompact"
      class="hidden flex-1 flex-col items-center gap-2.5 overflow-y-auto px-2 pt-3 pb-4 sm:flex"
    >
      <button
        type="button"
        class="md-state group flex w-full cursor-pointer flex-col items-center gap-1 rounded-corner-lg py-1 text-label-medium"
        :title="t('tool.workbench.view.backToMedia')"
        :aria-label="t('tool.workbench.view.backToMedia')"
        @click="emit('exit')"
      >
        <span
          class="md-state flex h-8 w-14 items-center justify-center rounded-corner-full bg-surface-container-high text-on-surface"
        >
          <LuArrowLeft class="h-[18px] w-[18px]" aria-hidden="true" />
        </span>
        <span class="truncate text-on-surface-variant">{{ t('tool.workbench.queue.backShort') }}</span>
      </button>

      <button
        type="button"
        class="md-state group flex w-full cursor-pointer flex-col items-center gap-1 rounded-corner-lg py-1 text-label-medium"
        :title="t('tool.workbench.queue.expandQueue')"
        :aria-label="t('tool.workbench.queue.expandQueue')"
        @click="emit('toggleCollapse')"
      >
        <span
          class="md-state flex h-8 w-14 items-center justify-center rounded-corner-full bg-secondary-container text-on-secondary-container"
        >
          <LuList class="h-[18px] w-[18px]" aria-hidden="true" />
        </span>
        <span class="truncate text-on-surface">{{ t('tool.workbench.queue.queueShort') }}</span>
      </button>

      <div class="my-1 h-px w-8 bg-outline-variant/60" />

      <MdIconButton
        :disabled="!hasPrev"
        :icon="LuArrowUp"
        :tooltip="t('tool.workbench.queue.prevItem')"
        :aria-label="t('tool.workbench.queue.prevItem')"
        @click="emit('step', -1)"
      />

      <div class="text-label-small text-center font-mono text-on-surface-variant">
        <div>{{ activeIndex >= 0 ? activeIndex + 1 : '—' }}</div>
        <div class="opacity-65">/ {{ entries.length }}</div>
      </div>

      <MdIconButton
        :disabled="!hasNext"
        :icon="LuArrowDown"
        :tooltip="t('tool.workbench.queue.nextItem')"
        :aria-label="t('tool.workbench.queue.nextItem')"
        @click="emit('step', 1)"
      />

      <div
        v-if="counts.saved > 0"
        class="text-label-small mt-auto rounded-corner-full bg-success-container px-2 py-0.5 font-mono text-on-success-container"
        :title="t('tool.workbench.queue.savedProgress', { saved: counts.saved, total: counts.all })"
      >
        ✓ {{ counts.saved }}
      </div>
    </nav>

    <!-- EXPANDED QUEUE LIST MODE -->
    <div v-else class="flex min-h-0 flex-1 flex-col overflow-hidden px-3 pt-2.5 pb-3">
      <!-- Top Return & Progress Header -->
      <div class="mb-2.5 flex items-center justify-between gap-2">
        <MdButton variant="tonal" :icon="LuArrowLeft" @click="emit('exit')">
          {{ t('tool.workbench.queue.backToList') }}
        </MdButton>

        <span
          class="text-label-small rounded-corner-full bg-surface-container-high px-2 py-0.5 font-mono text-on-surface-variant"
          :title="t('tool.workbench.queue.savedProgress', { saved: counts.saved, total: counts.all })"
        >
          ✓ {{ counts.saved }} / {{ counts.all }}
        </span>
      </div>

      <!-- Queue Search Input -->
      <div class="mb-2">
        <MdTextField
          v-model="query"
          size="sm"
          type="search"
          clearable
          :placeholder="t('tool.workbench.queue.filterPlaceholder')"
          :aria-label="t('tool.workbench.queue.filterPlaceholder')"
        />
      </div>

      <!-- Queue Filter Chips -->
      <div class="mb-2 flex flex-wrap gap-1">
        <MdChip
          v-for="tab in filterTabs"
          :key="tab.id"
          variant="filter"
          hide-check
          :label="tab.label"
          :selected="filter === tab.id"
          @click="filter = tab.id"
        />
      </div>

      <!-- Scrollable Queue Entries -->
      <div class="min-h-0 flex-1 space-y-1 overflow-y-auto pr-0.5">
        <div
          v-if="filteredEntries.length === 0"
          class="text-body-small py-10 text-center text-on-surface-variant"
        >
          {{ t('tool.workbench.queue.emptyFiltered') }}
        </div>

        <button
          v-for="entry in filteredEntries"
          :key="entryCompositeKey(entry)"
          type="button"
          class="md-state flex w-full cursor-pointer items-center gap-2.5 rounded-corner-lg px-2.5 py-2 text-left transition"
          :class="
            activeKey === entryCompositeKey(entry)
              ? 'bg-secondary-container text-on-secondary-container'
              : 'text-on-surface hover:bg-surface-container-high/70'
          "
          @click="emit('selectEntry', entry)"
        >
          <!-- Mini Thumbnail -->
          <div
            class="relative shrink-0 overflow-hidden rounded-corner-xs bg-surface-container-high"
            :class="entry.media_kind === 'music' ? 'h-9 w-9' : 'h-11 w-8'"
          >
            <img
              v-if="entry.completeness.has_cover"
              :src="
                buildArtworkUrl(
                  entry.root_id,
                  entry.rel_path,
                  entry.media_kind,
                  artworkVersion + (artworkVersions?.[entryCompositeKey(entry)] ?? 0),
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
              class="flex h-full w-full items-center justify-center text-on-surface-variant/55"
            >
              <LuBookOpen v-if="entry.media_kind === 'book'" class="h-3.5 w-3.5" aria-hidden="true" />
              <LuLayers v-else-if="entry.media_kind === 'comic'" class="h-3.5 w-3.5" aria-hidden="true" />
              <LuDisc3 v-else-if="entry.media_kind === 'music'" class="h-3.5 w-3.5" aria-hidden="true" />
              <LuClapperboard v-else class="h-3.5 w-3.5" aria-hidden="true" />
            </div>
          </div>

          <!-- Title & Status -->
          <div class="min-w-0 flex-1">
            <div class="text-label-large truncate">
              {{ entry.display_title || entry.file_name }}
            </div>
            <div class="text-label-small mt-0.5 flex items-center gap-1.5 opacity-75">
              <span class="truncate font-mono">{{ entry.file_name }}</span>
              <span
                v-if="!savedKeys.has(entryCompositeKey(entry)) && entryTaskStatus(entry) === 'running'"
                class="inline-flex shrink-0 items-center gap-0.5 rounded-corner-full bg-primary-container px-1.5 py-0.5 text-on-primary-container opacity-100"
              >
                <LuLoader2 class="h-2.5 w-2.5 animate-spin" aria-hidden="true" />
                <span>{{ t('tool.row.statusScraping') }}</span>
              </span>
              <span
                v-else-if="!savedKeys.has(entryCompositeKey(entry)) && entryTaskStatus(entry) === 'unmatched'"
                class="shrink-0 rounded-corner-full bg-warning-container px-1.5 py-0.5 text-on-warning-container opacity-100"
              >
                {{ t('tool.row.statusUnmatched') }}
              </span>
              <span
                v-else-if="!savedKeys.has(entryCompositeKey(entry)) && entryTaskStatus(entry) === 'failed'"
                class="shrink-0 rounded-corner-full bg-error-container px-1.5 py-0.5 text-on-error-container opacity-100"
              >
                {{ t('tool.row.statusFailed') }}
              </span>
            </div>
          </div>

          <!-- Session Saved / Missing Cover Indicator -->
          <div class="shrink-0">
            <span
              v-if="savedKeys.has(entryCompositeKey(entry))"
              class="inline-flex items-center gap-0.5 text-success"
              :title="t('tool.workbench.queue.sessionSaved')"
              :aria-label="t('tool.workbench.queue.sessionSaved')"
            >
              <LuCheckCircle2 class="h-3.5 w-3.5" aria-hidden="true" />
            </span>
            <span
              v-else-if="!entry.completeness.has_cover"
              class="text-label-small rounded-corner-full bg-warning-container px-1.5 py-0.5 text-on-warning-container"
            >
              {{ t('tool.workbench.queue.badgeMissingCover') }}
            </span>
            <span
              v-else-if="!isEntryComplete(entry)"
              class="text-label-small rounded-corner-full bg-primary-container px-1.5 py-0.5 text-on-primary-container"
            >
              {{ t('tool.workbench.queue.badgeIncomplete') }}
            </span>
          </div>
        </button>
      </div>
    </div>
  </aside>
</template>
