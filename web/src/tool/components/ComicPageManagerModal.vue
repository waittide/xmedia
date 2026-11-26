<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
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
} from 'vue-icons-plus/lu'
import AppSlider from '@/components/ui/AppSlider.vue'
import MdButton from '@/components/ui/MdButton.vue'
import MdDialog from '@/components/ui/MdDialog.vue'
import MdIconButton from '@/components/ui/MdIconButton.vue'
import MdSelect from '@/components/ui/MdSelect.vue'
import { useDialogStore } from '@/stores/dialog'
import { formatBytes } from '@/utils/format'
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

const { t } = useI18n()

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

const PAGE_BATCH_SIZE = 48
const visibleLimit = ref(PAGE_BATCH_SIZE)
const loadedEntries = ref<Set<string>>(new Set())

const dragFromIdx = ref<number | null>(null)
const dragOverIdx = ref<number | null>(null)

const lightboxIdx = ref<number | null>(null)
const lightboxPage = computed(() =>
  lightboxIdx.value !== null ? (pages.value[lightboxIdx.value] ?? null) : null,
)

const reencodeOptions = computed(() => [
  { value: 'keep', label: t('tool.modals.comicPages.encKeep') },
  { value: 'jpeg', label: t('tool.modals.comicPages.encJpeg') },
  { value: 'webp', label: t('tool.modals.comicPages.encWebp') },
])

const maxWidthOptions = computed(() => [
  { value: '', label: t('tool.modals.comicPages.widthNone') },
  { value: '2000', label: t('tool.modals.comicPages.width2000') },
  { value: '1600', label: t('tool.modals.comicPages.width1600') },
  { value: '1280', label: t('tool.modals.comicPages.width1280') },
])

const isCbzEditable = computed(() => format.value === 'cbz' || format.value === 'zip')
const keptCount = computed(() => pages.value.filter((p) => !p.deleted).length)
const deletedCount = computed(() => pages.value.filter((p) => p.deleted).length)
const visiblePages = computed(() => pages.value.slice(0, visibleLimit.value))
const hasMorePages = computed(() => pages.value.length > visibleLimit.value)

const dialogSubtitle = computed(() => {
  const parts = [
    t('tool.modals.comicPages.totalPages', { count: pages.value.length }),
    t('tool.modals.comicPages.keptPages', { count: keptCount.value }),
  ]
  if (deletedCount.value > 0) {
    parts.push(t('tool.modals.comicPages.pendingDeletePages', { count: deletedCount.value }))
  }
  parts.push(t('tool.modals.comicPages.currentSize', { size: formatBytes(totalSizeBytes.value) }))
  if (isCbzEditable.value) parts.push(t('tool.modals.comicPages.cbzHint'))
  return parts.join(' · ')
})

function markLoaded(entryName: string): void {
  if (loadedEntries.value.has(entryName)) return
  const next = new Set(loadedEntries.value)
  next.add(entryName)
  loadedEntries.value = next
}

const dialogStore = useDialogStore()
let pagesLoadSeq = 0
const initialSnapshot = ref('')

function serializePagesState(): string {
  return JSON.stringify(
    pages.value.map((p) => [p.entry_name, p.page_type, p.double_page, p.deleted]),
  )
}

const isDirty = computed(
  () => initialSnapshot.value !== '' && serializePagesState() !== initialSnapshot.value,
)

