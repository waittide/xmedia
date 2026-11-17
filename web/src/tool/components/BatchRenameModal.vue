<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import {
  LuAlertTriangle,
  LuArrowRight,
  LuCheck,
  LuFileSignature,
  LuRefreshCw,
  LuX,
} from 'vue-icons-plus/lu'
import type { MediaKind } from '@/types/api'
import { toolApi } from '../api'
import type { MediaFileEntry, RenamePlanItem, RenameTemplatesDto } from '../types'

const props = defineProps<{
  open: boolean
  rootId: string
  entries: MediaFileEntry[]
  templates: RenameTemplatesDto
}>()

const emit = defineEmits<{
  close: []
  renamed: [movedCount: number]
}>()

const templateInput = ref('')
const plans = ref<RenamePlanItem[]>([])
const loadingPreview = ref(false)
const executing = ref(false)
const errorMsg = ref<string | null>(null)

const dominantKind = computed<MediaKind>(() => props.entries[0]?.media_kind ?? 'music')

const tokenChips = computed<string[]>(() => {
  const k = dominantKind.value
  if (k === 'music') {
    return ['{track:2}', '{disc}', '{artist}', '{album}', '{title}', '{year}']
  }
  if (k === 'video') {
    return ['{show}', '{season:2}', '{episode:2}', '{title}', '{year}', '{jellyfin_id}', '{tmdbid}']
  }
  if (k === 'comic') {
    return ['{series}', '{number:2}', '{volume:2}', '{title}', '{year}']
  }
  return ['{author}', '{title}', '{year}', '{isbn}']
})

function defaultTemplateForEntries(): string {
  const k = dominantKind.value
  if (k === 'music') return props.templates.music
  if (k === 'comic') return props.templates.comic
  if (k === 'book') return props.templates.book
  const looksLikeEpisode = props.entries.some((e) => /s\d{1,2}e\d{1,3}/i.test(e.file_name))
  return looksLikeEpisode ? props.templates.video_episode : props.templates.video_movie
}

let debounceTimer: ReturnType<typeof setTimeout> | null = null

async function runPreview(): Promise<void> {
  if (!props.open || props.entries.length === 0) return
  loadingPreview.value = true
  errorMsg.value = null
  try {
    plans.value = await toolApi.previewRename(
      props.rootId,
      props.entries.map((e) => e.rel_path),
      templateInput.value.trim() || null,
      dominantKind.value,
    )
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    loadingPreview.value = false
  }
}

watch(
  () => props.open,
  (isOpen) => {
    if (isOpen) {
      templateInput.value = defaultTemplateForEntries()
      void runPreview()
    }
  },
)

watch(templateInput, () => {
  if (!props.open) return
  if (debounceTimer) clearTimeout(debounceTimer)
  debounceTimer = setTimeout(() => {
    void runPreview()
  }, 260)
})

function appendChip(tok: string): void {
  templateInput.value = `${templateInput.value}${tok}`
}

const actionableCount = computed(
  () => plans.value.filter((p) => !p.unchanged && !p.conflict).length,
)

async function handleExecute(): Promise<void> {
  if (actionableCount.value === 0 || executing.value) return
  executing.value = true
  errorMsg.value = null
  try {
    const res = await toolApi.executeRename(props.rootId, plans.value)
    emit('renamed', res.moved_count)
    emit('close')
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    executing.value = false
  }
}
</script>

