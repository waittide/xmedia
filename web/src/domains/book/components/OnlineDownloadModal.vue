<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import {
  LuBan,
  LuBookOpen,
  LuCheckCircle2,
  LuDownload,
  LuLoader2,
} from 'vue-icons-plus/lu'
import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import MdDialog from '@/components/ui/MdDialog.vue'
import MdLinearProgress from '@/components/ui/MdLinearProgress.vue'
import { useAuthStore } from '@/stores/auth'
import {
  formatDownloadSpeed,
  isOnlineDownloadJobActive,
  useOnlineDownloadsStore,
} from '@/stores/onlineDownloads'
import {
  buildOnlineDownloadFileUrl,
  onlineBookApi,
  type OnlineDownloadFormat,
  type OnlineDownloadJobDto,
} from '../onlineApi'

export interface OnlineDownloadTarget {
  bookId?: string
  sourceId: string
  sourceName: string
  bookName: string
  author: string
  bookUrl: string
  tocUrl: string
  coverUrl?: string | null
  intro?: string | null
  totalChapters?: number
}

const props = defineProps<{
  open: boolean
  target: OnlineDownloadTarget | null
  externalJob?: OnlineDownloadJobDto | null
}>()

const emit = defineEmits<{
  close: []
  jobUpdated: [job: OnlineDownloadJobDto]
}>()

const authStore = useAuthStore()
const onlineDownloadsStore = useOnlineDownloadsStore()
const router = useRouter()

const format = ref<OnlineDownloadFormat>('epub')
const useRange = ref(false)
const startChapter = ref<number | string>(1)
const endChapter = ref<number | string>(100)
const importToLibrary = ref(false)
const starting = ref(false)
const cancelling = ref(false)
const errorMsg = ref<string | null>(null)
const localJob = ref<OnlineDownloadJobDto | null>(null)

const activeJob = computed<OnlineDownloadJobDto | null>(() => {
  if (localJob.value) {
    if (props.externalJob && props.externalJob.id === localJob.value.id) {
      return props.externalJob
    }
    return (
      onlineDownloadsStore.jobs.find((job) => job.id === localJob.value?.id) ??
      localJob.value
    )
  }
  if (props.externalJob) {
    return (
      onlineDownloadsStore.jobs.find((job) => job.id === props.externalJob?.id) ??
      props.externalJob
    )
  }
  const target = props.target
  if (!target) return null
  return (
    onlineDownloadsStore.jobs.find(
      (job) =>
        isOnlineDownloadJobActive(job.status) &&
        job.source_id === target.sourceId &&
        job.book_name === target.bookName &&
        job.author === target.author,
    ) ?? null
  )
})

function isJobActive(status: OnlineDownloadJobDto['status']): boolean {
  return isOnlineDownloadJobActive(status)
}

watch(
  () => [
    props.open,
    props.target?.bookId ?? '',
    props.target?.sourceId ?? '',
    props.target?.bookUrl ?? '',
    props.target?.tocUrl ?? '',
  ] as const,
  ([isOpen, bookId, sourceId, bookUrl, tocUrl], prev) => {
    if (!isOpen || !props.target) return
    const wasOpen = prev?.[0] ?? false
    const sameTarget =
      wasOpen &&
      prev?.[1] === bookId &&
      prev?.[2] === sourceId &&
      prev?.[3] === bookUrl &&
      prev?.[4] === tocUrl
    if (sameTarget) return

    errorMsg.value = null
    localJob.value = null
    cancelling.value = false
    format.value = 'epub'
    useRange.value = false
    startChapter.value = 1
    endChapter.value = Math.max(1, props.target.totalChapters || 100)
    importToLibrary.value = authStore.isAdmin
  },
  { immediate: true },
)

function parsePositiveInt(val: number | string): number | null {
  if (typeof val === 'string' && val.trim() === '') return null
  const num = typeof val === 'number' ? val : Number(val)
  if (!Number.isFinite(num) || !Number.isInteger(num) || num < 1) return null
  return num
}

const progressPct = computed(() => {
  const j = activeJob.value
  if (!j || j.total_chapters <= 0) return 0
  return Math.min(100, Math.round((j.processed_chapters / j.total_chapters) * 100))
})

const speedLabel = computed(() => formatDownloadSpeed(activeJob.value?.chapters_per_sec))

