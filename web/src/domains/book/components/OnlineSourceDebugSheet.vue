<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import {
  LuArrowRight,
  LuBookOpen,
  LuCheckCircle2,
  LuClock,
  LuCopy,
  LuFileText,
  LuFlaskConical,
  LuList,
  LuLoader2,
  LuPlay,
  LuSearch,
  LuTerminal,
  LuX,
  LuXCircle,
} from 'vue-icons-plus/lu'
import MdSegmentedButton, { type SegmentedOption } from '@/components/ui/MdSegmentedButton.vue'
import MdSideSheet from '@/components/ui/MdSideSheet.vue'
import { useSnackbarStore } from '@/stores/snackbar'
import {
  buildOnlineCoverUrl,
  onlineBookApi,
  type BookSourceSummaryDto,
  type OnlineChapterItem,
  type SearchBookResult,
  type SourceTestInput,
  type SourceTestMode,
  type SourceTestReportDto,
} from '../onlineApi'

const props = defineProps<{
  open: boolean
  source: BookSourceSummaryDto | null
}>()

const emit = defineEmits<{
  close: []
  tested: []
}>()

const snackbar = useSnackbarStore()

type ResultTab = 'timeline' | 'search' | 'book_info' | 'toc' | 'content'

const modeOptions: readonly SegmentedOption<SourceTestMode>[] = [
  { value: 'full', label: '全链路', title: '搜索 → 详情 → 目录 → 正文 完整链路测试' },
  { value: 'search', label: '搜索', title: '仅测试 ruleSearch 搜索规则' },
  { value: 'book_info', label: '详情', title: '仅测试 ruleBookInfo 书籍详情规则' },
  { value: 'toc', label: '目录', title: '仅测试 ruleToc 目录列表规则' },
  { value: 'content', label: '正文', title: '仅测试 ruleContent 章节正文规则' },
]

const mode = ref<SourceTestMode>('full')
const queryInput = ref('')
const running = ref(false)
const requestError = ref<string | null>(null)
const report = ref<SourceTestReportDto | null>(null)
const activeTab = ref<ResultTab>('timeline')
const tocFilter = ref('')

// Remember last known URLs from earlier runs so switching single-rule modes pre-fills effortlessly
const lastBookUrl = ref('')
const lastTocUrl = ref('')
const lastChapterUrl = ref('')

watch(
  () => [props.open, props.source?.id] as const,
  ([isOpen, srcId], prev) => {
    if (!isOpen || !props.source) return
    if (!prev || !prev[0] || prev[1] !== srcId) {
      mode.value = 'full'
      queryInput.value = props.source.check_keyword?.trim() || '我的'
      report.value = null
      requestError.value = null
      activeTab.value = 'timeline'
      tocFilter.value = ''
      lastBookUrl.value = ''
      lastTocUrl.value = ''
      lastChapterUrl.value = ''
    }
  },
  { immediate: true },
)

watch(mode, (nextMode) => {
  if (nextMode === 'book_info' && lastBookUrl.value && !queryInput.value.startsWith('http')) {
    queryInput.value = lastBookUrl.value
  } else if (nextMode === 'toc' && lastTocUrl.value && !queryInput.value.startsWith('http')) {
    queryInput.value = lastTocUrl.value
  } else if (nextMode === 'content' && lastChapterUrl.value && !queryInput.value.startsWith('http')) {
    queryInput.value = lastChapterUrl.value
  } else if (
    (nextMode === 'full' || nextMode === 'search') &&
    queryInput.value.startsWith('http') &&
    props.source
  ) {
    queryInput.value = props.source.check_keyword?.trim() || '我的'
  }
})

const inputPlaceholder = computed(() => {
  const defaultKw = props.source?.check_keyword?.trim() || '我的'
  switch (mode.value) {
    case 'full':
      return `输入搜索关键词（留空默认「${defaultKw}」），或直接粘贴书籍详情页 URL…`
    case 'search':
      return `输入搜索关键词（留空默认「${defaultKw}」）…`
    case 'book_info':
      return '输入书籍详情页 URL（https://...）…'
    case 'toc':
      return '输入目录页 URL（https://...）…'
    case 'content':
      return '输入章节正文页 URL（https://...）…'
  }
})

const filteredToc = computed(() => {
  const list = report.value?.toc_chapters ?? []
  const q = tocFilter.value.trim().toLowerCase()
  if (!q) return list
  return list.filter(
    (ch) =>
      ch.title.toLowerCase().includes(q) ||
      String(ch.index + 1).includes(q) ||
      ch.url.toLowerCase().includes(q),
  )
})

const contentParagraphs = computed(() => {
  const raw = report.value?.content_text ?? ''
  if (!raw.trim()) return []
  return raw
    .split(/\r?\n+/)
    .map((line) => line.trim())
    .filter((line) => line.length > 0)
})

