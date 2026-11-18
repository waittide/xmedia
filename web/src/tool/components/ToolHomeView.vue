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
  LuFolder,
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
  enterFolder: [relPath: string]
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

const completionPct = computed(() => {
  if (props.stats.total <= 0) return 0
  return Math.min(100, Math.round((props.stats.complete / props.stats.total) * 100))
})

const incompleteFiles = computed<MediaFileEntry[]>(() =>
  (props.snapshot?.files ?? []).filter((f) => !isEntryComplete(f)),
)

const previewIncomplete = computed<MediaFileEntry[]>(() => incompleteFiles.value.slice(0, 8))
const previewFolders = computed(() => (props.snapshot?.folders ?? []).slice(0, 8))
</script>

<template>
  <div class="mx-auto max-w-6xl space-y-6 p-4 sm:p-6">
    <!-- Empty State: No Roots Configured -->
    <div
      v-if="domainRoots.length === 0"
      class="mx-auto mt-10 max-w-lg rounded-3xl border border-dashed border-[var(--ctp-surface1)] bg-[var(--ctp-mantle)]/45 p-8 text-center"
    >
      <div
        class="mx-auto mb-4 flex h-12 w-12 items-center justify-center rounded-2xl bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]"
      >
        <LuFolderPlus class="h-6 w-6" />
      </div>
      <h2 class="text-base font-bold text-[var(--ctp-text)]">
        {{ domainLabels.emptyRootsTitle }}
      </h2>
      <p class="mt-2 text-xs leading-relaxed text-[var(--ctp-subtext0)]">
        {{ domainLabels.emptyRootsDesc }}
      </p>
      <button
        type="button"
        class="mt-5 inline-flex items-center gap-2 rounded-xl bg-[var(--ctp-accent)] px-5 py-2.5 text-xs font-semibold text-[var(--ctp-on-accent)] shadow-sm transition hover:opacity-95"
        @click="emit('openSettings', 'roots')"
      >
        <LuFolderPlus class="h-4 w-4" />
        <span>{{ txt.roots.add }}</span>
      </button>
    </div>

    <template v-else>
      <!-- Top Header & Refresh -->
      <div class="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h1 class="text-xl font-bold tracking-tight text-[var(--ctp-text)]">
            {{ txt.domains[activeDomain] }}概览
          </h1>
          <p class="mt-0.5 text-xs text-[var(--ctp-subtext0)]">
            当前目录：{{ snapshot?.root_name || '—'
            }}{{ snapshot?.current_rel_path ? ` / ${snapshot.current_rel_path}` : '' }}
          </p>
        </div>

        <div class="flex flex-wrap items-center gap-2">
          <RefreshButton size="md" :action="() => emit('refreshDir')" :busy="loadingDir" />

          <button
            v-if="incompleteFiles.length > 0"
            type="button"
            class="inline-flex h-9 items-center gap-1.5 rounded-xl bg-[var(--ctp-accent)] px-3.5 text-xs font-semibold text-[var(--ctp-on-accent)] shadow-sm transition hover:opacity-95"
            title="按「本地已有信息 → 高优先级源 → 低优先级源」顺序自动添加后台刮削任务"
            @click="emit('scrapeFiles', incompleteFiles, '待补齐项')"
          >
            <LuSparkles class="h-3.5 w-3.5" />
            <span>刮削待补齐项 ({{ incompleteFiles.length }})</span>
          </button>

          <button
            v-if="stats.total > 0"
            type="button"
            class="inline-flex h-9 items-center gap-1.5 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-shell)] px-3.5 text-xs font-semibold text-[var(--ctp-text)] transition hover:border-[var(--ctp-accent)]"
            @click="emit('openRenameAll')"
          >
            <LuFileSignature class="h-3.5 w-3.5 text-[var(--ctp-accent)]" />
            <span>{{ txt.actions.batchRename }}</span>
          </button>
        </div>
      </div>

      <!-- 4 Stat Overview Cards (Aligned with Server HomeView) -->
      <div class="grid grid-cols-2 gap-3 lg:grid-cols-4">
        <button
          type="button"
          class="card-soft flex items-center justify-between rounded-2xl bg-[var(--ctp-shell)] p-4 text-left transition hover:border-[var(--ctp-accent)]"
          @click="emit('openItems', 'all')"
        >
          <div>
            <div class="text-xs text-[var(--ctp-subtext0)]">{{ txt.cards.totalFiles }}</div>
            <div class="mt-1 text-2xl font-bold tracking-tight">{{ stats.total }}</div>
          </div>
          <div
            class="flex h-10 w-10 items-center justify-center rounded-xl bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]"
          >
            <LuBookOpen v-if="activeDomain === 'book'" class="h-5 w-5" />
            <LuLayers v-else-if="activeDomain === 'comic'" class="h-5 w-5" />
            <LuDisc3 v-else-if="activeDomain === 'music'" class="h-5 w-5" />
            <LuClapperboard v-else class="h-5 w-5" />
          </div>
        </button>

        <button
          type="button"
          class="card-soft flex items-center justify-between rounded-2xl bg-[var(--ctp-shell)] p-4 text-left transition hover:border-emerald-500/50"
          @click="emit('openItems', 'complete')"
        >
          <div>
            <div class="text-xs text-[var(--ctp-subtext0)]">{{ txt.cards.completeFiles }}</div>
            <div class="mt-1 flex items-baseline gap-1.5">
              <span class="text-2xl font-bold tracking-tight text-emerald-500">
                {{ stats.complete }}
              </span>
              <span class="font-mono text-xs text-[var(--ctp-subtext0)]">{{ completionPct }}%</span>
            </div>
          </div>
          <div
            class="flex h-10 w-10 items-center justify-center rounded-xl bg-emerald-500/15 text-emerald-500"
          >
            <LuCheckCircle2 class="h-5 w-5" />
          </div>
        </button>

        <button
          type="button"
          class="card-soft flex items-center justify-between rounded-2xl bg-[var(--ctp-shell)] p-4 text-left transition hover:border-amber-500/50"
          @click="emit('openItems', 'missing_cover')"
        >
          <div>
            <div class="text-xs text-[var(--ctp-subtext0)]">{{ domainLabels.missingCover }}</div>
            <div class="mt-1 text-2xl font-bold tracking-tight text-amber-500">
              {{ stats.missingCover }}
            </div>
          </div>
          <div
            class="flex h-10 w-10 items-center justify-center rounded-xl bg-amber-500/15 text-amber-500"
          >
            <LuImage class="h-5 w-5" />
          </div>
        </button>

        <button
          type="button"
          class="card-soft flex items-center justify-between rounded-2xl bg-[var(--ctp-shell)] p-4 text-left transition hover:border-[var(--ctp-accent)]/50"
          @click="emit('openItems', 'missing_summary')"
        >
          <div>
            <div class="text-xs text-[var(--ctp-subtext0)]">{{ domainLabels.missingSummary }}</div>
            <div class="mt-1 text-2xl font-bold tracking-tight text-[var(--ctp-accent)]">
              {{ stats.missingSummary }}
            </div>
          </div>
          <div
            class="flex h-10 w-10 items-center justify-center rounded-xl bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]"
          >
            <LuFileText class="h-5 w-5" />
          </div>
        </button>
      </div>

      <!-- Metadata Completeness Progress Card -->
      <section class="card-soft space-y-2.5 rounded-2xl bg-[var(--ctp-shell)] p-5">
        <div class="flex flex-wrap items-center justify-between gap-2">
          <div class="flex items-center gap-2">
            <span class="text-xs font-bold text-[var(--ctp-text)]">元信息完整率</span>
            <span
              class="rounded-full bg-[var(--ctp-accent-soft)] px-2 py-0.5 font-mono text-[11px] font-bold text-[var(--ctp-accent)]"
            >
              {{ completionPct }}%
            </span>
          </div>
          <span class="text-[11px] text-[var(--ctp-subtext0)]">
            齐全 {{ stats.complete }} 项 · 待补齐 {{ stats.incomplete }} 项 · 缺标签/NFO
            {{ stats.missingTag }} 项
          </span>
        </div>
        <div class="h-2 w-full overflow-hidden rounded-full bg-[var(--ctp-surface0)]">
          <div
            class="h-full rounded-full bg-emerald-500 transition-all duration-300"
            :style="{ width: `${completionPct}%` }"
          />
        </div>
      </section>

      <!-- Subfolders Quick Grid -->
      <section v-if="previewFolders.length > 0" class="space-y-3">
        <div class="flex items-center justify-between">
          <h2 class="text-sm font-bold text-[var(--ctp-text)]">{{ txt.roots.foldersTitle }}</h2>
          <button
            type="button"
            class="inline-flex items-center gap-1 text-xs font-semibold text-[var(--ctp-accent)] hover:opacity-85"
            @click="emit('openItems', 'all')"
          >
            <span>查看全部目录</span>
            <LuArrowRight class="h-3.5 w-3.5" />
          </button>
        </div>

        <div class="grid grid-cols-2 gap-2.5 sm:grid-cols-3 lg:grid-cols-4">
          <button
            v-for="folder in previewFolders"
            :key="folder.rel_path"
            type="button"
            class="card-soft flex items-center gap-3 rounded-2xl bg-[var(--ctp-shell)] p-3 text-left transition hover:border-[var(--ctp-accent)]"
            @click="emit('enterFolder', folder.rel_path)"
          >
            <div
              class="flex h-9 w-9 shrink-0 items-center justify-center rounded-xl bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]"
            >
              <LuFolder class="h-4 w-4" />
            </div>
            <div class="min-w-0 flex-1">
              <div class="truncate text-xs font-semibold text-[var(--ctp-text)]">
                {{ folder.name }}
              </div>
              <div class="mt-0.5 text-[11px] text-[var(--ctp-subtext0)]">
                {{ folder.media_file_count }} 个媒体文件
              </div>
            </div>
          </button>
        </div>
      </section>

      <!-- Pending / Incomplete Items Section -->
      <section class="space-y-3">
        <div class="flex items-center justify-between">
          <div class="flex items-center gap-2">
            <h2 class="text-sm font-bold text-[var(--ctp-text)]">
              {{ incompleteFiles.length > 0 ? '待补齐元信息条目' : '全部条目元信息已齐全' }}
            </h2>
            <span
              v-if="incompleteFiles.length > 0"
              class="rounded-full bg-amber-500/15 px-2 py-0.5 font-mono text-[11px] font-semibold text-amber-500"
            >
              {{ incompleteFiles.length }}
            </span>
          </div>
          <button
            type="button"
            class="inline-flex items-center gap-1 text-xs font-semibold text-[var(--ctp-accent)] hover:opacity-85"
            @click="emit('openItems', incompleteFiles.length > 0 ? 'incomplete' : 'all')"
          >
            <span>进入条目列表</span>
            <LuArrowRight class="h-3.5 w-3.5" />
          </button>
        </div>

        <div
          v-if="previewIncomplete.length === 0"
          class="rounded-2xl border border-dashed border-[var(--ctp-surface1)] bg-[var(--ctp-shell)]/60 py-10 text-center text-xs text-[var(--ctp-subtext0)]"
        >
          {{
            stats.total === 0
              ? '当前目录下暂无受支持的媒体文件'
              : '当前目录所有媒体文件的封面、简介与标签均已完备'
          }}
        </div>

        <div v-else class="space-y-2">
          <div
            v-for="file in previewIncomplete"
            :key="file.rel_path"
            class="card-soft flex items-center gap-3 rounded-2xl bg-[var(--ctp-shell)] px-3.5 py-2.5 transition hover:border-[var(--ctp-accent)]/60"
          >
            <button
              type="button"
              class="flex min-w-0 flex-1 items-center gap-3 text-left"
              @click="emit('inspectFile', file)"
            >
              <div
                class="relative flex h-11 w-11 shrink-0 items-center justify-center overflow-hidden rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)]"
              >
                <img
                  v-if="file.completeness.has_cover"
                  :src="buildArtworkUrl(file.root_id, file.rel_path, file.media_kind, artworkVersion)"
                  :alt="file.display_title"
                  loading="lazy"
                  class="h-full w-full object-cover"
                />
                <LuAlertCircle v-else class="h-4 w-4 text-amber-500" />
              </div>

              <div class="min-w-0 flex-1">
                <div class="flex items-center gap-2">
                  <span class="truncate text-xs font-bold text-[var(--ctp-text)]">
                    {{ file.display_title }}
                  </span>
                  <span
                    class="shrink-0 rounded-md bg-[var(--ctp-surface0)]/80 px-1.5 py-0.5 font-mono text-[10px] font-semibold uppercase text-[var(--ctp-subtext1)]"
                  >
                    {{ file.extension }}
                  </span>
                </div>
                <div class="mt-0.5 truncate text-[11px] text-[var(--ctp-subtext0)]">
                  {{ file.display_subtitle }} · {{ formatBytes(file.file_size) }}
                </div>
              </div>
            </button>

            <div class="hidden shrink-0 items-center gap-1.5 sm:flex">
              <span
                v-if="!file.completeness.has_cover"
                class="rounded-full bg-amber-500/12 px-2 py-0.5 text-[10px] font-medium text-amber-500"
              >
                缺图
              </span>
              <span
                v-if="!file.completeness.has_summary_or_lyrics"
                class="rounded-full bg-[var(--ctp-surface0)] px-2 py-0.5 text-[10px] font-medium text-[var(--ctp-subtext0)]"
              >
                {{ file.media_kind === 'music' ? '缺歌词' : '缺简介' }}
              </span>
              <span
                v-if="!file.completeness.has_nfo_or_tag_block"
                class="rounded-full bg-[var(--ctp-surface0)] px-2 py-0.5 text-[10px] font-medium text-[var(--ctp-subtext0)]"
              >
                {{ file.media_kind === 'video' ? '缺 NFO' : '缺标签' }}
              </span>
            </div>

            <button
              type="button"
              class="inline-flex h-8 shrink-0 items-center gap-1 rounded-xl bg-[var(--ctp-accent-soft)] px-2.5 text-[11px] font-semibold text-[var(--ctp-accent)] transition hover:opacity-90"
              title="添加刮削任务（本地已有信息优先 -> 高优先级源 -> 低优先级源）"
              @click="emit('scrapeFiles', [file])"
            >
              <LuSparkles class="h-3.5 w-3.5" />
              <span>{{ txt.actions.scrape }}</span>
            </button>
          </div>
        </div>
      </section>
    </template>
  </div>
</template>
