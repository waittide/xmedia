<script setup lang="ts">
import { computed } from 'vue'
import {
  LuAlertCircle,
  LuArrowRight,
  LuBookOpen,
  LuCheckCircle2,
  LuClapperboard,
  LuDisc3,
  LuFileSignature,
  LuFileText,
  LuFolderPlus,
  LuImage,
  LuLayers,
  LuSparkles,
} from 'vue-icons-plus/lu'
import RefreshButton from '@/components/ui/RefreshButton.vue'
import type { MediaKind } from '@/types/api'
import { buildArtworkUrl } from '../api'
import { formatBytes, useToolText } from '../i18n'
import type {
  DirectorySnapshot,
  ManagedRootDto,
  MediaFileEntry,
} from '../types'

type CompletenessFilter =
  | 'all'
  | 'complete'
  | 'incomplete'
  | 'missing_cover'
  | 'missing_summary'
  | 'missing_tag'

const props = defineProps<{
  activeDomain: MediaKind
  domainRoots: ManagedRootDto[]
  activeRootId: string
  snapshot: DirectorySnapshot | null
  loadingDir: boolean
  artworkVersion: number
  stats: {
    total: number
    complete: number
    incomplete: number
    missingCover: number
    missingSummary: number
    missingTag: number
  }
}>()

const emit = defineEmits<{
  openItems: [filter?: CompletenessFilter]
  inspectFile: [file: MediaFileEntry]
  scrapeFiles: [files: MediaFileEntry[], labelHint?: string]
  openRenameAll: []
  openSettings: [tab: 'appearance' | 'roots' | 'scrapers' | 'templates']
  refreshDir: []
}>()

const txt = useToolText()
const domainLabels = computed(() => txt.value.domainLabels[props.activeDomain])

function isEntryComplete(e: MediaFileEntry): boolean {
  const c = e.completeness
  return (
    c.has_title &&
    c.has_creator &&
    c.has_cover &&
    c.has_summary_or_lyrics &&
    c.has_nfo_or_tag_block
  )
}

const scopeSubtitle = computed(() => {
  if (!props.activeRootId && props.domainRoots.length > 1) {
    return `已汇总 ${props.domainRoots.length} 个挂载目录`
  }
  const name = props.snapshot?.root_name || props.domainRoots[0]?.name || '—'
  const rel = props.snapshot?.current_rel_path
  return `当前挂载目录：${name}${rel ? ` / ${rel}` : ''}`
})

const completionPct = computed(() => {
  if (props.stats.total <= 0) return 0
  return Math.min(100, Math.round((props.stats.complete / props.stats.total) * 100))
})

const incompleteFiles = computed<MediaFileEntry[]>(() =>
  (props.snapshot?.files ?? []).filter((f) => !isEntryComplete(f)),
)

const previewIncomplete = computed<MediaFileEntry[]>(() => incompleteFiles.value.slice(0, 8))
</script>

