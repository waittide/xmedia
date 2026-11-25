<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import {
  LuBan,
  LuBookOpen,
  LuCheckCircle2,
  LuDownload,
} from 'vue-icons-plus/lu'
import {
  AppCheckbox,
  MdButton,
  MdCircularProgress,
  MdDialog,
  MdLinearProgress,
  MdRadio,
  MdTextField,
} from '@/components/ui'
import { useBookI18n as useI18n } from '@/domains/book/i18n'
import { itemRoute } from '@/sources'
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

const { t } = useI18n()
const authStore = useAuthStore()
const onlineDownloadsStore = useOnlineDownloadsStore()
const router = useRouter()

const format = ref<OnlineDownloadFormat>('epub')
const formatGroupRef = ref<HTMLElement | null>(null)
const formatOptions = computed<
  Array<{ value: OnlineDownloadFormat; title: string; desc: string }>
>(() => [
  {
    value: 'epub',
    title: t('online.formatEpubTitle'),
    desc: t('online.formatEpubDesc'),
  },
  {
    value: 'txt',
    title: t('online.formatTxtTitle'),
    desc: t('online.formatTxtDesc'),
  },
])

function handleFormatKeydown(event: KeyboardEvent, currentIdx: number): void {
  const keys = ['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', 'Home', 'End']
  if (!keys.includes(event.key)) return
  event.preventDefault()
  const opts = formatOptions.value
  let nextIdx = currentIdx
  if (event.key === 'Home') {
    nextIdx = 0
  } else if (event.key === 'End') {
    nextIdx = opts.length - 1
  } else {
    const delta = event.key === 'ArrowRight' || event.key === 'ArrowDown' ? 1 : -1
    nextIdx = (currentIdx + delta + opts.length) % opts.length
  }
  const nextOpt = opts[nextIdx]
  if (!nextOpt) return
  format.value = nextOpt.value
  const radios = formatGroupRef.value?.querySelectorAll<HTMLButtonElement>('[role="radio"]')
  radios?.[nextIdx]?.focus()
}
const useRange = ref(false)
const startChapter = ref('1')
const endChapter = ref('100')
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
    startChapter.value = '1'
    endChapter.value = String(Math.max(1, props.target.totalChapters || 100))
    importToLibrary.value = authStore.isAdmin
  },
  { immediate: true },
)

function parsePositiveInt(val: string): number | null {
  const trimmed = val.trim()
  if (!trimmed) return null
  const num = Number(trimmed)
  if (!Number.isFinite(num) || !Number.isInteger(num) || num < 1) return null
  return num
}

const progressPct = computed(() => {
  const j = activeJob.value
  if (!j || j.total_chapters <= 0) return 0
  return Math.min(100, Math.round((j.processed_chapters / j.total_chapters) * 100))
})

const speedLabel = computed(() => formatDownloadSpeed(activeJob.value?.chapters_per_sec))

const statusLabel = computed(() => {
  const st = activeJob.value?.status
  if (st === 'completed') return t('online.statusCompleted')
  if (st === 'cancelled') return t('online.statusCancelled')
  if (st === 'failed') return t('online.statusFailed')
  return t('online.statusRunning')
})

