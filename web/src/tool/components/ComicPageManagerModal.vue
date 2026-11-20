<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import {
  LuArrowLeft,
  LuArrowRight,
  LuBookOpen,
  LuCheck,
  LuChevronLeft,
  LuChevronRight,
  LuColumns2,
  LuGripVertical,
  LuImage,
  LuLoader2,
  LuMaximize2,
  LuRotateCcw,
  LuSave,
  LuSparkles,
  LuTrash2,
  LuX,
} from 'vue-icons-plus/lu'

import AppSelect from '@/components/ui/AppSelect.vue'
import AppSlider from '@/components/ui/AppSlider.vue'
import { toolApi } from '../api'
import type {
  ComicImageReencodeFormat,
  ComicOptimizeReportDto,
  ComicPageInfoDto,
  MediaFileDetail,
  MediaFileEntry,
} from '../types'

interface EditableComicPage extends ComicPageInfoDto {
  deleted: boolean
}

const props = defineProps<{
  open: boolean
  entry: MediaFileEntry | null
}>()

const emit = defineEmits<{
  close: []
  saved: [detail: MediaFileDetail]
}>()

const loading = ref(false)
const saving = ref(false)
const errorMsg = ref('')
const statusMsg = ref('')
const pages = ref<EditableComicPage[]>([])
const totalSizeBytes = ref(0)
const format = ref('cbz')
const imgVersion = ref(0)

const reencode = ref<ComicImageReencodeFormat>('keep')
const jpegQuality = ref(84)
const maxWidthStr = ref('')
const lastReport = ref<ComicOptimizeReportDto | null>(null)

// Progressive rendering & image skeleton state
const PAGE_BATCH_SIZE = 48
const visibleLimit = ref(PAGE_BATCH_SIZE)
const loadedEntries = ref<Set<string>>(new Set())

// Drag-and-drop reorder state
const dragFromIdx = ref<number | null>(null)
const dragOverIdx = ref<number | null>(null)

// Lightbox preview state
const lightboxIdx = ref<number | null>(null)
const lightboxPage = computed(() =>
  lightboxIdx.value !== null ? (pages.value[lightboxIdx.value] ?? null) : null,
)

const reencodeOptions = [
  { value: 'keep', label: '保持原图（无损重写）' },
  { value: 'jpeg', label: '转 JPEG 有损瘦身（推荐扫描版）' },
  { value: 'webp', label: '转 WebP 无损压缩（适合黑白线稿）' },
]

const maxWidthOptions = [
  { value: '', label: '不缩放' },
  { value: '2000', label: '2000 px（高清平板）' },
  { value: '1600', label: '1600 px（平衡瘦身）' },
  { value: '1280', label: '1280 px（轻量阅读）' },
]

