<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import {
  LuBookOpen,
  LuCheckCircle2,
  LuDownload,
  LuLoader2,
  LuX,
} from 'vue-icons-plus/lu'
import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import { useAuthStore } from '@/stores/auth'
import {
  buildOnlineDownloadFileUrl,
  onlineBookApi,
  type OnlineDownloadFormat,
  type OnlineDownloadJobDto,
} from '../onlineApi'

export interface OnlineDownloadTarget {
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
}>()

const emit = defineEmits<{
  close: []
  jobUpdated: [job: OnlineDownloadJobDto]
}>()

const authStore = useAuthStore()
const router = useRouter()

const format = ref<OnlineDownloadFormat>('epub')
const useRange = ref(false)
const startChapter = ref(1)
const endChapter = ref(100)
const importToLibrary = ref(false)
const starting = ref(false)
const errorMsg = ref<string | null>(null)
const activeJob = ref<OnlineDownloadJobDto | null>(null)
let pollTimer: ReturnType<typeof setInterval> | null = null

function stopPolling(): void {
  if (pollTimer) {
    clearInterval(pollTimer)
    pollTimer = null
  }
}

watch(
  () => [props.open, props.target] as const,
  ([isOpen, t]) => {
    stopPolling()
    if (!isOpen || !t) return
    errorMsg.value = null
    activeJob.value = null
    format.value = 'epub'
    useRange.value = false
    startChapter.value = 1
    endChapter.value = Math.max(1, t.totalChapters || 100)
    importToLibrary.value = authStore.isAdmin
  },
  { immediate: true },
)

onBeforeUnmount(stopPolling)

const progressPct = computed(() => {
  const j = activeJob.value
  if (!j || j.total_chapters <= 0) return 0
  return Math.min(100, Math.round((j.processed_chapters / j.total_chapters) * 100))
})

async function handleStart(): Promise<void> {
  if (!props.target || starting.value) return
  starting.value = true
  errorMsg.value = null
  try {
    const job = await onlineBookApi.startDownload({
      source_id: props.target.sourceId,
      book_name: props.target.bookName,
      author: props.target.author,
      book_url: props.target.bookUrl,
      toc_url: props.target.tocUrl,
      cover_url: props.target.coverUrl ?? null,
      intro: props.target.intro ?? null,
      format: format.value,
      start_chapter: useRange.value ? Math.max(0, startChapter.value - 1) : undefined,
      end_chapter: useRange.value ? Math.max(0, endChapter.value - 1) : undefined,
      import_to_library: authStore.isAdmin && importToLibrary.value,
    })
    activeJob.value = job
    emit('jobUpdated', job)

    stopPolling()
    pollTimer = setInterval(async () => {
      if (!activeJob.value) return
      try {
        const next = await onlineBookApi.getDownload(activeJob.value.id)
        activeJob.value = next
        emit('jobUpdated', next)
        if (next.status === 'completed' || next.status === 'failed') {
          stopPolling()
        }
      } catch {
        stopPolling()
      }
    }, 900)
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    starting.value = false
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
  <Teleport to="body">
    <div
      v-if="open && target"
      class="fixed inset-0 z-50 flex items-center justify-center bg-black/55 p-4 backdrop-blur-[2px]"
      @click.self="emit('close')"
    >
      <div
        class="flex max-h-[86vh] w-full max-w-lg flex-col overflow-hidden rounded-[28px] bg-[var(--md-sys-color-surface-container-low)] shadow-[var(--md-sys-elevation-3)]"
      >
        <div
          class="flex items-center justify-between gap-3 border-b border-[var(--md-sys-color-outline-variant)]/50 px-5 py-4"
        >
          <div class="min-w-0">
            <h2 class="truncate text-sm font-semibold text-[var(--md-sys-color-on-surface)]">
              下载与打包 · {{ target.bookName }}
            </h2>
            <p class="mt-0.5 truncate text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
              {{ target.author || '佚名' }} · 来源：{{ target.sourceName }}
            </p>
          </div>
          <button
            type="button"
            class="md-icon-btn md-icon-btn-sm md-state"
            @click="emit('close')"
          >
            <LuX class="h-4 w-4" />
          </button>
        </div>

        <div class="space-y-4 overflow-y-auto px-5 py-4 text-xs">
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
                  class="rounded-2xl border px-3 py-2.5 text-left transition"
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
                  class="rounded-2xl border px-3 py-2.5 text-left transition"
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
                label="打包完成后一键入库到本地托管图书库（自动支持 OPDS 与离线永久阅读）"
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
                  class="h-4 w-4 text-emerald-500"
                />
                <span>
                  {{
                    activeJob.status === 'completed'
                      ? '打包完成'
                      : activeJob.status === 'failed'
                        ? '打包失败'
                        : '正在抓取章节…'
                  }}
                </span>
              </span>
              <span class="font-mono text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
                {{ activeJob.processed_chapters }} / {{ activeJob.total_chapters || '?' }} 章
              </span>
            </div>

            <div class="h-2 overflow-hidden rounded-full bg-[var(--md-sys-color-surface-container-highest)]">
              <div
                class="h-full rounded-full bg-[var(--md-sys-color-primary)] transition-all duration-300"
                :style="{ width: `${activeJob.status === 'completed' ? 100 : progressPct}%` }"
              />
            </div>

            <p
              v-if="activeJob.current_chapter_title"
              class="truncate text-[11px] text-[var(--md-sys-color-on-surface-variant)]"
            >
              {{ activeJob.current_chapter_title }}
            </p>
            <p v-if="activeJob.failed_chapters > 0" class="text-[11px] text-amber-500">
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

        <div
          class="flex items-center justify-end gap-2 border-t border-[var(--md-sys-color-outline-variant)]/40 px-5 py-3"
        >
          <button
            type="button"
            class="md-btn md-btn-sm md-btn-text md-state"
            @click="emit('close')"
          >
            {{ activeJob ? '后台运行 / 关闭' : '取消' }}
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
      </div>
    </div>
  </Teleport>
</template>