async function handleStart(): Promise<void> {
  if (!props.target || starting.value) return
  errorMsg.value = null

  let startIdx: number | undefined
  let endIdx: number | undefined
  if (useRange.value) {
    const parsedStart = parsePositiveInt(startChapter.value)
    const parsedEnd = parsePositiveInt(endChapter.value)
    if (parsedStart === null || parsedEnd === null) {
      errorMsg.value = t('online.rangeInvalidErr')
      return
    }
    if (parsedStart > parsedEnd) {
      errorMsg.value = t('online.rangeOrderErr')
      return
    }
    startIdx = parsedStart - 1
    endIdx = parsedEnd - 1
  }

  starting.value = true
  try {
    const job = await onlineBookApi.startDownload({
      book_id: authStore.isAdmin ? props.target.bookId : undefined,
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
  void router.push(itemRoute({ scope: 'local', domain: 'book', id }))
}
</script>

<template>
  <MdDialog
    :open="open && target !== null"
    :title="target ? t('online.downloadTitle', { title: target.bookName }) : ''"
    :subtitle="
      target
        ? t('online.downloadSubtitle', {
            author: target.author || t('catalog.unknownAuthor'),
            source: target.sourceName,
          })
        : ''
    "
    size="sm"
    @close="emit('close')"
  >
    <template #icon>
      <LuDownload class="h-5 w-5" aria-hidden="true" />
    </template>

    <div v-if="target" class="space-y-4 py-1">
      <div
        v-if="errorMsg"
        role="alert"
        class="text-body-small rounded-corner-md bg-error-container px-3.5 py-2.5 text-on-error-container"
      >
        {{ errorMsg }}
      </div>

      <template v-if="!activeJob">
        <div class="space-y-2">
          <div class="text-label-medium text-on-surface-variant">
            {{ t('online.exportFormat') }}
          </div>
          <div
            ref="formatGroupRef"
            role="radiogroup"
            :aria-label="t('online.exportFormat')"
            class="grid grid-cols-1 gap-2 sm:grid-cols-2"
          >
            <div
              v-for="(opt, idx) in formatOptions"
              :key="opt.value"
              class="flex cursor-pointer items-start gap-2.5 rounded-corner-md px-3.5 py-3 transition-colors duration-150 ease-standard"
              :class="
                format === opt.value
                  ? 'bg-primary-container/50 text-on-surface'
                  : 'bg-surface-container text-on-surface hover:bg-surface-container-high'
              "
              @click="format = opt.value"
            >
              <MdRadio
                :checked="format === opt.value"
                :aria-label="opt.title"
                :tabindex="format === opt.value ? 0 : -1"
                class="mt-0.5"
                @select="format = opt.value"
                @keydown="handleFormatKeydown($event, idx)"
              />
              <div class="min-w-0 flex-1">
                <div class="text-label-large text-on-surface">{{ opt.title }}</div>
                <p class="text-body-small mt-1 text-on-surface-variant">
                  {{ opt.desc }}
                </p>
              </div>
            </div>
          </div>
        </div>

        <div class="space-y-3 rounded-corner-md bg-surface-container p-3.5">
          <AppCheckbox v-model="useRange" :label="t('online.rangeToggle')" />
          <div v-if="useRange" class="grid grid-cols-2 gap-3 pt-1">
            <MdTextField
              v-model="startChapter"
              type="number"
              size="sm"
              min="1"
              :label="t('online.startChapter')"
            />
            <MdTextField
              v-model="endChapter"
              type="number"
              size="sm"
              min="1"
              :label="t('online.endChapter')"
            />
          </div>
        </div>

        <div
          v-if="authStore.isAdmin"
          class="rounded-corner-md bg-surface-container p-3.5"
        >
          <AppCheckbox v-model="importToLibrary" :label="t('online.autoImportToggle')" />
        </div>
      </template>

      <!-- Live Job Progress -->
      <div v-else class="space-y-3 rounded-corner-md bg-surface-container p-4">
        <div class="flex items-center justify-between gap-2">
          <span class="text-title-small flex items-center gap-2 text-on-surface">
            <MdCircularProgress
              v-if="activeJob.status === 'pending' || activeJob.status === 'running'"
              indeterminate
              :size="18"
              :stroke-width="2.5"
            />
            <LuCheckCircle2
              v-else-if="activeJob.status === 'completed'"
              class="h-4 w-4 text-success"
              aria-hidden="true"
            />
            <LuBan
              v-else-if="activeJob.status === 'cancelled'"
              class="h-4 w-4 text-on-surface-variant"
              aria-hidden="true"
            />
            <span>{{ statusLabel }}</span>
          </span>
          <span class="text-label-small font-mono tabular-nums text-on-surface-variant">
            {{
              t('online.chaptersProgress', {
                processed: activeJob.processed_chapters,
                total: activeJob.total_chapters || '?',
              })
            }}
            <template v-if="isJobActive(activeJob.status) && speedLabel">
              · {{ speedLabel }}
            </template>
          </span>
        </div>

        <MdLinearProgress
          :value="
            activeJob.status === 'completed'
              ? 100
              : activeJob.total_chapters > 0
                ? progressPct
                : null
          "
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
          class="text-body-small truncate text-on-surface-variant"
        >
          {{ activeJob.current_chapter_title }}
        </p>
        <p v-if="activeJob.failed_chapters > 0" class="text-body-small text-warning">
          {{ t('online.failedChaptersHint', { count: activeJob.failed_chapters }) }}
        </p>
        <p v-if="activeJob.error" class="text-body-small text-error">
          {{ activeJob.error }}
        </p>
      </div>
    </div>

    <template #footer>
      <div class="flex w-full flex-wrap items-center justify-end gap-2">
        <MdButton
          v-if="activeJob && isJobActive(activeJob.status)"
          variant="outlined"
          tone="error"
          size="sm"
          :icon="LuBan"
          :loading="cancelling"
          class="mr-auto"
          @click="handleCancelJob"
        >
          {{ cancelling ? t('online.cancellingJobBtn') : t('online.cancelJobBtn') }}
        </MdButton>

        <MdButton variant="text" size="sm" @click="emit('close')">
          {{
            activeJob && isJobActive(activeJob.status)
              ? t('online.closeBackgroundBtn')
              : activeJob
                ? t('online.closeBtn')
                : t('detail.cancel')
          }}
        </MdButton>

        <template v-if="activeJob?.status === 'completed'">
          <MdButton
            v-if="activeJob.imported_book_id"
            variant="tonal"
            size="sm"
            :icon="LuBookOpen"
            @click="openImportedBook"
          >
            {{ t('online.openImportedBtn') }}
          </MdButton>

          <MdButton
            variant="filled"
            size="sm"
            :icon="LuDownload"
            :href="buildOnlineDownloadFileUrl(activeJob.id)"
            :download="activeJob.output_file_name || undefined"
          >
            {{ t('online.downloadFileBtn', { name: activeJob.output_file_name || '' }) }}
          </MdButton>
        </template>

        <MdButton
          v-else-if="!activeJob"
          variant="filled"
          size="sm"
          :icon="LuDownload"
          :loading="starting"
          @click="handleStart"
        >
          {{ t('online.startDownloadBtn') }}
        </MdButton>
      </div>
    </template>
  </MdDialog>
</template>
