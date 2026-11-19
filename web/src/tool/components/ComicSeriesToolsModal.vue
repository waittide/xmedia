<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { LuCheck, LuCombine, LuLayers, LuLoader2, LuX } from 'vue-icons-plus/lu'

import AppSelect from '@/components/ui/AppSelect.vue'
import { toolApi } from '../api'
import type { MediaFileEntry } from '../types'

const props = defineProps<{
  open: boolean
  rootId: string | null
  entries: MediaFileEntry[]
}>()

const emit = defineEmits<{
  close: []
  completed: []
}>()

const activeTab = ref<'sync' | 'merge'>('sync')
const busy = ref(false)
const errorMsg = ref('')
const successMsg = ref('')

// Batch Sync state
const series = ref('')
const seriesGroup = ref('')
const writers = ref('')
const pencillers = ref('')
const publisher = ref('')
const genres = ref('')
const tags = ref('')
const readingDirection = ref('')
const numberingMode = ref<'none' | 'chapter' | 'volume'>('chapter')

const directionOptions = [
  { value: '', label: '保持原样' },
  { value: 'rtl', label: '日漫右向左 (RTL)' },
  { value: 'ltr', label: '欧美漫左向右 (LTR)' },
  { value: 'webtoon', label: '条漫竖向滚动 (Webtoon)' },
]
const numberingOptions = [
  { value: 'none', label: '不修改编号' },
  { value: 'chapter', label: '顺序写入话号 (Number: 01, 02...)' },
  { value: 'volume', label: '顺序写入卷号 (Volume: 1, 2...)' },
]

function handleKeydown(ev: KeyboardEvent): void {
  if (props.open && ev.key === 'Escape') {
    ev.preventDefault()
    emit('close')
  }
}

watch(
  () => props.open,
  (isOpen) => {
    if (isOpen) window.addEventListener('keydown', handleKeydown)
    else window.removeEventListener('keydown', handleKeydown)
  },
  { immediate: true },
)

onBeforeUnmount(() => window.removeEventListener('keydown', handleKeydown))
const startIndex = ref(1)
const padWidth = ref(2)

// Merge state
const mergeOutputName = ref('')
const mergeSeries = ref('')
const mergeTitle = ref('')
const mergeVolume = ref<number | null>(1)

const sortedEntries = computed(() =>
  [...props.entries].sort((a, b) => a.file_name.localeCompare(b.file_name, undefined, { numeric: true })),
)