const isCbzEditable = computed(() => format.value === 'cbz' || format.value === 'zip')
const keptCount = computed(() => pages.value.filter((p) => !p.deleted).length)
const deletedCount = computed(() => pages.value.filter((p) => p.deleted).length)
const visiblePages = computed(() => pages.value.slice(0, visibleLimit.value))
const hasMorePages = computed(() => pages.value.length > visibleLimit.value)

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(2)} MB`
}

function markLoaded(entryName: string): void {
  if (loadedEntries.value.has(entryName)) return
  const next = new Set(loadedEntries.value)
  next.add(entryName)
  loadedEntries.value = next
}

async function loadPages(options?: { preserveReport?: boolean }): Promise<void> {
  if (!props.entry) return
  const preserveReport = options?.preserveReport === true
  loading.value = true
  errorMsg.value = ''
  if (!preserveReport) {
    statusMsg.value = ''
    lastReport.value = null
  }
  visibleLimit.value = PAGE_BATCH_SIZE
  loadedEntries.value = new Set()
  lightboxIdx.value = null
  try {
    const snap = await toolApi.getComicPages(props.entry.root_id, props.entry.rel_path)
    format.value = snap.format
    totalSizeBytes.value = snap.total_size_bytes
    pages.value = snap.pages.map((p) => ({ ...p, deleted: false }))
    imgVersion.value += 1
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    loading.value = false
  }
}

watch(
  () => [props.open, props.entry?.rel_path],
  ([isOpen]) => {
    if (isOpen && props.entry) {
      void loadPages()
    }
  },
  { immediate: true },
)

function handleKeydown(ev: KeyboardEvent): void {
  if (!props.open) return
  if (ev.key === 'Escape') {
    ev.preventDefault()
    if (lightboxIdx.value !== null) {
      lightboxIdx.value = null
    } else {
      emit('close')
    }
  } else if (lightboxIdx.value !== null && ev.key === 'ArrowLeft') {
    ev.preventDefault()
    if (lightboxIdx.value > 0) lightboxIdx.value -= 1
  } else if (lightboxIdx.value !== null && ev.key === 'ArrowRight') {
    ev.preventDefault()
    if (lightboxIdx.value < pages.value.length - 1) lightboxIdx.value += 1
  }
}

watch(
  () => props.open,
  (isOpen) => {
    if (isOpen) {
      window.addEventListener('keydown', handleKeydown)
    } else {
      window.removeEventListener('keydown', handleKeydown)
    }
  },
  { immediate: true },
)

onBeforeUnmount(() => {
  window.removeEventListener('keydown', handleKeydown)
})

function movePage(idx: number, delta: number): void {
  const target = idx + delta
  if (target < 0 || target >= pages.value.length) return
  const arr = [...pages.value]
  const [item] = arr.splice(idx, 1)
  arr.splice(target, 0, item!)
  pages.value = arr
}

function handleDragStart(idx: number, ev: DragEvent): void {
  if (!isCbzEditable.value) return
  dragFromIdx.value = idx
  if (ev.dataTransfer) {
    ev.dataTransfer.effectAllowed = 'move'
    ev.dataTransfer.setData('text/plain', String(idx))
  }
}

function handleDragOver(idx: number, ev: DragEvent): void {
  if (dragFromIdx.value === null) return
  ev.preventDefault()
  dragOverIdx.value = idx
}

function handleDrop(targetIdx: number): void {
  const from = dragFromIdx.value
  dragFromIdx.value = null
  dragOverIdx.value = null
  if (from === null || from === targetIdx) return
  const arr = [...pages.value]
  const [item] = arr.splice(from, 1)
  arr.splice(targetIdx, 0, item!)
  pages.value = arr
}

function handleDragEnd(): void {
  dragFromIdx.value = null
  dragOverIdx.value = null
}

function setAsCover(idx: number): void {
  const arr = [...pages.value]
  const [item] = arr.splice(idx, 1)
  if (!item) return
  for (const p of arr) {
    if (p.page_type === 'FrontCover') p.page_type = 'Story'
  }
  item.deleted = false
  item.page_type = 'FrontCover'
  arr.unshift(item)
  pages.value = arr
  if (lightboxIdx.value === idx) lightboxIdx.value = 0
}

function toggleDoublePage(idx: number): void {
  const item = pages.value[idx]
  if (!item) return
  item.double_page = !item.double_page
}

function toggleDelete(idx: number): void {
  const item = pages.value[idx]
  if (!item) return
  item.deleted = !item.deleted
}

function restoreAll(): void {
  for (const p of pages.value) {
    p.deleted = false
  }
}

async function handleSave(): Promise<void> {
  if (!props.entry || !isCbzEditable.value) return
  const kept = pages.value.filter((p) => !p.deleted)
  if (kept.length === 0) {
    errorMsg.value = '至少需要保留一页漫画图像'
    return
  }
  saving.value = true
  errorMsg.value = ''
  statusMsg.value = ''
  try {
    const parsedMaxWidth = maxWidthStr.value ? Number(maxWidthStr.value) : null
    const res = await toolApi.saveComicPages(
      props.entry.root_id,
      props.entry.rel_path,
      kept.map((p) => ({
        entry_name: p.entry_name,
        page_type: p.page_type,
        double_page: p.double_page,
      })),
      reencode.value,
      jpegQuality.value,
      parsedMaxWidth,
    )
    lastReport.value = res.report
    const savedRatio =
      res.report.before_bytes > 0
        ? Math.round(
            ((res.report.before_bytes - res.report.after_bytes) / res.report.before_bytes) * 100,
          )
        : 0
    statusMsg.value = `已保存 ${res.report.page_count} 页（${formatBytes(res.report.before_bytes)} → ${formatBytes(res.report.after_bytes)}，${savedRatio >= 0 ? `减少 ${savedRatio}%` : `增加 ${Math.abs(savedRatio)}%`}）`
    emit('saved', res.detail)
    await loadPages({ preserveReport: true })
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <Teleport to="body">
    <div
      v-if="open && entry"
      class="fixed inset-0 z-[var(--z-modal)] flex flex-col overflow-hidden bg-[var(--ctp-canvas)] text-[var(--ctp-text)]"
    >
      <!-- Top Header -->
      <header
        class="flex h-14 shrink-0 items-center justify-between gap-3 border-b border-[var(--ctp-surface0)]/85 bg-[var(--ctp-shell)] px-4 select-none"
      >
        <div class="flex min-w-0 items-center gap-2.5">
          <div
            class="flex h-8 w-8 shrink-0 items-center justify-center rounded-xl bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]"
          >
            <LuImage class="h-4 w-4" />
          </div>
          <div class="min-w-0">
            <h2 class="truncate text-sm font-semibold">
              漫画页面管理与体积瘦身 · {{ entry.file_name }}
            </h2>
            <p class="truncate text-[11px] text-[var(--ctp-subtext0)]">
              共 {{ pages.length }} 页 · 保留 {{ keptCount }} 页
              <span v-if="deletedCount > 0" class="text-rose-500"> · 待删除 {{ deletedCount }} 页</span>
              · 当前体积 {{ formatBytes(totalSizeBytes) }}
              <span v-if="isCbzEditable" class="ml-1 opacity-80">· 支持拖拽调序与点击放大预览</span>
            </p>
          </div>
        </div>

        <div class="flex items-center gap-2">
          <button
            v-if="deletedCount > 0"
            type="button"
            class="inline-flex items-center gap-1.5 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-panel)] px-3 py-1.5 text-xs font-medium hover:bg-[var(--ctp-surface0)]/70"
            @click="restoreAll"
          >
            <LuRotateCcw class="h-3.5 w-3.5" />
            恢复全部
          </button>
          <button
            type="button"
            :disabled="saving || loading || !isCbzEditable"
            class="inline-flex items-center gap-1.5 rounded-xl bg-[var(--ctp-accent)] px-3.5 py-1.5 text-xs font-medium text-[var(--ctp-on-accent)] shadow-xs transition hover:opacity-90 disabled:opacity-50"
            @click="handleSave"
          >
            <LuLoader2 v-if="saving" class="h-3.5 w-3.5 animate-spin" />
            <LuSave v-else class="h-3.5 w-3.5" />
            保存重写 CBZ
          </button>
          <button
            type="button"
            title="关闭 (Esc)"
            class="rounded-xl p-1.5 text-[var(--ctp-subtext0)] hover:bg-[var(--ctp-surface0)]/70 hover:text-[var(--ctp-text)]"
            @click="emit('close')"
          >
            <LuX class="h-4 w-4" />
          </button>
        </div>
      </header>

      <!-- Optimize / Re-encode Bar -->
      <div
        class="flex flex-wrap items-center justify-between gap-3 border-b border-[var(--ctp-surface0)]/75 bg-[var(--ctp-panel)]/70 px-4 py-2.5 text-xs"
      >
        <div class="flex flex-wrap items-center gap-3">
          <span class="inline-flex items-center gap-1 font-medium text-[var(--ctp-subtext0)]">
            <LuSparkles class="h-3.5 w-3.5 text-[var(--ctp-accent)]" />
            保存时优化：
          </span>
          <div class="flex items-center gap-1.5">
            <span class="text-[var(--ctp-subtext0)]">图像编码</span>
            <div class="w-56">
              <AppSelect
                :model-value="reencode"
                :options="reencodeOptions"
                variant="compact"
                @update:model-value="(v) => (reencode = v as ComicImageReencodeFormat)"
              />
            </div>
          </div>
          <div v-if="reencode === 'jpeg'" class="flex items-center gap-2">
            <span class="text-[var(--ctp-subtext0)]">质量 {{ jpegQuality }}</span>
            <div class="w-28">
              <AppSlider v-model="jpegQuality" :min="60" :max="95" :step="2" aria-label="JPEG 质量" />
            </div>
          </div>
          <div class="flex items-center gap-1.5">
            <span class="text-[var(--ctp-subtext0)]">最大宽度</span>
            <div class="w-40">
              <AppSelect v-model="maxWidthStr" :options="maxWidthOptions" variant="compact" />
            </div>
          </div>
        </div>

        <div class="flex items-center gap-2 text-[11px] text-[var(--ctp-subtext0)]">
          <span v-if="!isCbzEditable" class="text-amber-500">当前为 {{ format.toUpperCase() }}（仅预览，重写支持 CBZ/ZIP）</span>
          <span v-else-if="statusMsg" class="inline-flex items-center gap-1 font-medium text-emerald-500">
            <LuCheck class="h-3.5 w-3.5" /> {{ statusMsg }}
          </span>
          <span v-else-if="errorMsg" class="font-medium text-rose-500">{{ errorMsg }}</span>
          <span v-else>保存时自动剔除 __MACOSX 垃圾、补零重命名并写入 ComicInfo.xml &lt;Pages&gt;</span>
        </div>
      </div>

      <!-- Page Grid -->
      <div class="min-h-0 flex-1 overflow-y-auto p-4">
        <div v-if="loading" class="flex h-64 items-center justify-center gap-2 text-sm text-[var(--ctp-subtext0)]">
          <LuLoader2 class="h-5 w-5 animate-spin" />
          正在读取漫画页面与尺寸信息…
        </div>

        <template v-else>
          <div
            class="grid grid-cols-2 gap-3 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6"
          >
            <div
              v-for="(page, idx) in visiblePages"
              :key="page.entry_name"
              :draggable="isCbzEditable"
              class="group relative flex flex-col overflow-hidden rounded-2xl border transition"
              :class="[
                dragOverIdx === idx && dragFromIdx !== idx
                  ? 'ring-2 ring-[var(--ctp-accent)] border-[var(--ctp-accent)] scale-[1.01]'
                  : page.deleted
                    ? 'border-rose-500/50 bg-rose-500/5 opacity-45'
                    : page.page_type === 'FrontCover'
                      ? 'border-[var(--ctp-accent)] bg-[var(--ctp-panel)] shadow-sm'
                      : 'border-[var(--ctp-surface0)] bg-[var(--ctp-panel)] hover:border-[var(--ctp-accent)]/60',
              ]"
              @dragstart="handleDragStart(idx, $event)"
              @dragover="handleDragOver(idx, $event)"
              @drop.prevent="handleDrop(idx)"
              @dragend="handleDragEnd"
            >
              <!-- Thumbnail -->
              <div
                class="relative aspect-[3/4] w-full cursor-zoom-in overflow-hidden bg-[var(--ctp-canvas)]"
                @click="lightboxIdx = idx"
              >
                <div
                  v-if="!loadedEntries.has(page.entry_name)"
                  class="absolute inset-0 animate-pulse bg-[var(--ctp-surface0)]/55"
                />
                <img
                  :src="toolApi.comicPageImageUrl(entry.root_id, entry.rel_path, page.entry_name, 320, imgVersion)"
                  :alt="page.entry_name"
                  loading="lazy"
                  class="h-full w-full object-contain transition-opacity duration-200"
                  :class="loadedEntries.has(page.entry_name) ? 'opacity-100' : 'opacity-0'"
                  @load="markLoaded(page.entry_name)"
                  @error="markLoaded(page.entry_name)"
                />
                <!-- Top badges -->
                <div class="absolute top-1.5 left-1.5 flex flex-wrap gap-1">
                  <span class="inline-flex items-center gap-0.5 rounded-md bg-black/70 px-1.5 py-0.5 text-[10px] font-semibold text-white">
                    <LuGripVertical v-if="isCbzEditable" class="h-2.5 w-2.5 opacity-75" />
                    #{{ idx + 1 }}
                  </span>
                  <span
                    v-if="page.page_type === 'FrontCover'"
                    class="rounded-md bg-[var(--ctp-accent)] px-1.5 py-0.5 text-[10px] font-semibold text-[var(--ctp-on-accent)]"
                  >
                    封面
                  </span>
                  <span
                    v-if="page.double_page"
                    class="rounded-md bg-indigo-600/90 px-1.5 py-0.5 text-[10px] font-semibold text-white"
                  >
                    双页跨页
                  </span>
                </div>
                <span
                  class="absolute right-1.5 bottom-1.5 rounded-lg bg-black/65 p-1 text-white opacity-0 transition group-hover:opacity-100"
                  title="放大预览"
                >
                  <LuMaximize2 class="h-3 w-3" />
                </span>
                <span
                  v-if="page.deleted"
                  class="absolute inset-0 flex items-center justify-center bg-black/50 text-xs font-semibold text-rose-300"
                >
                  待删除
                </span>
              </div>

              <!-- Meta + Actions -->
              <div class="flex flex-col gap-1.5 p-2">
                <div class="truncate text-[11px] font-medium" :title="page.entry_name">
                  {{ page.entry_name.split('/').pop() }}
                </div>
                <div class="flex items-center justify-between text-[10px] text-[var(--ctp-subtext0)]">
                  <span>{{ page.width && page.height ? `${page.width}×${page.height}` : '—' }}</span>
                  <span>{{ formatBytes(page.file_size) }}</span>
                </div>
                <div v-if="isCbzEditable" class="mt-0.5 flex items-center justify-between gap-1">
                  <div class="flex items-center gap-0.5">
                    <button
                      type="button"
                      title="前移一页"
                      :disabled="idx === 0"
                      class="rounded-lg p-1 text-[var(--ctp-subtext0)] hover:bg-[var(--ctp-surface0)] disabled:opacity-30"
                      @click="movePage(idx, -1)"
                    >
                      <LuArrowLeft class="h-3.5 w-3.5" />
                    </button>
                    <button
                      type="button"
                      title="后移一页"
                      :disabled="idx === pages.length - 1"
                      class="rounded-lg p-1 text-[var(--ctp-subtext0)] hover:bg-[var(--ctp-surface0)] disabled:opacity-30"
                      @click="movePage(idx, 1)"
                    >
                      <LuArrowRight class="h-3.5 w-3.5" />
                    </button>
                  </div>
                  <div class="flex items-center gap-0.5">
                    <button
                      type="button"
                      title="设为封面并移至首页"
                      class="rounded-lg p-1 hover:bg-[var(--ctp-surface0)]"
                      :class="page.page_type === 'FrontCover' ? 'text-[var(--ctp-accent)]' : 'text-[var(--ctp-subtext0)]'"
                      @click="setAsCover(idx)"
                    >
                      <LuBookOpen class="h-3.5 w-3.5" />
                    </button>
                    <button
                      type="button"
                      title="标记/取消双页跨页"
                      class="rounded-lg p-1 hover:bg-[var(--ctp-surface0)]"
                      :class="page.double_page ? 'text-indigo-500' : 'text-[var(--ctp-subtext0)]'"
                      @click="toggleDoublePage(idx)"
                    >
                      <LuColumns2 class="h-3.5 w-3.5" />
                    </button>
                    <button
                      type="button"
                      :title="page.deleted ? '恢复此页' : '删除此页（如广告页/空白页）'"
                      class="rounded-lg p-1 hover:bg-rose-500/15"
                      :class="page.deleted ? 'text-emerald-500' : 'text-rose-500'"
                      @click="toggleDelete(idx)"
                    >
                      <LuRotateCcw v-if="page.deleted" class="h-3.5 w-3.5" />
                      <LuTrash2 v-else class="h-3.5 w-3.5" />
                    </button>
                  </div>
                </div>
              </div>
            </div>
          </div>

          <div v-if="hasMorePages" class="mt-6 flex justify-center gap-2">
            <button
              type="button"
              class="rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-panel)] px-4 py-2 text-xs font-medium hover:border-[var(--ctp-accent)]"
              @click="visibleLimit += PAGE_BATCH_SIZE"
            >
              加载更多页面（剩余 {{ pages.length - visibleLimit }} 页）
            </button>
            <button
              type="button"
              class="rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-panel)] px-4 py-2 text-xs font-medium text-[var(--ctp-subtext0)] hover:border-[var(--ctp-accent)] hover:text-[var(--ctp-text)]"
              @click="visibleLimit = pages.length"
            >
              展开全部 {{ pages.length }} 页
            </button>
          </div>
        </template>
      </div>

      <!-- High-res Lightbox Preview -->
      <div
        v-if="lightboxIdx !== null && lightboxPage"
        class="fixed inset-0 z-[var(--z-fullscreen)] flex flex-col bg-black/88 text-white backdrop-blur-xs"
        @click.self="lightboxIdx = null"
      >
        <div class="flex items-center justify-between border-b border-white/10 px-4 py-3 text-xs">
          <div class="flex items-center gap-2">
            <span class="rounded-md bg-white/15 px-2 py-0.5 font-semibold">
              #{{ lightboxIdx + 1 }} / {{ pages.length }}
            </span>
            <span class="truncate font-medium">{{ lightboxPage.entry_name }}</span>
            <span class="text-white/60">
              {{ lightboxPage.width && lightboxPage.height ? `${lightboxPage.width}×${lightboxPage.height}` : '' }}
              · {{ formatBytes(lightboxPage.file_size) }}
            </span>
          </div>
          <div class="flex items-center gap-2">
            <template v-if="isCbzEditable">
              <button
                type="button"
                class="rounded-lg bg-white/10 px-2.5 py-1 hover:bg-white/20"
                @click="setAsCover(lightboxIdx)"
              >
                设为封面
              </button>
              <button
                type="button"
                class="rounded-lg px-2.5 py-1"
                :class="lightboxPage.double_page ? 'bg-indigo-500/80' : 'bg-white/10 hover:bg-white/20'"
                @click="toggleDoublePage(lightboxIdx)"
              >
                {{ lightboxPage.double_page ? '已标双页' : '标为双页' }}
              </button>
              <button
                type="button"
                class="rounded-lg px-2.5 py-1"
                :class="lightboxPage.deleted ? 'bg-emerald-600/80' : 'bg-rose-600/80 hover:bg-rose-600'"
                @click="toggleDelete(lightboxIdx)"
              >
                {{ lightboxPage.deleted ? '恢复此页' : '删除此页' }}
              </button>
            </template>
            <button
              type="button"
              class="rounded-lg bg-white/10 p-1.5 hover:bg-white/20"
              @click="lightboxIdx = null"
            >
              <LuX class="h-4 w-4" />
            </button>
          </div>
        </div>

        <div class="relative flex min-h-0 flex-1 items-center justify-center p-4" @click.self="lightboxIdx = null">
          <button
            v-if="lightboxIdx > 0"
            type="button"
            class="absolute left-4 rounded-full bg-black/60 p-2.5 text-white hover:bg-black/85"
            @click="lightboxIdx -= 1"
          >
            <LuChevronLeft class="h-5 w-5" />
          </button>
          <img
            :src="toolApi.comicPageImageUrl(entry.root_id, entry.rel_path, lightboxPage.entry_name, 1400, imgVersion)"
            :alt="lightboxPage.entry_name"
            class="max-h-full max-w-full rounded-lg object-contain shadow-2xl"
          />
          <button
            v-if="lightboxIdx < pages.length - 1"
            type="button"
            class="absolute right-4 rounded-full bg-black/60 p-2.5 text-white hover:bg-black/85"
            @click="lightboxIdx += 1"
          >
            <LuChevronRight class="h-5 w-5" />
          </button>
        </div>
      </div>
    </div>
  </Teleport>
</template>
