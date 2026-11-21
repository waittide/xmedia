<script setup lang="ts">
import { computed, ref } from 'vue'
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
  LuSearch,
  LuX,
} from 'vue-icons-plus/lu'
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

const filter = ref<QueueFilter>('all')
const query = ref('')
const taskStore = useToolTaskStore()

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
      class="fixed inset-0 z-30 bg-[var(--md-sys-color-scrim)]/32 md:hidden"
      @click="emit('closeMobile')"
    />
  </Transition>

  <aside
    class="fixed top-16 bottom-0 left-0 z-40 flex shrink-0 flex-col bg-[var(--md-sys-color-surface-container)] transition-[width,transform] duration-300 ease-[var(--md-sys-motion-easing-standard)] select-none md:static"
    :class="[
      mobileMenuOpen
        ? 'w-72 translate-x-0 rounded-r-[28px] bg-[var(--md-sys-color-surface-container-low)] shadow-[var(--md-sys-elevation-3)] md:rounded-none md:bg-[var(--md-sys-color-surface-container)] md:shadow-none'
        : '-translate-x-full md:translate-x-0',
      collapsed ? 'md:w-20' : 'md:w-64',
    ]"
  >
    <!-- COMPACT RAIL MODE (80px) -->
    <nav
      v-if="isCompact"
      class="hidden flex-1 flex-col items-center gap-2.5 overflow-y-auto px-2 pt-3 pb-4 md:flex"
    >
      <button
        type="button"
        class="md-state group flex w-full flex-col items-center gap-1 rounded-2xl py-1 text-xs font-medium"
        title="返回媒体列表"
        aria-label="返回媒体列表"
        @click="emit('exit')"
      >
        <span
          class="md-state flex h-8 w-14 items-center justify-center rounded-full bg-[var(--md-sys-color-surface-container-high)] text-[var(--md-sys-color-on-surface)]"
        >
          <LuArrowLeft class="h-[18px] w-[18px]" />
        </span>
        <span class="truncate text-[var(--md-sys-color-on-surface-variant)]">返回</span>
      </button>

      <button
        type="button"
        class="md-state group flex w-full flex-col items-center gap-1 rounded-2xl py-1 text-xs font-medium"
        title="展开刮削队列列表"
        aria-label="展开刮削队列列表"
        @click="emit('toggleCollapse')"
      >
        <span
          class="md-state flex h-8 w-14 items-center justify-center rounded-full bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]"
        >
          <LuList class="h-[18px] w-[18px]" />
        </span>
        <span class="truncate font-semibold text-[var(--md-sys-color-on-surface)]">队列</span>
      </button>

      <div class="my-1 h-px w-8 bg-[var(--md-sys-color-outline-variant)]/60" />

      <button
        type="button"
        :disabled="!hasPrev"
        class="md-icon-btn md-state disabled:opacity-35"
        title="上一项 (Alt+↑)"
        @click="emit('step', -1)"
      >
        <LuArrowUp class="h-[18px] w-[18px]" />
      </button>

      <div class="text-center font-mono text-[11px] font-semibold text-[var(--md-sys-color-on-surface-variant)]">
        <div>{{ activeIndex >= 0 ? activeIndex + 1 : '—' }}</div>
        <div class="text-[9px] opacity-65">/ {{ entries.length }}</div>
      </div>

      <button
        type="button"
        :disabled="!hasNext"
        class="md-icon-btn md-state disabled:opacity-35"
        title="下一项 (Alt+↓)"
        @click="emit('step', 1)"
      >
        <LuArrowDown class="h-[18px] w-[18px]" />
      </button>

      <div
        v-if="counts.saved > 0"
        class="mt-auto rounded-full bg-[var(--md-sys-color-success-container)] px-2 py-0.5 font-mono text-[10px] font-semibold text-[var(--md-sys-color-on-success-container)]"
        :title="`本次已保存 ${counts.saved} / ${counts.all} 项`"
      >
        ✓ {{ counts.saved }}
      </div>
    </nav>

    <!-- EXPANDED QUEUE LIST MODE -->
    <div v-else class="flex min-h-0 flex-1 flex-col overflow-hidden px-3 pt-2.5 pb-3">
      <!-- Top Return & Progress Header -->
      <div class="mb-2.5 flex items-center justify-between gap-2">
        <button
          type="button"
          class="md-btn md-btn-sm md-btn-tonal md-state"
          @click="emit('exit')"
        >
          <LuArrowLeft class="h-3.5 w-3.5" />
          <span>返回列表</span>
        </button>

        <span
          class="rounded-full bg-[var(--md-sys-color-surface-container-high)] px-2 py-0.5 font-mono text-[10px] font-semibold text-[var(--md-sys-color-on-surface-variant)]"
          :title="`本次已保存 ${counts.saved} 项，共 ${counts.all} 项`"
        >
          ✓ {{ counts.saved }} / {{ counts.all }}
        </span>
      </div>

      <!-- Queue Search Input -->
      <div
        class="mb-2 flex h-8 items-center gap-1.5 rounded-full bg-[var(--md-sys-color-surface-container-high)] px-2.5"
      >
        <LuSearch class="h-3.5 w-3.5 shrink-0 text-[var(--md-sys-color-on-surface-variant)]" />
        <input
          v-model="query"
          type="text"
          placeholder="筛选队列条目…"
          class="min-w-0 flex-1 bg-transparent text-xs text-[var(--md-sys-color-on-surface)] outline-none placeholder:text-[var(--md-sys-color-on-surface-variant)]/70"
        />
        <button
          v-if="query"
          type="button"
          class="text-[var(--md-sys-color-on-surface-variant)] hover:text-[var(--md-sys-color-on-surface)]"
          @click="query = ''"
        >
          <LuX class="h-3 w-3" />
        </button>
      </div>

      <!-- Queue Filter Pills -->
      <div class="mb-2 flex flex-wrap gap-1">
        <button
          v-for="tab in [
            { id: 'all' as const, label: '全部', count: counts.all },
            { id: 'incomplete' as const, label: '未完善', count: counts.incomplete },
            { id: 'missing_cover' as const, label: '缺封', count: counts.missingCover },
            { id: 'saved' as const, label: '已存', count: counts.saved },
          ]"
          :key="tab.id"
          type="button"
          class="rounded-full px-2 py-0.5 text-[10px] font-semibold transition"
          :class="
            filter === tab.id
              ? 'bg-[var(--md-sys-color-primary)] text-[var(--md-sys-color-on-primary)]'
              : 'bg-[var(--md-sys-color-surface-container-high)] text-[var(--md-sys-color-on-surface-variant)] hover:text-[var(--md-sys-color-on-surface)]'
          "
          @click="filter = tab.id"
        >
          {{ tab.label }} {{ tab.count }}
        </button>
      </div>

      <!-- Scrollable Queue Entries -->
      <div class="min-h-0 flex-1 space-y-1 overflow-y-auto pr-0.5">
        <div
          v-if="filteredEntries.length === 0"
          class="py-10 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
        >
          无匹配队列条目
        </div>

        <button
          v-for="entry in filteredEntries"
          :key="entryCompositeKey(entry)"
          type="button"
          class="md-state flex w-full items-center gap-2.5 rounded-2xl px-2.5 py-2 text-left transition"
          :class="
            activeKey === entryCompositeKey(entry)
              ? 'bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]'
              : 'text-[var(--md-sys-color-on-surface)] hover:bg-[var(--md-sys-color-surface-container-high)]/70'
          "
          @click="emit('selectEntry', entry)"
        >
          <!-- Mini Thumbnail -->
          <div
            class="relative shrink-0 overflow-hidden rounded-lg bg-[var(--md-sys-color-surface-container-high)]"
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
              class="h-full w-full object-cover"
            />
            <div
              v-else
              class="flex h-full w-full items-center justify-center text-[var(--md-sys-color-on-surface-variant)]/55"
            >
              <LuBookOpen v-if="entry.media_kind === 'book'" class="h-3.5 w-3.5" />
              <LuLayers v-else-if="entry.media_kind === 'comic'" class="h-3.5 w-3.5" />
              <LuDisc3 v-else-if="entry.media_kind === 'music'" class="h-3.5 w-3.5" />
              <LuClapperboard v-else class="h-3.5 w-3.5" />
            </div>
          </div>

          <!-- Title & Status -->
          <div class="min-w-0 flex-1">
            <div class="truncate text-xs font-semibold">
              {{ entry.display_title || entry.file_name }}
            </div>
            <div class="mt-0.5 flex items-center gap-1.5 text-[10px] opacity-75">
              <span class="truncate font-mono">{{ entry.file_name }}</span>
              <span
                v-if="!savedKeys.has(entryCompositeKey(entry)) && entryTaskStatus(entry) === 'running'"
                class="inline-flex shrink-0 items-center gap-0.5 rounded-full bg-[var(--md-sys-color-primary-container)] px-1.5 py-0.25 text-[9px] font-semibold text-[var(--md-sys-color-on-primary-container)] opacity-100"
              >
                <LuLoader2 class="h-2.5 w-2.5 animate-spin" />
                <span>刮削中</span>
              </span>
              <span
                v-else-if="!savedKeys.has(entryCompositeKey(entry)) && entryTaskStatus(entry) === 'unmatched'"
                class="shrink-0 rounded-full bg-[var(--md-sys-color-warning-container)] px-1.5 py-0.25 text-[9px] font-semibold text-[var(--md-sys-color-on-warning-container)] opacity-100"
              >
                未匹配
              </span>
              <span
                v-else-if="!savedKeys.has(entryCompositeKey(entry)) && entryTaskStatus(entry) === 'failed'"
                class="shrink-0 rounded-full bg-[var(--md-sys-color-error-container)] px-1.5 py-0.25 text-[9px] font-semibold text-[var(--md-sys-color-on-error-container)] opacity-100"
              >
                失败
              </span>
            </div>
          </div>

          <!-- Session Saved / Missing Cover Indicator -->
          <div class="shrink-0">
            <LuCheckCircle2
              v-if="savedKeys.has(entryCompositeKey(entry))"
              class="h-3.5 w-3.5 text-[var(--md-sys-color-success)]"
              title="本次会话已保存"
            />
            <span
              v-else-if="!entry.completeness.has_cover"
              class="block h-2 w-2 rounded-full bg-[var(--md-sys-color-warning)]"
              title="缺封面"
            />
            <span
              v-else-if="!isEntryComplete(entry)"
              class="block h-2 w-2 rounded-full bg-[var(--md-sys-color-primary)]/60"
              title="元数据待完善"
            />
          </div>
        </button>
      </div>
    </div>
  </aside>
</template>