async function loadPages(options?: { preserveReport?: boolean }): Promise<void> {
  if (!props.entry) return
  const seq = ++pagesLoadSeq
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
    if (seq !== pagesLoadSeq || !props.open) return
    format.value = snap.format
    totalSizeBytes.value = snap.total_size_bytes
    pages.value = snap.pages.map((p) => ({ ...p, deleted: false }))
    initialSnapshot.value = serializePagesState()
    imgVersion.value += 1
  } catch (err) {
    if (seq !== pagesLoadSeq) return
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    if (seq === pagesLoadSeq) {
      loading.value = false
    }
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

function handleLightboxKeydown(ev: KeyboardEvent): void {
  if (!props.open || lightboxIdx.value === null) return
  if (ev.key === 'ArrowLeft') {
    ev.preventDefault()
    if (lightboxIdx.value > 0) lightboxIdx.value -= 1
  } else if (ev.key === 'ArrowRight') {
    ev.preventDefault()
    if (lightboxIdx.value < pages.value.length - 1) lightboxIdx.value += 1
  }
}

watch(
  () => lightboxIdx.value !== null,
  (isOpen) => {
    if (isOpen) {
      window.addEventListener('keydown', handleLightboxKeydown)
    } else {
      window.removeEventListener('keydown', handleLightboxKeydown)
    }
  },
)

onBeforeUnmount(() => {
  window.removeEventListener('keydown', handleLightboxKeydown)
})

async function handleDialogClose(): Promise<void> {
  if (saving.value) return
  if (lightboxIdx.value !== null) {
    lightboxIdx.value = null
    return
  }
  if (isDirty.value) {
    const ok = await dialogStore.confirm({
      title: t('tool.modals.comicPages.discardTitle'),
      message: t('tool.modals.comicPages.discardMsg'),
      confirmText: t('tool.modals.comicPages.discardConfirm'),
      cancelText: t('tool.inspector.continueEditing'),
      danger: true,
    })
    if (!ok) return
  }
  emit('close')
}

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
    errorMsg.value = t('tool.modals.comicPages.keepAtLeastOne')
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
    statusMsg.value = t('tool.modals.comicPages.savedReport', {
      pages: res.report.page_count,
      before: formatBytes(res.report.before_bytes),
      after: formatBytes(res.report.after_bytes),
    })
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
  <MdDialog
    :open="Boolean(open && entry)"
    :title="
      entry
        ? t('tool.modals.comicPages.titleWithFile', { name: entry.file_name })
        : t('tool.modals.comicPages.title')
    "
    :subtitle="dialogSubtitle"
    fullscreen
    :close-on-backdrop="!saving && lightboxIdx === null"
    @close="handleDialogClose"
  >
    <template #icon>
      <LuImage class="h-5 w-5" aria-hidden="true" />
    </template>

    <template #headerActions>
      <MdButton
        v-if="deletedCount > 0"
        variant="outlined"
        :icon="LuRotateCcw"
        @click="restoreAll"
      >
        {{ t('tool.modals.comicPages.restoreAll') }}
      </MdButton>
      <MdButton
        variant="filled"
        :icon="LuSave"
        :loading="saving"
        :disabled="loading || !isCbzEditable"
        @click="handleSave"
      >
        {{ t('tool.modals.comicPages.saveCbz') }}
      </MdButton>
    </template>

    <div v-if="entry" class="space-y-4 py-1">
      <!-- Optimize / Re-encode Bar -->
      <div
        class="text-body-small flex flex-wrap items-center justify-between gap-3 rounded-corner-lg bg-surface-container-low px-4 py-2.5"
      >
        <div class="flex flex-wrap items-center gap-3">
          <span class="inline-flex items-center gap-1 font-medium text-on-surface-variant">
            <LuSparkles class="h-3.5 w-3.5 text-primary" aria-hidden="true" />
            {{ t('tool.modals.comicPages.optimizeLabel') }}
          </span>
          <div class="flex items-center gap-1.5">
            <span class="text-on-surface-variant">{{ t('tool.modals.comicPages.imageEncoding') }}</span>
            <div class="w-56">
              <MdSelect
                :model-value="reencode"
                :options="reencodeOptions"
                variant="compact"
                @update:model-value="(v) => (reencode = v as ComicImageReencodeFormat)"
              />
            </div>
          </div>
          <div v-if="reencode === 'jpeg'" class="flex items-center gap-2">
            <span class="text-on-surface-variant">
              {{ t('tool.modals.comicPages.quality', { q: jpegQuality }) }}
            </span>
            <div class="w-28">
              <AppSlider
                v-model="jpegQuality"
                :min="60"
                :max="95"
                :step="2"
                :aria-label="t('tool.modals.comicPages.quality', { q: jpegQuality })"
              />
            </div>
          </div>
          <div class="flex items-center gap-1.5">
            <span class="text-on-surface-variant">{{ t('tool.modals.comicPages.maxWidth') }}</span>
            <div class="w-40">
              <MdSelect v-model="maxWidthStr" :options="maxWidthOptions" variant="compact" />
            </div>
          </div>
        </div>

        <div class="flex items-center gap-2 text-on-surface-variant">
          <span v-if="!isCbzEditable" class="text-warning">
            {{ t('tool.modals.comicPages.readOnlyFormat', { fmt: format.toUpperCase() }) }}
          </span>
          <span
            v-else-if="statusMsg"
            class="inline-flex items-center gap-1 font-medium text-success"
          >
            <LuCheck class="h-3.5 w-3.5" aria-hidden="true" /> {{ statusMsg }}
          </span>
          <span v-else-if="errorMsg" class="font-medium text-error">{{ errorMsg }}</span>
          <span v-else>{{ t('tool.modals.comicPages.cbzAutoCleanHint') }}</span>
        </div>
      </div>

      <!-- Page Grid -->
      <div
        v-if="loading"
        class="text-body-medium flex h-64 items-center justify-center gap-2 text-on-surface-variant"
      >
        <LuLoader2 class="h-5 w-5 animate-spin text-primary" aria-hidden="true" />
        <span>{{ t('tool.modals.comicPages.loadingPages') }}</span>
      </div>

      <template v-else>
        <div
          class="grid grid-cols-2 gap-3 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6"
        >
          <div
            v-for="(page, idx) in visiblePages"
            :key="page.entry_name"
            :draggable="isCbzEditable"
            class="group relative flex flex-col overflow-hidden rounded-corner-lg transition"
            :class="[
              dragOverIdx === idx && dragFromIdx !== idx
                ? 'scale-[1.01] ring-2 ring-primary'
                : page.deleted
                  ? 'bg-error-container/30 opacity-45'
                  : page.page_type === 'FrontCover'
                    ? 'bg-surface-container-low shadow-elevation-1 ring-1 ring-primary'
                    : 'bg-surface-container-low hover:bg-surface-container',
            ]"
            @dragstart="handleDragStart(idx, $event)"
            @dragover="handleDragOver(idx, $event)"
            @drop.prevent="handleDrop(idx)"
            @dragend="handleDragEnd"
          >
            <!-- Thumbnail -->
            <button
              type="button"
              :aria-label="
                t('tool.modals.comicPages.previewPageAria', {
                  num: idx + 1,
                  name: page.entry_name,
                })
              "
              class="relative block aspect-[3/4] w-full cursor-zoom-in overflow-hidden bg-surface-container text-left outline-none focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-secondary"
              @click="lightboxIdx = idx"
            >
              <div
                v-if="!loadedEntries.has(page.entry_name)"
                class="absolute inset-0 animate-pulse bg-surface-container-high"
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
                <span class="text-label-small inline-flex items-center gap-0.5 rounded-corner-xs bg-scrim/75 px-1.5 py-0.5 text-white">
                  <LuGripVertical v-if="isCbzEditable" class="h-2.5 w-2.5 opacity-75" aria-hidden="true" />
                  #{{ idx + 1 }}
                </span>
                <span
                  v-if="page.page_type === 'FrontCover'"
                  class="text-label-small rounded-corner-xs bg-primary px-1.5 py-0.5 text-on-primary"
                >
                  {{ t('tool.modals.comicPages.coverBadge') }}
                </span>
                <span
                  v-if="page.double_page"
                  class="text-label-small rounded-corner-xs bg-tertiary-container px-1.5 py-0.5 text-on-tertiary-container"
                >
                  {{ t('tool.modals.comicPages.doublePageBadge') }}
                </span>
              </div>
              <span
                class="absolute right-1.5 bottom-1.5 rounded-corner-xs bg-scrim/70 p-1 text-white opacity-0 transition group-hover:opacity-100"
                aria-hidden="true"
              >
                <LuMaximize2 class="h-3 w-3" />
              </span>
              <span
                v-if="page.deleted"
                class="text-label-large absolute inset-0 flex items-center justify-center bg-scrim/65 text-error-container"
              >
                {{ t('tool.modals.comicPages.pendingDeleteBadge') }}
              </span>
            </button>

            <!-- Meta + Actions -->
            <div class="flex flex-col gap-1 p-2">
              <div class="text-label-medium truncate" :title="page.entry_name">
                {{ page.entry_name.split('/').pop() }}
              </div>
              <div class="text-label-small flex items-center justify-between text-on-surface-variant">
                <span>{{ page.width && page.height ? `${page.width}×${page.height}` : '—' }}</span>
                <span>{{ formatBytes(page.file_size) }}</span>
              </div>
              <div v-if="isCbzEditable" class="mt-0.5 flex items-center justify-between gap-0.5">
                <div class="flex items-center">
                  <MdIconButton
                    size="sm"
                    :disabled="idx === 0"
                    :icon="LuArrowLeft"
                    :tooltip="t('tool.modals.comicPages.movePrev')"
                    :aria-label="t('tool.modals.comicPages.movePrev')"
                    @click="movePage(idx, -1)"
                  />
                  <MdIconButton
                    size="sm"
                    :disabled="idx === pages.length - 1"
                    :icon="LuArrowRight"
                    :tooltip="t('tool.modals.comicPages.moveNext')"
                    :aria-label="t('tool.modals.comicPages.moveNext')"
                    @click="movePage(idx, 1)"
                  />
                </div>
                <div class="flex items-center">
                  <MdIconButton
                    size="sm"
                    :selected="page.page_type === 'FrontCover'"
                    :icon="LuBookOpen"
                    :tooltip="t('tool.modals.comicPages.setAsCover')"
                    :aria-label="t('tool.modals.comicPages.setAsCover')"
                    @click="setAsCover(idx)"
                  />
                  <MdIconButton
                    size="sm"
                    :selected="page.double_page"
                    :icon="LuColumns2"
                    :tooltip="t('tool.modals.comicPages.toggleDouble')"
                    :aria-label="t('tool.modals.comicPages.toggleDouble')"
                    @click="toggleDoublePage(idx)"
                  />
                  <MdIconButton
                    size="sm"
                    :icon="page.deleted ? LuRotateCcw : LuTrash2"
                    :tooltip="
                      page.deleted
                        ? t('tool.modals.comicPages.restorePage')
                        : t('tool.modals.comicPages.deletePage')
                    "
                    :aria-label="
                      page.deleted
                        ? t('tool.modals.comicPages.restorePage')
                        : t('tool.modals.comicPages.deletePage')
                    "
                    @click="toggleDelete(idx)"
                  />
                </div>
              </div>
            </div>
          </div>
        </div>

        <div v-if="hasMorePages" class="mt-6 flex justify-center gap-2">
          <MdButton variant="tonal" @click="visibleLimit += PAGE_BATCH_SIZE">
            {{ t('tool.modals.comicPages.loadMorePages', { count: pages.length - visibleLimit }) }}
          </MdButton>
          <MdButton variant="text" @click="visibleLimit = pages.length">
            {{ t('tool.modals.comicPages.expandAllPages', { count: pages.length }) }}
          </MdButton>
        </div>
      </template>

      <!-- High-res Lightbox Preview via Fullscreen MdDialog -->
      <MdDialog
        :open="lightboxIdx !== null && Boolean(lightboxPage)"
        fullscreen
        no-body-padding
        :title="
          lightboxPage && lightboxIdx !== null
            ? `#${lightboxIdx + 1} / ${pages.length} · ${lightboxPage.entry_name}`
            : ''
        "
        :subtitle="
          lightboxPage
            ? `${
                lightboxPage.width && lightboxPage.height
                  ? `${lightboxPage.width}×${lightboxPage.height} · `
                  : ''
              }${formatBytes(lightboxPage.file_size)}`
            : ''
        "
        @close="lightboxIdx = null"
      >
        <template v-if="lightboxPage && lightboxIdx !== null && isCbzEditable" #headerActions>
          <MdButton variant="tonal" @click="setAsCover(lightboxIdx)">
            {{ t('tool.modals.comicPages.setAsCoverBtn') }}
          </MdButton>
          <MdButton
            :variant="lightboxPage.double_page ? 'filled' : 'outlined'"
            @click="toggleDoublePage(lightboxIdx)"
          >
            {{
              lightboxPage.double_page
                ? t('tool.modals.comicPages.markedDouble')
                : t('tool.modals.comicPages.markDouble')
            }}
          </MdButton>
          <MdButton
            :variant="lightboxPage.deleted ? 'tonal' : 'outlined'"
            :tone="lightboxPage.deleted ? 'primary' : 'error'"
            @click="toggleDelete(lightboxIdx)"
          >
            {{
              lightboxPage.deleted
                ? t('tool.modals.comicPages.restorePageBtn')
                : t('tool.modals.comicPages.deletePageBtn')
            }}
          </MdButton>
        </template>

        <div
          v-if="lightboxPage && lightboxIdx !== null"
          class="relative flex min-h-0 flex-1 items-center justify-center bg-surface-container-lowest p-4"
          @click.self="lightboxIdx = null"
        >
          <MdIconButton
            v-if="lightboxIdx > 0"
            variant="tonal"
            class="absolute left-4"
            :icon="LuChevronLeft"
            :aria-label="t('tool.modals.comicPages.prevPage')"
            @click="lightboxIdx -= 1"
          />
          <img
            :src="toolApi.comicPageImageUrl(entry.root_id, entry.rel_path, lightboxPage.entry_name, 1400, imgVersion)"
            :alt="lightboxPage.entry_name"
            class="max-h-full max-w-full rounded-corner-md object-contain shadow-elevation-3"
          />
          <MdIconButton
            v-if="lightboxIdx < pages.length - 1"
            variant="tonal"
            class="absolute right-4"
            :icon="LuChevronRight"
            :aria-label="t('tool.modals.comicPages.nextPage')"
            @click="lightboxIdx += 1"
          />
        </div>
      </MdDialog>
    </div>
  </MdDialog>
</template>