<template>
  <div class="mx-auto max-w-6xl space-y-6 p-4 sm:p-6">
    <!-- Empty State: No Mounted Directories Configured -->
    <div
      v-if="domainRoots.length === 0"
      class="mx-auto mt-10 max-w-lg rounded-[28px] border border-dashed border-[var(--md-sys-color-outline-variant)] bg-[var(--md-sys-color-surface-container-low)] p-8 text-center"
    >
      <div
        class="mx-auto mb-4 flex h-14 w-14 items-center justify-center rounded-2xl bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)]"
      >
        <LuFolderPlus class="h-6 w-6" />
      </div>
      <h2 class="text-base font-semibold text-[var(--md-sys-color-on-surface)]">
        {{ domainLabels.emptyRootsTitle }}
      </h2>
      <p class="mt-2 text-xs leading-relaxed text-[var(--md-sys-color-on-surface-variant)]">
        {{ domainLabels.emptyRootsDesc }}
      </p>
      <button
        type="button"
        class="md-btn md-btn-filled md-state mt-6"
        @click="emit('openSettings', 'roots')"
      >
        <LuFolderPlus class="h-4 w-4" />
        <span>{{ txt.roots.add }}</span>
      </button>
    </div>

    <template v-else>
      <!-- Top Header & Actions -->
      <div class="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h1 class="text-xl font-semibold tracking-tight text-[var(--md-sys-color-on-surface)]">
            {{ txt.domains[activeDomain] }}概览
          </h1>
          <p class="mt-0.5 text-xs text-[var(--md-sys-color-on-surface-variant)]">
            {{ scopeSubtitle }}
          </p>
        </div>

        <div class="flex flex-wrap items-center gap-2">
          <RefreshButton size="md" :action="() => emit('refreshDir')" :busy="loadingDir" />

          <button
            v-if="incompleteFiles.length > 0"
            type="button"
            class="md-btn md-btn-sm md-btn-filled md-state"
            title="按「本地已有信息 → 高优先级源 → 低优先级源」顺序自动添加后台刮削任务"
            @click="emit('scrapeFiles', incompleteFiles, '待补齐项')"
          >
            <LuSparkles class="h-3.5 w-3.5" />
            <span>刮削待补齐项 ({{ incompleteFiles.length }})</span>
          </button>

          <button
            v-if="stats.total > 0"
            type="button"
            class="md-btn md-btn-sm md-btn-tonal md-state"
            @click="emit('openRenameAll')"
          >
            <LuFileSignature class="h-3.5 w-3.5" />
            <span>{{ txt.actions.batchRename }}</span>
          </button>
        </div>
      </div>

      <!-- 4 MD3 Tonal Metric Cards -->
      <div class="grid grid-cols-2 gap-3 lg:grid-cols-4">
        <button
          type="button"
          class="md-card-filled md-state flex items-center justify-between p-4 text-left"
          @click="emit('openItems', 'all')"
        >
          <div>
            <div class="text-xs text-[var(--md-sys-color-on-surface-variant)]">{{ txt.cards.totalFiles }}</div>
            <div class="mt-1 text-2xl font-semibold tracking-tight">{{ stats.total }}</div>
          </div>
          <div
            class="flex h-11 w-11 items-center justify-center rounded-2xl bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)]"
          >
            <LuBookOpen v-if="activeDomain === 'book'" class="h-5 w-5" />
            <LuLayers v-else-if="activeDomain === 'comic'" class="h-5 w-5" />
            <LuDisc3 v-else-if="activeDomain === 'music'" class="h-5 w-5" />
            <LuClapperboard v-else class="h-5 w-5" />
          </div>
        </button>

        <button
          type="button"
          class="md-card-filled md-state flex items-center justify-between p-4 text-left"
          @click="emit('openItems', 'complete')"
        >
          <div>
            <div class="text-xs text-[var(--md-sys-color-on-surface-variant)]">{{ txt.cards.completeFiles }}</div>
            <div class="mt-1 flex items-baseline gap-1.5">
              <span class="text-2xl font-semibold tracking-tight text-[var(--md-sys-color-success)]">
                {{ stats.complete }}
              </span>
              <span class="font-mono text-xs text-[var(--md-sys-color-on-surface-variant)]">{{ completionPct }}%</span>
            </div>
          </div>
          <div
            class="flex h-11 w-11 items-center justify-center rounded-2xl bg-[var(--md-sys-color-success-container)] text-[var(--md-sys-color-on-success-container)]"
          >
            <LuCheckCircle2 class="h-5 w-5" />
          </div>
        </button>

        <button
          type="button"
          class="md-card-filled md-state flex items-center justify-between p-4 text-left"
          @click="emit('openItems', 'missing_cover')"
        >
          <div>
            <div class="text-xs text-[var(--md-sys-color-on-surface-variant)]">{{ domainLabels.missingCover }}</div>
            <div class="mt-1 text-2xl font-semibold tracking-tight text-[var(--md-sys-color-warning)]">
              {{ stats.missingCover }}
            </div>
          </div>
          <div
            class="flex h-11 w-11 items-center justify-center rounded-2xl bg-[var(--md-sys-color-warning-container)] text-[var(--md-sys-color-on-warning-container)]"
          >
            <LuImage class="h-5 w-5" />
          </div>
        </button>

        <button
          type="button"
          class="md-card-filled md-state flex items-center justify-between p-4 text-left"
          @click="emit('openItems', 'missing_summary')"
        >
          <div>
            <div class="text-xs text-[var(--md-sys-color-on-surface-variant)]">{{ domainLabels.missingSummary }}</div>
            <div class="mt-1 text-2xl font-semibold tracking-tight text-[var(--md-sys-color-primary)]">
              {{ stats.missingSummary }}
            </div>
          </div>
          <div
            class="flex h-11 w-11 items-center justify-center rounded-2xl bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]"
          >
            <LuFileText class="h-5 w-5" />
          </div>
        </button>
      </div>

      <!-- Metadata Completeness Progress Card -->
      <section class="md-card-filled space-y-2.5 p-5">
        <div class="flex flex-wrap items-center justify-between gap-2">
          <div class="flex items-center gap-2">
            <span class="text-xs font-semibold text-[var(--md-sys-color-on-surface)]">元信息完整率</span>
            <span
              class="rounded-full bg-[var(--md-sys-color-primary-container)] px-2.5 py-0.5 font-mono text-[11px] font-semibold text-[var(--md-sys-color-on-primary-container)]"
            >
              {{ completionPct }}%
            </span>
          </div>
          <span class="text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
            齐全 {{ stats.complete }} 项 · 待补齐 {{ stats.incomplete }} 项 · 缺标签/NFO
            {{ stats.missingTag }} 项
          </span>
        </div>
        <div class="h-2 w-full overflow-hidden rounded-full bg-[var(--md-sys-color-surface-container-highest)]">
          <div
            class="h-full rounded-full bg-[var(--md-sys-color-success)] transition-all duration-300"
            :style="{ width: `${completionPct}%` }"
          />
        </div>
      </section>

      <!-- Pending / Incomplete Items Section -->
      <section class="space-y-3">
        <div class="flex items-center justify-between">
          <div class="flex items-center gap-2">
            <h2 class="text-sm font-semibold text-[var(--md-sys-color-on-surface)]">
              {{ incompleteFiles.length > 0 ? '待补齐元信息条目' : '全部条目元信息已齐全' }}
            </h2>
            <span
              v-if="incompleteFiles.length > 0"
              class="rounded-full bg-[var(--md-sys-color-warning-container)] px-2 py-0.5 font-mono text-[11px] font-semibold text-[var(--md-sys-color-on-warning-container)]"
            >
              {{ incompleteFiles.length }}
            </span>
          </div>
          <button
            type="button"
            class="md-btn md-btn-sm md-btn-text md-state"
            @click="emit('openItems', incompleteFiles.length > 0 ? 'incomplete' : 'all')"
          >
            <span>进入条目列表</span>
            <LuArrowRight class="h-3.5 w-3.5" />
          </button>
        </div>

        <div
          v-if="previewIncomplete.length === 0"
          class="rounded-[24px] bg-[var(--md-sys-color-surface-container-low)] py-10 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
        >
          {{
            stats.total === 0
              ? '当前挂载目录下暂无受支持的媒体文件'
              : '所有媒体文件的封面、简介与标签均已完备'
          }}
        </div>

        <div
          v-else
          class="divide-y divide-[var(--md-sys-color-outline-variant)]/35 overflow-hidden rounded-[20px] bg-[var(--md-sys-color-surface-container-low)]"
        >
          <div
            v-for="file in previewIncomplete"
            :key="`${file.root_id}::${file.rel_path}`"
            class="md-state flex items-center gap-3 px-4 py-2.5"
          >
            <button
              type="button"
              class="flex min-w-0 flex-1 items-center gap-3 text-left"
              @click="emit('inspectFile', file)"
            >
              <div
                class="relative flex h-11 w-11 shrink-0 items-center justify-center overflow-hidden rounded-xl bg-[var(--md-sys-color-surface-container-high)]"
              >
                <img
                  v-if="file.completeness.has_cover"
                  :src="buildArtworkUrl(file.root_id, file.rel_path, file.media_kind, artworkVersion)"
                  :alt="file.display_title"
                  loading="lazy"
                  class="h-full w-full object-cover"
                />
                <LuAlertCircle v-else class="h-4 w-4 text-[var(--md-sys-color-warning)]" />
              </div>

              <div class="min-w-0 flex-1">
                <div class="flex items-center gap-2">
                  <span class="truncate text-[13px] font-semibold text-[var(--md-sys-color-on-surface)]">
                    {{ file.display_title }}
                  </span>
                  <span
                    class="shrink-0 rounded-full bg-[var(--md-sys-color-surface-container-high)] px-2 py-0.5 font-mono text-[10px] font-semibold uppercase text-[var(--md-sys-color-on-surface-variant)]"
                  >
                    {{ file.extension }}
                  </span>
                </div>
                <div class="mt-0.5 truncate text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
                  {{ file.display_subtitle }} · {{ formatBytes(file.file_size) }}
                </div>
              </div>
            </button>

            <div class="hidden shrink-0 items-center gap-1.5 sm:flex">
              <span
                v-if="!file.completeness.has_cover"
                class="rounded-full bg-[var(--md-sys-color-warning-container)] px-2 py-0.5 text-[10px] font-medium text-[var(--md-sys-color-on-warning-container)]"
              >
                缺图
              </span>
              <span
                v-if="!file.completeness.has_summary_or_lyrics"
                class="rounded-full bg-[var(--md-sys-color-surface-container-high)] px-2 py-0.5 text-[10px] font-medium text-[var(--md-sys-color-on-surface-variant)]"
              >
                {{ file.media_kind === 'music' ? '缺歌词' : '缺简介' }}
              </span>
              <span
                v-if="!file.completeness.has_nfo_or_tag_block"
                class="rounded-full bg-[var(--md-sys-color-surface-container-high)] px-2 py-0.5 text-[10px] font-medium text-[var(--md-sys-color-on-surface-variant)]"
              >
                缺标签
              </span>
            </div>

            <button
              type="button"
              class="md-icon-btn md-icon-btn-sm md-state shrink-0 text-[var(--md-sys-color-primary)]"
              title="快速刮削此条目"
              @click="emit('scrapeFiles', [file], file.display_title)"
            >
              <LuSparkles class="h-4 w-4" />
            </button>
          </div>
        </div>
      </section>
    </template>
  </div>
</template>