watch(
  () => props.open,
  (isOpen) => {
    if (!isOpen) return
    errorMsg.value = ''
    successMsg.value = ''
    const first = sortedEntries.value[0]
    if (first) {
      const parentDir = first.rel_path.includes('/')
        ? first.rel_path.slice(0, first.rel_path.lastIndexOf('/'))
        : ''
      const guessedSeries = first.display_title.replace(/\s*(?:Vol\.?|Ch\.?|#)\s*\d+.*$/i, '').trim()
      series.value = guessedSeries
      mergeSeries.value = guessedSeries
      mergeTitle.value = `${guessedSeries || '合订本'} Vol.01`
      const baseName = `${guessedSeries || 'merged'}_Vol.01.cbz`
      mergeOutputName.value = parentDir ? `${parentDir}/${baseName}` : baseName
    }
  },
  { immediate: true },
)

function splitCsv(raw: string): string[] | undefined {
  const arr = raw
    .split(/[,，、;；]/)
    .map((s) => s.trim())
    .filter(Boolean)
  return arr.length > 0 ? arr : undefined
}

async function handleBatchSync(): Promise<void> {
  if (!props.rootId || sortedEntries.value.length === 0) return
  busy.value = true
  errorMsg.value = ''
  successMsg.value = ''
  try {
    const rep = await toolApi.batchSyncComics(
      props.rootId,
      sortedEntries.value.map((e) => e.rel_path),
      {
        series: series.value.trim() || undefined,
        series_group: seriesGroup.value.trim() || undefined,
        writers: splitCsv(writers.value),
        pencillers: splitCsv(pencillers.value),
        publisher: publisher.value.trim() || undefined,
        genres: splitCsv(genres.value),
        tags: splitCsv(tags.value),
        reading_direction: readingDirection.value || undefined,
        numbering_mode: numberingMode.value,
        start_index: startIndex.value,
        pad_width: padWidth.value,
      },
    )
    successMsg.value = `已同步 ${rep.updated_count} 本漫画${rep.failed_paths.length > 0 ? `（失败 ${rep.failed_paths.length} 本）` : ''}`
    emit('completed')
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    busy.value = false
  }
}

async function handleMerge(): Promise<void> {
  if (!props.rootId || sortedEntries.value.length < 2) return
  if (!mergeOutputName.value.trim()) {
    errorMsg.value = '请填写输出 CBZ 路径'
    return
  }
  busy.value = true
  errorMsg.value = ''
  successMsg.value = ''
  try {
    const res = await toolApi.mergeComics(
      props.rootId,
      sortedEntries.value.map((e) => e.rel_path),
      mergeOutputName.value.trim(),
      mergeSeries.value.trim() || undefined,
      mergeTitle.value.trim() || undefined,
      mergeVolume.value,
    )
    successMsg.value = `已合并 ${res.report.source_count} 话为 ${res.report.output_rel_path}（共 ${res.report.total_pages} 页）`
    emit('completed')
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <Teleport to="body">
    <div
      v-if="open"
      class="fixed inset-0 z-50 flex items-center justify-center bg-black/55 p-4 backdrop-blur-xs"
      @click.self="emit('close')"
    >
      <div
        class="flex max-h-[88vh] w-full max-w-2xl flex-col overflow-hidden rounded-2xl border border-[var(--ctp-surface0)] bg-[var(--ctp-panel)] text-[var(--ctp-text)] shadow-2xl"
      >
        <!-- Header -->
        <div class="flex items-center justify-between border-b border-[var(--ctp-surface0)] px-5 py-3.5">
          <div>
            <h3 class="text-sm font-semibold">漫画系列批量工具（已选 {{ sortedEntries.length }} 项）</h3>
            <p class="text-[11px] text-[var(--ctp-subtext0)]">
              按文件名自然序处理 · 支持批量同步系列元数据、顺序重编号与多话合卷
            </p>
          </div>
          <button
            type="button"
            class="rounded-xl p-1.5 text-[var(--ctp-subtext0)] hover:bg-[var(--ctp-surface0)]"
            @click="emit('close')"
          >
            <LuX class="h-4 w-4" />
          </button>
        </div>

        <!-- Tabs -->
        <div class="flex border-b border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)]/60 px-5 pt-2">
          <button
            type="button"
            class="inline-flex items-center gap-1.5 border-b-2 px-3 py-2 text-xs font-medium transition"
            :class="
              activeTab === 'sync'
                ? 'border-[var(--ctp-accent)] text-[var(--ctp-accent)]'
                : 'border-transparent text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]'
            "
            @click="activeTab = 'sync'"
          >
            <LuLayers class="h-3.5 w-3.5" />
            系列同步 & 顺序编号
          </button>
          <button
            type="button"
            class="inline-flex items-center gap-1.5 border-b-2 px-3 py-2 text-xs font-medium transition"
            :class="
              activeTab === 'merge'
                ? 'border-[var(--ctp-accent)] text-[var(--ctp-accent)]'
                : 'border-transparent text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]'
            "
            @click="activeTab = 'merge'"
          >
            <LuCombine class="h-3.5 w-3.5" />
            多话合卷为单行本 CBZ
          </button>
        </div>

        <!-- Body -->
        <div class="min-h-0 flex-1 overflow-y-auto p-5 text-xs">
          <div v-if="activeTab === 'sync'" class="grid grid-cols-1 gap-3 sm:grid-cols-2">
            <label class="block">
              <span class="mb-1 block text-[11px] text-[var(--ctp-subtext0)]">系列名 (Series) · 留空则不覆盖</span>
              <input
                v-model="series"
                type="text"
                class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 outline-none focus:border-[var(--ctp-accent)]"
              />
            </label>
            <label class="block">
              <span class="mb-1 block text-[11px] text-[var(--ctp-subtext0)]">系列合集组 (SeriesGroup)</span>
              <input
                v-model="seriesGroup"
                type="text"
                class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 outline-none focus:border-[var(--ctp-accent)]"
              />
            </label>
            <label class="block">
              <span class="mb-1 block text-[11px] text-[var(--ctp-subtext0)]">原作 / 编剧 (逗号分隔)</span>
              <input
                v-model="writers"
                type="text"
                class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 outline-none focus:border-[var(--ctp-accent)]"
              />
            </label>
            <label class="block">
              <span class="mb-1 block text-[11px] text-[var(--ctp-subtext0)]">作画 (逗号分隔)</span>
              <input
                v-model="pencillers"
                type="text"
                class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 outline-none focus:border-[var(--ctp-accent)]"
              />
            </label>
            <label class="block">
              <span class="mb-1 block text-[11px] text-[var(--ctp-subtext0)]">出版社</span>
              <input
                v-model="publisher"
                type="text"
                class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 outline-none focus:border-[var(--ctp-accent)]"
              />
            </label>
            <div class="block">
              <span class="mb-1 block text-[11px] text-[var(--ctp-subtext0)]">阅读方向</span>
              <AppSelect v-model="readingDirection" :options="directionOptions" />
            </div>
            <label class="block">
              <span class="mb-1 block text-[11px] text-[var(--ctp-subtext0)]">分类 (Genre)</span>
              <input
                v-model="genres"
                type="text"
                class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 outline-none focus:border-[var(--ctp-accent)]"
              />
            </label>
            <label class="block">
              <span class="mb-1 block text-[11px] text-[var(--ctp-subtext0)]">标签 (Tags)</span>
              <input
                v-model="tags"
                type="text"
                class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 outline-none focus:border-[var(--ctp-accent)]"
              />
            </label>

            <div class="rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)]/70 p-3 sm:col-span-2">
              <div class="mb-2 text-[11px] font-medium text-[var(--ctp-subtext0)]">顺序重编号（按文件名自然序）</div>
              <div class="flex flex-wrap items-center gap-3">
                <div class="w-60">
                  <AppSelect
                    :model-value="numberingMode"
                    :options="numberingOptions"
                    variant="compact"
                    @update:model-value="(v) => (numberingMode = v as 'none' | 'chapter' | 'volume')"
                  />
                </div>
                <label v-if="numberingMode !== 'none'" class="inline-flex items-center gap-1.5">
                  <span>起始序号</span>
                  <input
                    v-model.number="startIndex"
                    type="number"
                    min="0"
                    class="w-16 rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-panel)] px-2 py-1 text-xs"
                  />
                </label>
                <label v-if="numberingMode === 'chapter'" class="inline-flex items-center gap-1.5">
                  <span>补零位数</span>
                  <input
                    v-model.number="padWidth"
                    type="number"
                    min="1"
                    max="4"
                    class="w-14 rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-panel)] px-2 py-1 text-xs"
                  />
                </label>
              </div>
            </div>
          </div>

          <div v-else class="space-y-3">
            <label class="block">
              <span class="mb-1 block text-[11px] text-[var(--ctp-subtext0)]">输出 CBZ 相对路径</span>
              <input
                v-model="mergeOutputName"
                type="text"
                class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 outline-none focus:border-[var(--ctp-accent)]"
              />
            </label>
            <div class="grid grid-cols-1 gap-3 sm:grid-cols-3">
              <label class="block">
                <span class="mb-1 block text-[11px] text-[var(--ctp-subtext0)]">系列名</span>
                <input
                  v-model="mergeSeries"
                  type="text"
                  class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 outline-none focus:border-[var(--ctp-accent)]"
                />
              </label>
              <label class="block">
                <span class="mb-1 block text-[11px] text-[var(--ctp-subtext0)]">单行本标题</span>
                <input
                  v-model="mergeTitle"
                  type="text"
                  class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 outline-none focus:border-[var(--ctp-accent)]"
                />
              </label>
              <label class="block">
                <span class="mb-1 block text-[11px] text-[var(--ctp-subtext0)]">卷号 (Volume)</span>
                <input
                  v-model.number="mergeVolume"
                  type="number"
                  min="1"
                  class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 outline-none focus:border-[var(--ctp-accent)]"
                />
              </label>
            </div>
            <div class="rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)]/70 p-3">
              <div class="mb-1.5 text-[11px] font-medium text-[var(--ctp-subtext0)]">
                合并顺序预览（原文件将完整保留）：
              </div>
              <ol class="max-h-36 list-decimal space-y-1 overflow-y-auto pl-5 text-[11px]">
                <li v-for="e in sortedEntries" :key="e.rel_path" class="truncate">
                  {{ e.file_name }}
                </li>
              </ol>
            </div>
          </div>

          <div v-if="errorMsg" class="mt-3 rounded-xl bg-rose-500/10 px-3 py-2 text-rose-500">
            {{ errorMsg }}
          </div>
          <div v-if="successMsg" class="mt-3 flex items-center gap-1.5 rounded-xl bg-emerald-500/10 px-3 py-2 text-emerald-500">
            <LuCheck class="h-4 w-4 shrink-0" />
            <span>{{ successMsg }}</span>
          </div>
        </div>

        <!-- Footer -->
        <div class="flex items-center justify-end gap-2 border-t border-[var(--ctp-surface0)] px-5 py-3">
          <button
            type="button"
            class="rounded-xl border border-[var(--ctp-surface0)] px-3.5 py-1.5 text-xs hover:bg-[var(--ctp-surface0)]/60"
            @click="emit('close')"
          >
            关闭
          </button>
          <button
            v-if="activeTab === 'sync'"
            type="button"
            :disabled="busy || sortedEntries.length === 0"
            class="inline-flex items-center gap-1.5 rounded-xl bg-[var(--ctp-accent)] px-4 py-1.5 text-xs font-medium text-[var(--ctp-on-accent)] disabled:opacity-50"
            @click="handleBatchSync"
          >
            <LuLoader2 v-if="busy" class="h-3.5 w-3.5 animate-spin" />
            执行批量同步
          </button>
          <button
            v-else
            type="button"
            :disabled="busy || sortedEntries.length < 2"
            class="inline-flex items-center gap-1.5 rounded-xl bg-[var(--ctp-accent)] px-4 py-1.5 text-xs font-medium text-[var(--ctp-on-accent)] disabled:opacity-50"
            @click="handleMerge"
          >
            <LuLoader2 v-if="busy" class="h-3.5 w-3.5 animate-spin" />
            合并生成 CBZ
          </button>
        </div>
      </div>
    </div>
  </Teleport>
</template>