<template>
  <Teleport to="body">
    <div
      v-if="open"
      class="fixed inset-0 z-50 flex items-center justify-center bg-black/55 p-3 backdrop-blur-xs sm:p-6"
      @click.self="emit('close')"
    >
      <div
        class="flex max-h-[88vh] w-full max-w-4xl flex-col overflow-hidden rounded-3xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] shadow-2xl"
      >
        <!-- Header -->
        <div
          class="flex shrink-0 items-center justify-between gap-3 border-b border-[var(--ctp-surface0)]/80 px-5 py-4"
        >
          <div class="flex items-center gap-2">
            <LuFileSignature class="h-4 w-4 text-[var(--ctp-accent)]" />
            <h2 class="text-sm font-bold text-[var(--ctp-text)]">
              按元信息模板批量重命名 ({{ entries.length }} 个文件)
            </h2>
          </div>
          <button
            type="button"
            class="flex h-8 w-8 items-center justify-center rounded-xl text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)]/70 hover:text-[var(--ctp-text)]"
            @click="emit('close')"
          >
            <LuX class="h-4 w-4" />
          </button>
        </div>

        <!-- Template Editor -->
        <div class="shrink-0 space-y-2.5 border-b border-[var(--ctp-surface0)]/75 bg-[var(--ctp-mantle)]/40 px-5 py-3.5">
          <div class="flex items-center gap-2">
            <input
              v-model="templateInput"
              type="text"
              placeholder="输入重命名模板，例如 {track} - {artist} - {title}"
              class="flex-1 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3.5 py-2 font-mono text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
            />
            <button
              type="button"
              class="inline-flex items-center gap-1.5 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs font-medium text-[var(--ctp-text)] transition hover:border-[var(--ctp-accent)]"
              @click="runPreview"
            >
              <LuRefreshCw class="h-3.5 w-3.5" :class="loadingPreview ? 'animate-spin' : ''" />
              <span>刷新预览</span>
            </button>
          </div>

          <div class="flex flex-wrap items-center gap-1.5">
            <span class="text-[11px] text-[var(--ctp-subtext0)]">可用占位符：</span>
            <button
              v-for="chip in tokenChips"
              :key="chip"
              type="button"
              class="rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-2 py-0.5 font-mono text-[11px] text-[var(--ctp-accent)] transition hover:border-[var(--ctp-accent)]"
              @click="appendChip(chip)"
            >
              {{ chip }}
            </button>
            <span class="ml-auto text-[11px] text-[var(--ctp-subtext0)]">
              同名外挂文件（.lrc / .nfo / .srt / .ass）将自动跟随改名
            </span>
          </div>
        </div>

        <!-- Preview Diff List -->
        <div class="flex-1 space-y-2 overflow-y-auto p-4">
          <div
            v-if="errorMsg"
            class="rounded-xl border border-red-500/35 bg-red-500/10 px-3 py-2 text-xs text-red-400"
          >
            {{ errorMsg }}
          </div>

          <div
            v-for="item in plans"
            :key="item.from_rel_path"
            class="rounded-2xl border border-[var(--ctp-surface0)]/80 bg-[var(--ctp-mantle)]/35 p-3"
          >
            <div class="flex flex-wrap items-center gap-2 text-xs">
              <span class="font-mono text-[var(--ctp-subtext1)]">{{ item.from_rel_path }}</span>
              <LuArrowRight class="h-3.5 w-3.5 shrink-0 text-[var(--ctp-subtext0)]" />
              <span
                class="font-mono font-semibold"
                :class="item.unchanged ? 'text-[var(--ctp-subtext0)]' : 'text-[var(--ctp-accent)]'"
              >
                {{ item.to_rel_path }}
              </span>

              <span
                v-if="item.unchanged"
                class="ml-auto rounded-full bg-[var(--ctp-surface0)]/65 px-2 py-0.5 text-[10px] text-[var(--ctp-subtext0)]"
              >
                文件名无变化
              </span>
              <span
                v-else-if="item.conflict"
                class="ml-auto inline-flex items-center gap-1 rounded-full bg-red-500/15 px-2.5 py-0.5 text-[10px] font-semibold text-red-400"
              >
                <LuAlertTriangle class="h-3 w-3" />
                {{ item.conflict }}
              </span>
              <span
                v-else
                class="ml-auto inline-flex items-center gap-1 rounded-full bg-emerald-500/15 px-2.5 py-0.5 text-[10px] font-semibold text-emerald-500"
              >
                <LuCheck class="h-3 w-3" />
                可安全重命名
              </span>
            </div>

            <div
              v-if="item.companion_moves.length > 0"
              class="mt-1.5 flex flex-wrap items-center gap-1.5 text-[11px] text-[var(--ctp-subtext0)]"
            >
              <span>联动附属文件：</span>
              <span
                v-for="[cFrom, cTo] in item.companion_moves"
                :key="cFrom"
                class="rounded-md bg-[var(--ctp-canvas)] px-2 py-0.5 font-mono text-[10px]"
              >
                {{ cFrom }} → {{ cTo }}
              </span>
            </div>
          </div>
        </div>

        <!-- Footer -->
        <div
          class="flex shrink-0 items-center justify-between gap-3 border-t border-[var(--ctp-surface0)]/80 bg-[var(--ctp-mantle)]/35 px-5 py-3.5"
        >
          <span class="text-xs text-[var(--ctp-subtext0)]">
            共 {{ actionableCount }} 项待改名（冲突项与未变化项会自动跳过）
          </span>
          <div class="flex items-center gap-2">
            <button
              type="button"
              class="rounded-xl px-4 py-2 text-xs font-medium text-[var(--ctp-subtext1)] transition hover:bg-[var(--ctp-surface0)]/65"
              @click="emit('close')"
            >
              取消
            </button>
            <button
              type="button"
              :disabled="actionableCount === 0 || executing"
              class="inline-flex items-center gap-1.5 rounded-xl bg-[var(--ctp-accent)] px-4 py-2 text-xs font-semibold text-[var(--ctp-on-accent)] shadow-sm transition hover:opacity-95 disabled:opacity-50"
              @click="handleExecute"
            >
              <LuRefreshCw v-if="executing" class="h-3.5 w-3.5 animate-spin" />
              <LuCheck v-else class="h-3.5 w-3.5" />
              <span>执行批量重命名 ({{ actionableCount }})</span>
            </button>
          </div>
        </div>
      </div>
    </div>
  </Teleport>
</template>
