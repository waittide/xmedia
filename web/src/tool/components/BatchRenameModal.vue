<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue'
import {
  LuAlertTriangle,
  LuArrowRight,
  LuCheck,
  LuFileSignature,
  LuRefreshCw,
  LuX,
} from 'vue-icons-plus/lu'
import AppCheckbox from '@/components/ui/AppCheckbox.vue'
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
  renamed: [movedCount: number, movedMap: Record<string, string>]
}>()

const templateInput = ref('')
const episodeTemplateInput = ref('')
const focusedVideoInput = ref<'movie' | 'episode'>('movie')
const movieInputRef = ref<HTMLInputElement | null>(null)
const episodeInputRef = ref<HTMLInputElement | null>(null)
const genericInputRef = ref<HTMLInputElement | null>(null)
const plans = ref<RenamePlanItem[]>([])
const excludedPaths = ref<Set<string>>(new Set())
const loadingPreview = ref(false)
const executing = ref(false)
const errorMsg = ref<string | null>(null)

const dominantKind = computed<MediaKind>(() => props.entries[0]?.media_kind ?? 'music')

const tokenChips = computed<string[]>(() => {
  const k = dominantKind.value
  if (k === 'music') {
    return [
      '{album_artist}',
      '{artist}',
      '{album}',
      '{disc}',
      '{track:2}',
      '{title}',
      '{year}',
      '{genre}',
    ]
  }
  if (k === 'video') {
    return [
      '{show}',
      '{season:2}',
      '{episode:2}',
      '{title}',
      '{original_title}',
      '{year}',
      '{jellyfin_id}',
      '{tmdbid}',
    ]
  }
  if (k === 'comic') {
    return [
      '{series}',
      '{series_group}',
      '<Vol.{volume:2}>',
      '<Ch.{number:2}>',
      '{title}',
      '{publisher}',
      '{year}',
    ]
  }
  return [
    '{author}',
    '{series}',
    '{subseries}',
    '<Vol.{volume:2} >',
    '{title}',
    '{publisher}',
    '{year}',
    '{isbn}',
  ]
})

function seedTemplatesForEntries(): void {
  const k = dominantKind.value
  if (k === 'music') {
    templateInput.value = props.templates.music
    episodeTemplateInput.value = ''
  } else if (k === 'comic') {
    templateInput.value = props.templates.comic
    episodeTemplateInput.value = ''
  } else if (k === 'book') {
    templateInput.value = props.templates.book
    episodeTemplateInput.value = ''
  } else {
    templateInput.value = props.templates.video_movie
    episodeTemplateInput.value = props.templates.video_episode
    const looksLikeEpisode = props.entries.some((e) => /s\d{1,2}e\d{1,3}/i.test(e.file_name))
    focusedVideoInput.value = looksLikeEpisode ? 'episode' : 'movie'
  }
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
      dominantKind.value === 'video' ? episodeTemplateInput.value.trim() || null : undefined,
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
      excludedPaths.value = new Set()
      seedTemplatesForEntries()
      void runPreview()
    }
  },
)

watch([templateInput, episodeTemplateInput], () => {
  if (!props.open) return
  if (debounceTimer) clearTimeout(debounceTimer)
  debounceTimer = setTimeout(() => {
    void runPreview()
  }, 260)
})

async function insertTokenAtCursor(
  model: { value: string },
  inputEl: HTMLInputElement | null,
  tok: string,
): Promise<void> {
  if (!inputEl) {
    model.value = `${model.value}${tok}`
    return
  }
  const start = inputEl.selectionStart ?? model.value.length
  const end = inputEl.selectionEnd ?? start
  const nextVal = `${model.value.slice(0, start)}${tok}${model.value.slice(end)}`
  model.value = nextVal
  await nextTick()
  inputEl.focus()
  const caret = start + tok.length
  inputEl.setSelectionRange(caret, caret)
}

function appendChip(tok: string): void {
  if (dominantKind.value === 'video' && focusedVideoInput.value === 'episode') {
    void insertTokenAtCursor(episodeTemplateInput, episodeInputRef.value, tok)
  } else if (dominantKind.value === 'video') {
    void insertTokenAtCursor(templateInput, movieInputRef.value, tok)
  } else {
    void insertTokenAtCursor(templateInput, genericInputRef.value, tok)
  }
}

function toggleRowExcluded(fromRelPath: string): void {
  const next = new Set(excludedPaths.value)
  if (next.has(fromRelPath)) {
    next.delete(fromRelPath)
  } else {
    next.add(fromRelPath)
  }
  excludedPaths.value = next
}

const activePlans = computed(() =>
  plans.value.filter((p) => !excludedPaths.value.has(p.from_rel_path)),
)

const actionableCount = computed(
  () => activePlans.value.filter((p) => !p.unchanged && !p.conflict).length,
)

async function handleExecute(): Promise<void> {
  if (actionableCount.value === 0 || executing.value) return
  executing.value = true
  errorMsg.value = null
  try {
    const selected = activePlans.value
    const movedMap: Record<string, string> = {}
    for (const p of selected) {
      if (!p.unchanged && !p.conflict) {
        movedMap[p.from_rel_path] = p.to_rel_path
      }
    }
    const res = await toolApi.executeRename(props.rootId, selected)
    emit('renamed', res.moved_count, movedMap)
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
          <template v-if="dominantKind === 'video'">
            <div class="grid grid-cols-1 gap-2 sm:grid-cols-12">
              <label class="block sm:col-span-5">
                <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">
                  电影归档模板（对非剧集生效）
                </span>
                <input
                  ref="movieInputRef"
                  v-model="templateInput"
                  type="text"
                  placeholder="{title} ({year})/{title} ({year})"
                  class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 font-mono text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
                  @focus="focusedVideoInput = 'movie'"
                />
              </label>
              <label class="block sm:col-span-5">
                <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">
                  剧集归档模板（对剧集 SxxExx 生效）
                </span>
                <input
                  ref="episodeInputRef"
                  v-model="episodeTemplateInput"
                  type="text"
                  placeholder="{show} ({year})/Season {season:2}/{show} - S{season:2}E{episode:2} - {title}"
                  class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 font-mono text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
                  @focus="focusedVideoInput = 'episode'"
                />
              </label>
              <div class="flex items-end sm:col-span-2">
                <button
                  type="button"
                  class="inline-flex w-full items-center justify-center gap-1.5 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs font-medium text-[var(--ctp-text)] transition hover:border-[var(--ctp-accent)]"
                  @click="runPreview"
                >
                  <LuRefreshCw class="h-3.5 w-3.5" :class="loadingPreview ? 'animate-spin' : ''" />
                  <span>刷新</span>
                </button>
              </div>
            </div>
          </template>
          <div v-else class="flex items-center gap-2">
            <input
              ref="genericInputRef"
              v-model="templateInput"
              type="text"
              placeholder="输入重命名模板，例如 {track:2} - {artist} - {title}"
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
            class="rounded-2xl border border-[var(--ctp-surface0)]/80 bg-[var(--ctp-mantle)]/35 p-3 transition"
            :class="excludedPaths.has(item.from_rel_path) ? 'opacity-50' : ''"
          >
            <div class="flex flex-wrap items-center gap-2 text-xs">
              <AppCheckbox
                :model-value="!excludedPaths.has(item.from_rel_path)"
                @change="toggleRowExcluded(item.from_rel_path)"
              />
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