async function handleStart(): Promise<void> {
  if (!props.target || starting.value) return
  errorMsg.value = null

  let startIdx: number | undefined
  let endIdx: number | undefined
  if (useRange.value) {
    const parsedStart = parsePositiveInt(startChapter.value)
    const parsedEnd = parsePositiveInt(endChapter.value)
    if (parsedStart === null || parsedEnd === null) {
      errorMsg.value = '请输入有效的起始章和结束章（必须为大于等于 1 的整数）'
      return
    }
    if (parsedStart > parsedEnd) {
      errorMsg.value = '起始章不能大于结束章'
      return
    }
    startIdx = parsedStart - 1
    endIdx = parsedEnd - 1
  }

  starting.value = true
  try {
    const job = await onlineBookApi.startDownload({
      book_id: props.target.bookId,
      source_id: props.target.sourceId,
      book_name: props.target.bookName,
      author: props.target.author,
      book_url: props.target.bookUrl,
      toc_url: props.target.tocUrl,
      cover_url: props.target.coverUrl ?? null,
      intro: props.target.intro ?? null,
      format: format.value,
      start_chapter: startIdx,
      end_chapter: endIdx,
      import_to_library: authStore.isAdmin && importToLibrary.value,
    })
    localJob.value = job
    onlineDownloadsStore.trackJob(job)
    emit('jobUpdated', job)
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    starting.value = false
  }
}

async function handleCancelJob(): Promise<void> {
  const j = activeJob.value
  if (!j || !isJobActive(j.status) || cancelling.value) return
  cancelling.value = true
  errorMsg.value = null
  try {
    const updated = await onlineBookApi.cancelDownload(j.id)
    localJob.value = updated
    onlineDownloadsStore.trackJob(updated)
    emit('jobUpdated', updated)
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    cancelling.value = false
  }
}

function openImportedBook(): void {
  const id = activeJob.value?.imported_book_id
  if (!id) return
  emit('close')
  void router.push(`/book/items/${id}`)
}
</script>