const totalDiagnosticsCount = computed(() => {
  if (!report.value) return 0
  const d = report.value.diagnostics
  return d.logs.length + d.toasts.length + d.browser_urls.length
})

const detailFields = computed(() => {
  const d = report.value?.book_detail
  if (!d) return []
  return [
    { label: '书名 (name)', value: d.name },
    { label: '作者 (author)', value: d.author },
    { label: '分类 (kind)', value: d.kind ?? '' },
    { label: '字数 (wordCount)', value: d.word_count ?? '' },
    { label: '最新章节 (lastChapter)', value: d.last_chapter ?? '' },
    { label: '详情 URL (bookUrl)', value: d.book_url, mono: true },
    { label: '目录 URL (tocUrl)', value: d.toc_url, mono: true },
    { label: '封面 URL (coverUrl)', value: d.cover_url ?? '', mono: true },
    { label: '简介 (intro)', value: d.intro ?? '', multiline: true },
  ]
})

function stageToTab(stage: string): ResultTab {
  if (stage === 'search') return 'search'
  if (stage === 'book_info') return 'book_info'
  if (stage === 'toc') return 'toc'
  if (stage === 'content') return 'content'
  return 'timeline'
}

async function executeTest(
  payload: SourceTestInput,
  preferredTab?: ResultTab,
  preservePrevious = false,
): Promise<void> {
  if (!props.source || running.value) return
  running.value = true
  requestError.value = null
  const prev = preservePrevious ? report.value : null
  try {
    const res = await onlineBookApi.testSource(props.source.id, payload)
    const ranStages = new Set(res.steps.map((s) => s.stage))
    const merged: SourceTestReportDto = prev
      ? {
          ...res,
          result_count: ranStages.has('search') ? res.result_count : prev.result_count,
          search_items: ranStages.has('search') ? res.search_items : prev.search_items,
          sample_titles: ranStages.has('search') ? res.sample_titles : prev.sample_titles,
          book_detail: ranStages.has('book_info') ? res.book_detail : prev.book_detail,
          toc_chapters: ranStages.has('toc') ? res.toc_chapters : prev.toc_chapters,
          toc_total_count: ranStages.has('toc') ? res.toc_total_count : prev.toc_total_count,
          content_chapter_index: ranStages.has('content')
            ? res.content_chapter_index
            : prev.content_chapter_index,
          content_chapter_title: ranStages.has('content')
            ? res.content_chapter_title
            : prev.content_chapter_title,
          content_chapter_url: ranStages.has('content')
            ? res.content_chapter_url
            : prev.content_chapter_url,
          content_text: ranStages.has('content') ? res.content_text : prev.content_text,
          content_char_count: ranStages.has('content')
            ? res.content_char_count
            : prev.content_char_count,
        }
      : res

    report.value = merged
    if (merged.book_detail?.book_url) lastBookUrl.value = merged.book_detail.book_url
    else if (merged.search_items[0]?.book_url) lastBookUrl.value = merged.search_items[0].book_url
    if (merged.book_detail?.toc_url) lastTocUrl.value = merged.book_detail.toc_url
    if (merged.content_chapter_url) lastChapterUrl.value = merged.content_chapter_url
    else if (merged.toc_chapters[0]?.url) lastChapterUrl.value = merged.toc_chapters[0].url

    if (preferredTab) {
      activeTab.value = preferredTab
    } else if (res.mode === 'search') {
      activeTab.value = res.search_items.length > 0 ? 'search' : 'timeline'
    } else if (res.mode === 'book_info') {
      activeTab.value = res.book_detail ? 'book_info' : 'timeline'
    } else if (res.mode === 'toc') {
      activeTab.value = res.toc_chapters.length > 0 ? 'toc' : 'timeline'
    } else if (res.mode === 'content') {
      activeTab.value = res.content_text ? 'content' : 'timeline'
    } else {
      activeTab.value = 'timeline'
    }

    emit('tested')
  } catch (err) {
    requestError.value = err instanceof Error ? err.message : String(err)
  } finally {
    running.value = false
  }
}

async function handleRun(): Promise<void> {
  const val = queryInput.value.trim()
  const payload: SourceTestInput = { mode: mode.value }
  if (mode.value === 'full' || mode.value === 'search') {
    payload.keyword = val || null
  } else if (mode.value === 'book_info') {
    payload.book_url = val || null
  } else if (mode.value === 'toc') {
    payload.toc_url = val || null
  } else if (mode.value === 'content') {
    payload.chapter_url = val || null
  }
  await executeTest(payload, undefined, mode.value !== 'full' && mode.value !== 'search')
}

async function handleDebugBookItem(item: SearchBookResult): Promise<void> {
  mode.value = 'full'
  queryInput.value = item.book_url
  lastBookUrl.value = item.book_url
  await executeTest({ mode: 'full', book_url: item.book_url }, 'timeline', true)
}

async function handleDebugTocFromDetail(): Promise<void> {
  const tocUrl = report.value?.book_detail?.toc_url || lastTocUrl.value
  if (!tocUrl) return
  mode.value = 'toc'
  queryInput.value = tocUrl
  await executeTest({ mode: 'toc', toc_url: tocUrl }, 'toc', true)
}

async function handleDebugChapter(ch: OnlineChapterItem): Promise<void> {
  if (!ch.url) return
  mode.value = 'content'
  queryInput.value = ch.url
  lastChapterUrl.value = ch.url
  await executeTest(
    {
      mode: 'content',
      chapter_url: ch.url,
      chapter_index: ch.index,
      chapter_title: ch.title,
      book_url: lastBookUrl.value || null,
      toc_url: lastTocUrl.value || null,
    },
    'content',
    true,
  )
}

async function copyDebugReport(): Promise<void> {
  if (!report.value || !props.source) return
  const r = report.value
  const lines: string[] = [
    `=== 书源调试报告：${props.source.name} ===`,
    `源地址：${props.source.source_url}`,
    `模式：${r.mode} | 输入：${r.keyword} | 结果：${r.ok ? '通过' : '失败'} | 总耗时：${r.total_elapsed_ms} ms`,
    '',
    '--- 规则执行步骤 ---',
    ...r.steps.map(
      (s, i) =>
        `${i + 1}. [${s.ok ? 'OK' : 'FAIL'}] ${s.label} (${s.elapsed_ms}ms) — ${s.summary}${s.error ? `\n   错误：${s.error}` : ''}`,
    ),
  ]
  if (r.book_detail) {
    lines.push(
      '',
      '--- 书籍详情 ---',
      `书名：${r.book_detail.name}`,
      `作者：${r.book_detail.author}`,
      `目录：${r.book_detail.toc_url}`,
      `简介：${r.book_detail.intro ?? ''}`,
    )
  }
  if (r.toc_total_count > 0) {
    lines.push('', `--- 目录（共 ${r.toc_total_count} 章）---`)
    for (const ch of r.toc_chapters.slice(0, 20)) {
      lines.push(`[${ch.index + 1}] ${ch.title} -> ${ch.url}`)
    }
  }
  if (r.content_text) {
    lines.push(
      '',
      `--- 正文：${r.content_chapter_title ?? ''}（${r.content_char_count} 字）---`,
      r.content_text,
    )
  }
  if (r.diagnostics.logs.length > 0) {
    lines.push('', '--- 诊断日志 ---', ...r.diagnostics.logs)
  }
  try {
    await navigator.clipboard.writeText(lines.join('\n'))
    snackbar.show({ message: '已复制完整调试报告' })
  } catch {
    snackbar.show({ message: '复制失败，请手动选中复制', tone: 'error' })
  }
}

async function copyContentText(): Promise<void> {
  if (!report.value?.content_text) return
  try {
    await navigator.clipboard.writeText(report.value.content_text)
    snackbar.show({ message: '已复制完整正文内容' })
  } catch {
    snackbar.show({ message: '复制失败', tone: 'error' })
  }
}
</script>