<template>
  <MdDialog
    :open="open && target !== null"
    :title="target ? `下载与打包 · ${target.bookName}` : ''"
    :subtitle="target ? `${target.author || '佚名'} · 来源：${target.sourceName}` : ''"
    max-width="max-w-lg"
    @close="emit('close')"
  >
    <template #icon>
      <LuDownload class="h-5 w-5" />
    </template>

    <div v-if="target" class="space-y-4 py-1 text-xs">
      <div
        v-if="errorMsg"
        class="rounded-2xl bg-[var(--md-sys-color-error-container)] px-3.5 py-2.5 text-[var(--md-sys-color-on-error-container)]"
      >
        {{ errorMsg }}
      </div>

      <template v-if="!activeJob">
        <div>
          <div class="mb-1.5 font-medium text-[var(--md-sys-color-on-surface-variant)]">
            导出格式
          </div>
          <div class="grid grid-cols-2 gap-2">
            <button
              type="button"
              class="md-state rounded-2xl border px-3 py-2.5 text-left transition"
              :class="
                format === 'epub'
                  ? 'border-[var(--md-sys-color-primary)] bg-[var(--md-sys-color-primary-container)]/50 text-[var(--md-sys-color-on-surface)]'
                  : 'border-[var(--md-sys-color-outline-variant)]/70 bg-[var(--md-sys-color-surface-container)] text-[var(--md-sys-color-on-surface-variant)]'
              "
              @click="format = 'epub'"
            >
              <div class="font-semibold">EPUB 3.0（推荐）</div>
              <div class="mt-0.5 text-[10px] opacity-80">含封面、目录与标准排版</div>
            </button>
            <button
              type="button"
              class="md-state rounded-2xl border px-3 py-2.5 text-left transition"
              :class="
                format === 'txt'
                  ? 'border-[var(--md-sys-color-primary)] bg-[var(--md-sys-color-primary-container)]/50 text-[var(--md-sys-color-on-surface)]'
                  : 'border-[var(--md-sys-color-outline-variant)]/70 bg-[var(--md-sys-color-surface-container)] text-[var(--md-sys-color-on-surface-variant)]'
              "
              @click="format = 'txt'"
            >
              <div class="font-semibold">纯文本 TXT</div>
              <div class="mt-0.5 text-[10px] opacity-80">UTF-8 编码，轻量通用</div>
            </button>
          </div>
        </div>

        <div class="space-y-2 rounded-2xl bg-[var(--md-sys-color-surface-container)] p-3.5">
          <AppCheckbox v-model="useRange" label="仅下载指定章节区间（默认下载全本）" />
          <div v-if="useRange" class="grid grid-cols-2 gap-2.5 pt-1">
            <label class="block">
              <span class="mb-1 block text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
                起始章（从 1 开始）
              </span>
              <input
                v-model.number="startChapter"
                type="number"
                min="1"
                class="md-input h-9 w-full text-xs"
              />
            </label>
            <label class="block">
              <span class="mb-1 block text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
                结束章
              </span>
              <input
                v-model.number="endChapter"
                type="number"
                min="1"
                class="md-input h-9 w-full text-xs"
              />
            </label>
          </div>
        </div>

        <div
          v-if="authStore.isAdmin"
          class="rounded-2xl bg-[var(--md-sys-color-surface-container)] p-3.5"
        >
          <AppCheckbox
            v-model="importToLibrary"
            label="打包后自动入库"
          />
        </div>
      </template>

      <!-- Live Job Progress -->
      <div v-else class="space-y-3 rounded-2xl bg-[var(--md-sys-color-surface-container)] p-4">
        <div class="flex items-center justify-between gap-2">
          <span class="flex items-center gap-1.5 font-semibold text-[var(--md-sys-color-on-surface)]">
            <LuLoader2
              v-if="activeJob.status === 'pending' || activeJob.status === 'running'"
              class="h-4 w-4 animate-spin text-[var(--md-sys-color-primary)]"
            />
            <LuCheckCircle2
              v-else-if="activeJob.status === 'completed'"
              class="h-4 w-4 text-[var(--md-sys-color-success)]"
            />
            <LuBan
              v-else-if="activeJob.status === 'cancelled'"
              class="h-4 w-4 text-[var(--md-sys-color-on-surface-variant)]"
            />
            <span>
              {{
                activeJob.status === 'completed'
                  ? '打包完成'
                  : activeJob.status === 'cancelled'
                    ? '已取消'
                    : activeJob.status === 'failed'
                      ? '打包失败'
                      : '正在抓取章节…'
              }}
            </span>
          </span>
          <span class="font-mono text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
            {{ activeJob.processed_chapters }} / {{ activeJob.total_chapters || '?' }} 章
            <template v-if="isJobActive(activeJob.status) && speedLabel">
              · {{ speedLabel }}
            </template>
          </span>
        </div>

        <MdLinearProgress
          :value="activeJob.status === 'completed' ? 100 : activeJob.total_chapters > 0 ? progressPct : null"
          :indeterminate="isJobActive(activeJob.status) && activeJob.total_chapters <= 0"
          :tone="
            activeJob.status === 'cancelled'
              ? 'neutral'
              : activeJob.status === 'failed'
                ? 'error'
                : activeJob.status === 'completed'
                  ? 'success'
                  : 'primary'
          "
          :aria-label="target.bookName"
        />

        <p
          v-if="activeJob.current_chapter_title"
          class="truncate text-[11px] text-[var(--md-sys-color-on-surface-variant)]"
        >
          {{ activeJob.current_chapter_title }}
        </p>
        <p v-if="activeJob.failed_chapters > 0" class="text-[11px] text-[var(--md-sys-color-warning)]">
          其中 {{ activeJob.failed_chapters }} 章抓取失败（已以占位提示保留）
        </p>
        <p v-if="activeJob.error" class="text-[11px] text-[var(--md-sys-color-error)]">
          {{ activeJob.error }}
        </p>

        <div
          v-if="activeJob.status === 'completed'"
          class="flex flex-wrap items-center gap-2 pt-2"
        >
          <a
            :href="buildOnlineDownloadFileUrl(activeJob.id)"
            :download="activeJob.output_file_name || undefined"
            class="md-btn md-btn-sm md-btn-filled md-state"
          >
            <LuDownload class="h-3.5 w-3.5" />
            <span>下载 {{ activeJob.output_file_name }}</span>
          </a>

          <button
            v-if="activeJob.imported_book_id"
            type="button"
            class="md-btn md-btn-sm md-btn-tonal md-state"
            @click="openImportedBook"
          >
            <LuBookOpen class="h-3.5 w-3.5" />
            <span>打开已入库书籍</span>
          </button>
        </div>
      </div>
    </div>

    <template #footer>
      <div class="flex w-full items-center justify-end gap-2">
        <button
          v-if="activeJob && isJobActive(activeJob.status)"
          type="button"
          :disabled="cancelling"
          class="md-btn md-btn-sm md-btn-outlined md-state mr-auto !border-[var(--md-sys-color-error)] !text-[var(--md-sys-color-error)]"
          @click="handleCancelJob"
        >
          <LuLoader2 v-if="cancelling" class="h-3.5 w-3.5 animate-spin" />
          <LuBan v-else class="h-3.5 w-3.5" />
          <span>{{ cancelling ? '取消中…' : '取消任务' }}</span>
        </button>
        <button
          type="button"
          class="md-btn md-btn-sm md-btn-text md-state"
          @click="emit('close')"
        >
          {{
            activeJob && isJobActive(activeJob.status)
              ? '后台运行 / 关闭'
              : activeJob
                ? '关闭'
                : '取消'
          }}
        </button>
        <button
          v-if="!activeJob"
          type="button"
          :disabled="starting"
          class="md-btn md-btn-sm md-btn-filled md-state"
          @click="handleStart"
        >
          <LuLoader2 v-if="starting" class="h-3.5 w-3.5 animate-spin" />
          <LuDownload v-else class="h-3.5 w-3.5" />
          <span>开始抓取并打包</span>
        </button>
      </div>
    </template>
  </MdDialog>
</template>