<template>
  <MdSideSheet
    :open="open"
    :title="`书源调试 · ${source?.name ?? ''}`"
    :subtitle="source?.source_url"
    width-class="w-[min(52rem,100vw)]"
    @close="emit('close')"
  >
    <template #subheader>
      <div class="space-y-3 border-b border-[var(--md-sys-color-outline-variant)]/60 pb-3.5">
        <!-- Rule Mode Selector -->
        <div class="flex flex-wrap items-center justify-between gap-2">
          <MdSegmentedButton
            v-model="mode"
            :options="modeOptions"
            size="sm"
            :show-check="false"
          />

          <button
            v-if="report"
            type="button"
            class="md-btn md-btn-sm md-btn-tonal md-state"
            @click="copyDebugReport"
          >
            <LuCopy class="h-3.5 w-3.5" />
            <span>复制调试报告</span>
          </button>
        </div>

        <!-- Keyword / URL Input Bar -->
        <form class="flex items-center gap-2" @submit.prevent="handleRun">
          <div class="relative flex-1">
            <LuSearch
              class="pointer-events-none absolute top-1/2 left-3.5 h-4 w-4 -translate-y-1/2 text-[var(--md-sys-color-on-surface-variant)]"
            />
            <input
              v-model="queryInput"
              type="text"
              :placeholder="inputPlaceholder"
              class="md-input w-full py-2 pr-9 pl-10 text-sm"
            />
            <button
              v-if="queryInput"
              type="button"
              class="absolute top-1/2 right-2.5 -translate-y-1/2 rounded-full p-1 text-[var(--md-sys-color-on-surface-variant)] hover:bg-[var(--md-sys-color-surface-container-highest)]"
              title="清空"
              @click="queryInput = ''"
            >
              <LuX class="h-3.5 w-3.5" />
            </button>
          </div>

          <button
            type="submit"
            :disabled="running"
            class="md-btn md-btn-filled md-state shrink-0"
          >
            <LuLoader2 v-if="running" class="h-4 w-4 animate-spin" />
            <LuPlay v-else class="h-4 w-4" />
            <span>{{ running ? '调试中…' : '开始测试' }}</span>
          </button>
        </form>

        <!-- Quick Hint Chips -->
        <div
          v-if="source?.check_keyword || lastBookUrl || lastTocUrl || lastChapterUrl"
          class="flex flex-wrap items-center gap-1.5 text-[11px]"
        >
          <span class="text-[var(--md-sys-color-on-surface-variant)]">快捷填入：</span>
          <button
            v-if="source?.check_keyword && (mode === 'full' || mode === 'search')"
            type="button"
            class="md-state rounded-full bg-[var(--md-sys-color-surface-container-high)] px-2.5 py-0.5 text-[var(--md-sys-color-on-surface)]"
            @click="queryInput = source.check_keyword!"
          >
            校验词：{{ source.check_keyword }}
          </button>
          <button
            v-if="lastBookUrl && mode === 'book_info'"
            type="button"
            class="md-state max-w-xs truncate rounded-full bg-[var(--md-sys-color-surface-container-high)] px-2.5 py-0.5 font-mono text-[var(--md-sys-color-on-surface)]"
            @click="queryInput = lastBookUrl"
          >
            上次详情 URL
          </button>
          <button
            v-if="lastTocUrl && mode === 'toc'"
            type="button"
            class="md-state max-w-xs truncate rounded-full bg-[var(--md-sys-color-surface-container-high)] px-2.5 py-0.5 font-mono text-[var(--md-sys-color-on-surface)]"
            @click="queryInput = lastTocUrl"
          >
            上次目录 URL
          </button>
          <button
            v-if="lastChapterUrl && mode === 'content'"
            type="button"
            class="md-state max-w-xs truncate rounded-full bg-[var(--md-sys-color-surface-container-high)] px-2.5 py-0.5 font-mono text-[var(--md-sys-color-on-surface)]"
            @click="queryInput = lastChapterUrl"
          >
            上次章节 URL
          </button>
        </div>
      </div>
    </template>

    <!-- Main Body -->
    <div class="space-y-4 pt-1">
      <!-- Request / Validation Error -->
      <div
        v-if="requestError"
        class="flex items-start gap-2.5 rounded-2xl bg-[var(--md-sys-color-error-container)] px-4 py-3 text-xs text-[var(--md-sys-color-on-error-container)]"
      >
        <LuXCircle class="mt-0.5 h-4 w-4 shrink-0" />
        <div class="min-w-0 flex-1 whitespace-pre-wrap break-all">{{ requestError }}</div>
      </div>

      <!-- Running Skeleton -->
      <div
        v-if="running && !report"
        class="flex flex-col items-center justify-center gap-3 rounded-3xl bg-[var(--md-sys-color-surface-container)] py-16 text-center"
      >
        <LuLoader2 class="h-7 w-7 animate-spin text-[var(--md-sys-color-primary)]" />
        <p class="text-sm font-medium text-[var(--md-sys-color-on-surface)]">
          正在执行书源规则解析…
        </p>
        <p class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
          全链路将依次测试 搜索 → 书籍详情 → 目录列表 → 章节正文
        </p>
      </div>

      <!-- Idle Empty State -->
      <div
        v-else-if="!report && !running"
        class="rounded-3xl bg-[var(--md-sys-color-surface-container)] p-6 text-center"
      >
        <div
          class="mx-auto flex h-12 w-12 items-center justify-center rounded-2xl bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]"
        >
          <LuFlaskConical class="h-6 w-6" />
        </div>
        <h4 class="mt-3 text-sm font-semibold text-[var(--md-sys-color-on-surface)]">
          书源规则调试器
        </h4>
        <p class="mx-auto mt-1.5 max-w-md text-xs leading-relaxed text-[var(--md-sys-color-on-surface-variant)]">
          支持指定搜索关键词或直接粘贴书籍 URL，按 Legado 流程依次验证
          <span class="font-mono text-[var(--md-sys-color-on-surface)]">ruleSearch</span> →
          <span class="font-mono text-[var(--md-sys-color-on-surface)]">ruleBookInfo</span> →
          <span class="font-mono text-[var(--md-sys-color-on-surface)]">ruleToc</span> →
          <span class="font-mono text-[var(--md-sys-color-on-surface)]">ruleContent</span>，
          也可切换顶部模式单独调试任一规则。
        </p>
        <button
          type="button"
          class="md-btn md-btn-tonal md-state mt-4"
          @click="handleRun"
        >
          <LuPlay class="h-4 w-4" />
          <span>立即开始全链路测试</span>
        </button>
      </div>

      <!-- Debug Report -->
      <template v-if="report">
        <!-- Overview Banner & Clickable Stage Pipeline -->
        <div class="rounded-2xl bg-[var(--md-sys-color-surface-container)] p-4">
          <div class="flex flex-wrap items-center justify-between gap-2">
            <div class="flex items-center gap-2">
              <span
                class="inline-flex items-center gap-1.5 rounded-full px-3 py-1 text-xs font-semibold"
                :class="
                  report.ok
                    ? 'bg-[var(--md-sys-color-success-container)] text-[var(--md-sys-color-on-success-container)]'
                    : 'bg-[var(--md-sys-color-error-container)] text-[var(--md-sys-color-on-error-container)]'
                "
              >
                <LuCheckCircle2 v-if="report.ok" class="h-3.5 w-3.5" />
                <LuXCircle v-else class="h-3.5 w-3.5" />
                <span>{{ report.ok ? '全部规则测试通过' : '存在失败规则' }}</span>
              </span>
              <span
                class="inline-flex items-center gap-1 rounded-full bg-[var(--md-sys-color-surface-container-high)] px-2.5 py-1 text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]"
              >
                <LuClock class="h-3 w-3" />
                <span>总耗时 {{ report.total_elapsed_ms }} ms</span>
              </span>
            </div>

            <span
              :title="report.keyword"
              class="max-w-[16rem] truncate text-xs text-[var(--md-sys-color-on-surface-variant)]"
            >
              输入：<span class="font-mono text-[var(--md-sys-color-on-surface)]">{{ report.keyword }}</span>
            </span>
          </div>

          <!-- Stage Steps Strip -->
          <div class="mt-3 grid grid-cols-1 gap-2 sm:grid-cols-2">
            <button
              v-for="step in report.steps"
              :key="step.stage"
              type="button"
              class="md-state flex items-start gap-2.5 rounded-xl bg-[var(--md-sys-color-surface-container-low)] px-3 py-2.5 text-left transition"
              :class="
                activeTab === stageToTab(step.stage)
                  ? 'ring-1 ring-[var(--md-sys-color-primary)]'
                  : ''
              "
              @click="activeTab = stageToTab(step.stage)"
            >
              <LuCheckCircle2
                v-if="step.ok"
                class="mt-0.5 h-4 w-4 shrink-0 text-[var(--md-sys-color-success)]"
              />
              <LuXCircle
                v-else
                class="mt-0.5 h-4 w-4 shrink-0 text-[var(--md-sys-color-error)]"
              />
              <div class="min-w-0 flex-1">
                <div class="flex items-center justify-between gap-2">
                  <span class="text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
                    {{ step.label }}
                  </span>
                  <span class="shrink-0 font-mono text-[10px] text-[var(--md-sys-color-on-surface-variant)]">
                    {{ step.elapsed_ms }}ms
                  </span>
                </div>
                <p class="mt-0.5 truncate text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
                  {{ step.summary }}
                </p>
              </div>
            </button>
          </div>
        </div>

        <!-- MD3 Result Section Tabs -->
        <div
          class="flex flex-wrap items-center gap-1.5 border-b border-[var(--md-sys-color-outline-variant)]/50 pb-2.5"
        >
          <button
            type="button"
            class="md-state inline-flex items-center gap-1.5 rounded-full px-3.5 py-1.5 text-xs font-medium transition"
            :class="
              activeTab === 'timeline'
                ? 'bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]'
                : 'text-[var(--md-sys-color-on-surface-variant)] hover:bg-[var(--md-sys-color-surface-container-high)]'
            "
            @click="activeTab = 'timeline'"
          >
            <LuTerminal class="h-3.5 w-3.5" />
            <span>规则流水</span>
            <span
              v-if="totalDiagnosticsCount > 0"
              class="rounded-full bg-[var(--md-sys-color-surface-container-highest)] px-1.5 text-[10px]"
            >
              {{ totalDiagnosticsCount }}
            </span>
          </button>

          <button
            type="button"
            class="md-state inline-flex items-center gap-1.5 rounded-full px-3.5 py-1.5 text-xs font-medium transition"
            :class="
              activeTab === 'search'
                ? 'bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]'
                : 'text-[var(--md-sys-color-on-surface-variant)] hover:bg-[var(--md-sys-color-surface-container-high)]'
            "
            @click="activeTab = 'search'"
          >
            <LuSearch class="h-3.5 w-3.5" />
            <span>搜索结果 ({{ report.result_count }})</span>
          </button>

          <button
            type="button"
            class="md-state inline-flex items-center gap-1.5 rounded-full px-3.5 py-1.5 text-xs font-medium transition"
            :class="
              activeTab === 'book_info'
                ? 'bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]'
                : 'text-[var(--md-sys-color-on-surface-variant)] hover:bg-[var(--md-sys-color-surface-container-high)]'
            "
            @click="activeTab = 'book_info'"
          >
            <LuBookOpen class="h-3.5 w-3.5" />
            <span>书籍详情</span>
          </button>

          <button
            type="button"
            class="md-state inline-flex items-center gap-1.5 rounded-full px-3.5 py-1.5 text-xs font-medium transition"
            :class="
              activeTab === 'toc'
                ? 'bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]'
                : 'text-[var(--md-sys-color-on-surface-variant)] hover:bg-[var(--md-sys-color-surface-container-high)]'
            "
            @click="activeTab = 'toc'"
          >
            <LuList class="h-3.5 w-3.5" />
            <span>目录列表 ({{ report.toc_total_count }})</span>
          </button>

          <button
            type="button"
            class="md-state inline-flex items-center gap-1.5 rounded-full px-3.5 py-1.5 text-xs font-medium transition"
            :class="
              activeTab === 'content'
                ? 'bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]'
                : 'text-[var(--md-sys-color-on-surface-variant)] hover:bg-[var(--md-sys-color-surface-container-high)]'
            "
            @click="activeTab = 'content'"
          >
            <LuFileText class="h-3.5 w-3.5" />
            <span>正文内容 ({{ report.content_char_count }} 字)</span>
          </button>
        </div>

        <!-- TAB 1: Timeline & Rule Diagnostics -->
        <div v-if="activeTab === 'timeline'" class="space-y-3">
          <div
            v-for="(step, idx) in report.steps"
            :key="`${step.stage}-${idx}`"
            class="rounded-2xl bg-[var(--md-sys-color-surface-container)] p-4"
          >
            <div class="flex flex-wrap items-center justify-between gap-2">
              <div class="flex items-center gap-2">
                <span
                  class="flex h-5 w-5 items-center justify-center rounded-full text-[11px] font-bold"
                  :class="
                    step.ok
                      ? 'bg-[var(--md-sys-color-success-container)] text-[var(--md-sys-color-on-success-container)]'
                      : 'bg-[var(--md-sys-color-error-container)] text-[var(--md-sys-color-on-error-container)]'
                  "
                >
                  {{ idx + 1 }}
                </span>
                <span class="text-sm font-semibold text-[var(--md-sys-color-on-surface)]">
                  {{ step.label }}
                </span>
              </div>
              <div class="flex items-center gap-2">
                <span class="font-mono text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
                  {{ step.elapsed_ms }} ms
                </span>
                <button
                  type="button"
                  class="md-btn md-btn-sm md-btn-text md-state"
                  @click="activeTab = stageToTab(step.stage)"
                >
                  <span>查看解析结果</span>
                  <LuArrowRight class="h-3.5 w-3.5" />
                </button>
              </div>
            </div>

            <p class="mt-2 text-xs text-[var(--md-sys-color-on-surface)]">
              {{ step.summary }}
            </p>

            <div
              v-if="step.error"
              class="mt-2.5 rounded-xl bg-[var(--md-sys-color-error-container)] px-3 py-2 font-mono text-xs whitespace-pre-wrap break-all text-[var(--md-sys-color-on-error-container)]"
            >
              {{ step.error }}
            </div>

            <div
              v-if="
                step.diagnostics.logs.length > 0 ||
                step.diagnostics.toasts.length > 0 ||
                step.diagnostics.browser_urls.length > 0
              "
              class="mt-2.5 space-y-1.5 rounded-xl bg-[var(--md-sys-color-surface-container-low)] p-3 font-mono text-[11px]"
            >
              <div
                v-for="(toast, ti) in step.diagnostics.toasts"
                :key="`toast-${ti}`"
                class="text-[var(--md-sys-color-primary)]"
              >
                [toast] {{ toast }}
              </div>
              <div
                v-for="(log, li) in step.diagnostics.logs"
                :key="`log-${li}`"
                class="whitespace-pre-wrap break-all text-[var(--md-sys-color-on-surface-variant)]"
              >
                [log] {{ log }}
              </div>
              <div
                v-for="(hint, hi) in step.diagnostics.browser_urls"
                :key="`url-${hi}`"
                class="break-all text-[var(--md-sys-color-tertiary)]"
              >
                [browser] {{ hint.title || '跳转' }}: {{ hint.url }}
              </div>
            </div>
          </div>
        </div>

        <!-- TAB 2: Search Results -->
        <div v-else-if="activeTab === 'search'" class="space-y-2.5">
          <div
            v-if="report.search_items.length === 0"
            class="rounded-2xl bg-[var(--md-sys-color-surface-container)] py-10 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
          >
            本次调试未包含搜索结果（可能未执行搜索阶段或未命中书籍）
          </div>

          <template v-else>
            <p class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
              共命中 {{ report.result_count }} 本（展示前 {{ report.search_items.length }} 条完整解析字段，点击右侧可对单本继续全链路调试）
            </p>
            <div
              v-for="(item, idx) in report.search_items"
              :key="`${item.book_url}-${idx}`"
              class="flex items-start gap-3 rounded-2xl bg-[var(--md-sys-color-surface-container)] p-3.5"
            >
              <div
                class="h-20 w-14 shrink-0 overflow-hidden rounded-lg bg-[var(--md-sys-color-surface-container-high)]"
              >
                <img
                  v-if="item.cover_url"
                  :src="buildOnlineCoverUrl(item.cover_url)"
                  :alt="item.name"
                  class="h-full w-full object-cover"
                  loading="lazy"
                />
                <div
                  v-else
                  class="flex h-full w-full items-center justify-center text-[10px] text-[var(--md-sys-color-on-surface-variant)]"
                >
                  无封面
                </div>
              </div>

              <div class="min-w-0 flex-1 space-y-1">
                <div class="flex flex-wrap items-start justify-between gap-2">
                  <div class="min-w-0">
                    <div class="flex flex-wrap items-center gap-1.5">
                      <span class="font-mono text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
                        #{{ idx + 1 }}
                      </span>
                      <span class="text-sm font-semibold text-[var(--md-sys-color-on-surface)]">
                        {{ item.name }}
                      </span>
                      <span
                        v-if="item.author"
                        class="text-xs text-[var(--md-sys-color-on-surface-variant)]"
                      >
                        · {{ item.author }}
                      </span>
                    </div>
                  </div>
                  <button
                    type="button"
                    :disabled="running"
                    class="md-btn md-btn-sm md-btn-tonal md-state shrink-0"
                    @click="handleDebugBookItem(item)"
                  >
                    <span>测此书详情</span>
                    <LuArrowRight class="h-3.5 w-3.5" />
                  </button>
                </div>

                <div class="flex flex-wrap items-center gap-1.5 text-[11px]">
                  <span
                    v-if="item.kind"
                    class="rounded-full bg-[var(--md-sys-color-surface-container-high)] px-2 py-0.5 text-[var(--md-sys-color-on-surface-variant)]"
                  >
                    {{ item.kind }}
                  </span>
                  <span
                    v-if="item.word_count"
                    class="rounded-full bg-[var(--md-sys-color-surface-container-high)] px-2 py-0.5 text-[var(--md-sys-color-on-surface-variant)]"
                  >
                    {{ item.word_count }}
                  </span>
                  <span
                    v-if="item.last_chapter"
                    class="text-[var(--md-sys-color-on-surface-variant)]"
                  >
                    最新：{{ item.last_chapter }}
                  </span>
                </div>

                <p
                  v-if="item.intro"
                  class="text-xs leading-relaxed whitespace-pre-wrap text-[var(--md-sys-color-on-surface-variant)]"
                >
                  {{ item.intro }}
                </p>

                <p class="break-all font-mono text-[11px] text-[var(--md-sys-color-on-surface-variant)]/85">
                  {{ item.book_url }}
                </p>
              </div>
            </div>
          </template>
        </div>

        <!-- TAB 3: Book Detail -->
        <div v-else-if="activeTab === 'book_info'" class="space-y-3">
          <div
            v-if="!report.book_detail"
            class="rounded-2xl bg-[var(--md-sys-color-surface-container)] py-10 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
          >
            本次调试尚未获取书籍详情（可切换至「详情」模式填入 URL，或在搜索结果中点击「测此书详情」）
          </div>

          <template v-else>
            <div class="flex flex-wrap items-center justify-between gap-2">
              <span class="text-xs font-medium text-[var(--md-sys-color-on-surface-variant)]">
                ruleBookInfo 完整字段解析结果
              </span>
              <button
                type="button"
                :disabled="running || !report.book_detail.toc_url"
                class="md-btn md-btn-sm md-btn-tonal md-state"
                @click="handleDebugTocFromDetail"
              >
                <span>用此目录 URL 测目录</span>
                <LuArrowRight class="h-3.5 w-3.5" />
              </button>
            </div>

            <div class="overflow-hidden rounded-2xl bg-[var(--md-sys-color-surface-container)]">
              <div
                v-for="(field, fIdx) in detailFields"
                :key="field.label"
                class="grid grid-cols-1 gap-1 px-4 py-3 sm:grid-cols-[11rem_1fr] sm:gap-3"
                :class="fIdx > 0 ? 'border-t border-[var(--md-sys-color-outline-variant)]/40' : ''"
              >
                <div class="text-xs font-medium text-[var(--md-sys-color-on-surface-variant)]">
                  {{ field.label }}
                </div>
                <div
                  class="min-w-0 text-xs"
                  :class="[
                    field.value
                      ? 'text-[var(--md-sys-color-on-surface)]'
                      : 'italic text-[var(--md-sys-color-on-surface-variant)]/60',
                    field.mono ? 'break-all font-mono' : '',
                    field.multiline ? 'leading-relaxed whitespace-pre-wrap' : 'break-words',
                  ]"
                >
                  {{ field.value || '（未解析 / 为空）' }}
                </div>
              </div>
            </div>
          </template>
        </div>

        <!-- TAB 4: TOC Chapters -->
        <div v-else-if="activeTab === 'toc'" class="space-y-3">
          <div
            v-if="report.toc_chapters.length === 0"
            class="rounded-2xl bg-[var(--md-sys-color-surface-container)] py-10 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
          >
            本次调试未解析到章节目录
          </div>

          <template v-else>
            <div class="flex flex-wrap items-center justify-between gap-2">
              <span class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
                共解析到 {{ report.toc_total_count }} 章
                <template v-if="report.toc_total_count > report.toc_chapters.length">
                  （已展示前 {{ report.toc_chapters.length }} 章）
                </template>
              </span>
              <input
                v-model="tocFilter"
                type="text"
                placeholder="筛选章节名或序号…"
                class="md-input w-56 py-1.5 text-xs"
              />
            </div>

            <div class="divide-y divide-[var(--md-sys-color-outline-variant)]/40 overflow-hidden rounded-2xl bg-[var(--md-sys-color-surface-container)]">
              <div
                v-for="ch in filteredToc"
                :key="`${ch.index}-${ch.url}`"
                class="flex items-center justify-between gap-3 px-4 py-2.5"
              >
                <div class="min-w-0 flex-1">
                  <div class="flex items-center gap-2">
                    <span class="w-10 shrink-0 font-mono text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
                      #{{ ch.index + 1 }}
                    </span>
                    <span
                      v-if="ch.is_volume"
                      class="rounded-full bg-[var(--md-sys-color-tertiary-container)] px-2 py-0.5 text-[10px] font-medium text-[var(--md-sys-color-on-tertiary-container)]"
                    >
                      卷标
                    </span>
                    <span class="truncate text-xs font-medium text-[var(--md-sys-color-on-surface)]">
                      {{ ch.title }}
                    </span>
                  </div>
                  <p class="mt-0.5 truncate pl-12 font-mono text-[10px] text-[var(--md-sys-color-on-surface-variant)]">
                    {{ ch.url || '（无链接）' }}
                  </p>
                </div>

                <button
                  v-if="ch.url && !ch.is_volume"
                  type="button"
                  :disabled="running"
                  class="md-btn md-btn-sm md-btn-outlined md-state shrink-0"
                  @click="handleDebugChapter(ch)"
                >
                  <span>测本章正文</span>
                </button>
              </div>
            </div>
          </template>
        </div>

        <!-- TAB 5: Full Chapter Content -->
        <div v-else-if="activeTab === 'content'" class="space-y-3">
          <div
            v-if="!report.content_text"
            class="rounded-2xl bg-[var(--md-sys-color-surface-container)] py-10 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
          >
            本次调试未获取到正文内容（可在目录列表中点击任一章节的「测本章正文」）
          </div>

          <template v-else>
            <div class="flex flex-wrap items-center justify-between gap-2 rounded-2xl bg-[var(--md-sys-color-surface-container)] px-4 py-3">
              <div class="min-w-0">
                <div class="text-sm font-semibold text-[var(--md-sys-color-on-surface)]">
                  {{ report.content_chapter_title || '测试章节' }}
                </div>
                <div class="mt-0.5 flex flex-wrap items-center gap-2 text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
                  <span>共 {{ report.content_char_count }} 字</span>
                  <span v-if="report.content_chapter_url" class="break-all font-mono">
                    · {{ report.content_chapter_url }}
                  </span>
                </div>
              </div>
              <button
                type="button"
                class="md-btn md-btn-sm md-btn-tonal md-state shrink-0"
                @click="copyContentText"
              >
                <LuCopy class="h-3.5 w-3.5" />
                <span>复制正文</span>
              </button>
            </div>

            <article
              class="space-y-3 rounded-2xl bg-[var(--md-sys-color-surface-container)] p-5 text-sm leading-7 text-[var(--md-sys-color-on-surface)]"
            >
              <p v-for="(para, pIdx) in contentParagraphs" :key="pIdx" class="indent-7 break-words">
                {{ para }}
              </p>
            </article>
          </template>
        </div>
      </template>
    </div>
  </MdSideSheet>
</template>
