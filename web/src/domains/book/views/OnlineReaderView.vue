<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import {
  LuArrowLeft,
  LuBookMarked,
  LuChevronLeft,
  LuChevronRight,
  LuDownload,
  LuList,
  LuLoader2,
  LuMinus,
  LuPlus,
  LuRefreshCw,
  LuRepeat,
  LuSearch,
  LuX,
} from 'vue-icons-plus/lu'
import { useSnackbarStore } from '@/stores/snackbar'
import { useThemeStore } from '@/stores/theme'
import OnlineDownloadModal, {
  type OnlineDownloadTarget,
} from '../components/OnlineDownloadModal.vue'
import {
  onlineBookApi,
  type AggregatedSearchItemDto,
  type BookJsContextInput,
  type OnlineChapterItem,
} from '../onlineApi'

interface ReaderContext {
  shelfId?: string
  sourceId: string
  sourceName: string
  name: string
  author: string
  bookUrl: string
  tocUrl: string
  coverUrl?: string | null
  intro?: string | null
  currentChapterIndex: number
}

const route = useRoute()
const router = useRouter()
const themeStore = useThemeStore()
const snackbar = useSnackbarStore()

const readerCtx = ref<ReaderContext | null>(null)
const chapters = ref<OnlineChapterItem[]>([])
const activeIndex = ref(0)
const chapterTitle = ref('')
const paragraphs = ref<string[]>([])
const loadingToc = ref(false)
const loadingContent = ref(false)
const contentCached = ref(false)
const errorMsg = ref<string | null>(null)
const savingShelf = ref(false)

const tocOpen = ref(false)
const tocQuery = ref('')
const switchSourceOpen = ref(false)
const searchingAlts = ref(false)
const altCandidates = ref<AggregatedSearchItemDto[]>([])
const switchingKey = ref<string | null>(null)
const downloadOpen = ref(false)

const fontSize = ref(themeStore.reader.font_size || 18)
const lineHeight = ref(themeStore.reader.line_height || 1.85)
const maxWidthPx = ref(800)

const contentScrollRef = ref<HTMLElement | null>(null)

const readableChapters = computed(() =>
  chapters.value.filter((c) => !c.is_volume && Boolean(c.url)),
)

const filteredToc = computed(() => {
  const q = tocQuery.value.trim().toLowerCase()
  if (!q) return chapters.value
  return chapters.value.filter((c) => c.title.toLowerCase().includes(q))
})

const currentChapter = computed<OnlineChapterItem | null>(
  () => chapters.value[activeIndex.value] ?? null,
)

const prevReadableIndex = computed<number | null>(() => {
  for (let i = activeIndex.value - 1; i >= 0; i -= 1) {
    const ch = chapters.value[i]
    if (ch && !ch.is_volume && ch.url) return i
  }
  return null
})

const nextReadableIndex = computed<number | null>(() => {
  for (let i = activeIndex.value + 1; i < chapters.value.length; i += 1) {
    const ch = chapters.value[i]
    if (ch && !ch.is_volume && ch.url) return i
  }
  return null
})

const downloadTarget = computed<OnlineDownloadTarget | null>(() => {
  const c = readerCtx.value
  if (!c) return null
  return {
    sourceId: c.sourceId,
    sourceName: c.sourceName,
    bookName: c.name,
    author: c.author,
    bookUrl: c.bookUrl,
    tocUrl: c.tocUrl,
    coverUrl: c.coverUrl,
    intro: c.intro,
    totalChapters: readableChapters.value.length,
  }
})

function buildBookJsCtx(idx: number, title: string): BookJsContextInput {
  const c = readerCtx.value!
  return {
    name: c.name,
    author: c.author,
    book_url: c.bookUrl,
    toc_url: c.tocUrl,
    cover_url: c.coverUrl ?? null,
    intro: c.intro ?? null,
    dur_chapter_index: idx,
    dur_chapter_title: title,
  }
}

async function initReader(): Promise<void> {
  const shelfIdQuery = typeof route.query.shelfId === 'string' ? route.query.shelfId : ''
  const rawStored = sessionStorage.getItem('xmedia_online_reader_ctx')
  if (rawStored) {
    try {
      const parsed = JSON.parse(rawStored) as ReaderContext
      if (!shelfIdQuery || parsed.shelfId === shelfIdQuery) {
        readerCtx.value = parsed
      }
    } catch {
      // ignore invalid storage
    }
  }

  if (!readerCtx.value && shelfIdQuery) {
    try {
      const shelf = await onlineBookApi.listShelf()
      const found = shelf.find((b) => b.id === shelfIdQuery)
      if (found) {
        readerCtx.value = {
          shelfId: found.id,
          sourceId: found.source_id,
          sourceName: found.source_name,
          name: found.name,
          author: found.author,
          bookUrl: found.book_url,
          tocUrl: found.toc_url,
          coverUrl: found.cover_url,
          intro: found.intro,
          currentChapterIndex: found.current_chapter_index,
        }
      }
    } catch {
      // ignore
    }
  }

  if (!readerCtx.value) {
    void router.replace('/book/online')
    return
  }

  await loadTocAndOpen(readerCtx.value.currentChapterIndex || 0)
}

async function loadTocAndOpen(targetIdx: number): Promise<void> {
  const ctx = readerCtx.value
  if (!ctx) return
  loadingToc.value = true
  errorMsg.value = null
  try {
    const tocRes = await onlineBookApi.fetchToc(
      ctx.sourceId,
      ctx.tocUrl,
      buildBookJsCtx(targetIdx, ''),
    )
    chapters.value = tocRes.chapters
    if (tocRes.chapters.length === 0) {
      errorMsg.value = '目录为空，请检查书源状态或尝试换源'
      return
    }
    let idx = Math.max(0, Math.min(targetIdx, tocRes.chapters.length - 1))
    if (tocRes.chapters[idx]?.is_volume || !tocRes.chapters[idx]?.url) {
      const firstReadable = tocRes.chapters.findIndex((c) => !c.is_volume && Boolean(c.url))
      if (firstReadable >= 0) idx = firstReadable
    }
    await openChapter(idx, false)
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    loadingToc.value = false
  }
}

async function openChapter(idx: number, forceRefresh = false): Promise<void> {
  const ctx = readerCtx.value
  const ch = chapters.value[idx]
  if (!ctx || !ch || ch.is_volume || !ch.url) return

  activeIndex.value = idx
  tocOpen.value = false
  loadingContent.value = true
  errorMsg.value = null

  try {
    const res = await onlineBookApi.fetchContent(
      ctx.sourceId,
      ch.url,
      buildBookJsCtx(idx, ch.title),
      { index: ch.index, title: ch.title, url: ch.url },
      forceRefresh,
    )
    chapterTitle.value = res.content.title || ch.title
    paragraphs.value = res.content.content
      .split(/\n+/)
      .map((s) => s.trim())
      .filter(Boolean)
    contentCached.value = res.cached

    await nextTick()
    if (contentScrollRef.value) {
      contentScrollRef.value.scrollTop = 0
    }

    // Persist reading progress if in online shelf
    ctx.currentChapterIndex = idx
    sessionStorage.setItem('xmedia_online_reader_ctx', JSON.stringify(ctx))
    if (ctx.shelfId) {
      void onlineBookApi
        .updateShelfBook(ctx.shelfId, {
          current_chapter_index: idx,
          current_chapter_title: chapterTitle.value,
          total_chapters: chapters.value.length,
          last_chapter: chapters.value[chapters.value.length - 1]?.title ?? null,
        })
        .catch(() => {})
    }

    // Prefetch next chapter silently into server disk cache
    const nextIdx = nextReadableIndex.value
    if (nextIdx !== null) {
      const nextCh = chapters.value[nextIdx]
      if (nextCh && nextCh.url) {
        void onlineBookApi
          .fetchContent(
            ctx.sourceId,
            nextCh.url,
            buildBookJsCtx(nextIdx, nextCh.title),
            { index: nextCh.index, title: nextCh.title, url: nextCh.url },
            false,
          )
          .catch(() => {})
      }
    }
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    loadingContent.value = false
  }
}

async function handleSaveToShelf(): Promise<void> {
  const ctx = readerCtx.value
  if (!ctx || ctx.shelfId || savingShelf.value) return
  savingShelf.value = true
  try {
    const saved = await onlineBookApi.addToShelf({
      source_id: ctx.sourceId,
      name: ctx.name,
      author: ctx.author,
      book_url: ctx.bookUrl,
      toc_url: ctx.tocUrl,
      cover_url: ctx.coverUrl ?? null,
      intro: ctx.intro ?? null,
      last_chapter: chapters.value[chapters.value.length - 1]?.title ?? null,
      total_chapters: chapters.value.length,
    })
    await onlineBookApi.updateShelfBook(saved.id, {
      current_chapter_index: activeIndex.value,
      current_chapter_title: chapterTitle.value || null,
      total_chapters: chapters.value.length,
    })
    ctx.shelfId = saved.id
    sessionStorage.setItem('xmedia_online_reader_ctx', JSON.stringify(ctx))
    snackbar.show({ message: `已将《${ctx.name}》加入在线书架并记录当前进度` })
  } catch (err) {
    snackbar.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    savingShelf.value = false
  }
}

async function openSwitchSource(): Promise<void> {
  const ctx = readerCtx.value
  if (!ctx) return
  switchSourceOpen.value = true
  searchingAlts.value = true
  altCandidates.value = []
  try {
    const res = await onlineBookApi.search(ctx.name, undefined, 1)
    altCandidates.value = res.items
  } catch (err) {
    snackbar.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    searchingAlts.value = false
  }
}

async function handleSelectAltSource(item: AggregatedSearchItemDto): Promise<void> {
  const ctx = readerCtx.value
  if (!ctx) return
  const key = `${item.source_id}:${item.book.book_url}`
  switchingKey.value = key
  try {
    const detailRes = await onlineBookApi.fetchBookInfo(item.source_id, item.book.book_url, {
      name: item.book.name,
      author: item.book.author,
      book_url: item.book.book_url,
      toc_url: '',
      cover_url: item.book.cover_url,
      intro: item.book.intro,
    })
    ctx.sourceId = item.source_id
    ctx.sourceName = item.source_name
    ctx.bookUrl = item.book.book_url
    ctx.tocUrl = detailRes.detail.toc_url
    if (ctx.shelfId) {
      await onlineBookApi.updateShelfBook(ctx.shelfId, {
        source_id: ctx.sourceId,
        book_url: ctx.bookUrl,
        toc_url: ctx.tocUrl,
      })
    }
    sessionStorage.setItem('xmedia_online_reader_ctx', JSON.stringify(ctx))
    switchSourceOpen.value = false
    snackbar.show({ message: `已切换至书源「${item.source_name}」` })
    await loadTocAndOpen(activeIndex.value)
  } catch (err) {
    snackbar.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    switchingKey.value = null
  }
}

function handleKeydown(ev: KeyboardEvent): void {
  if (ev.target instanceof HTMLInputElement || ev.target instanceof HTMLTextAreaElement) return
  if (ev.key === 'ArrowLeft' && prevReadableIndex.value !== null) {
    ev.preventDefault()
    void openChapter(prevReadableIndex.value)
  } else if (ev.key === 'ArrowRight' && nextReadableIndex.value !== null) {
    ev.preventDefault()
    void openChapter(nextReadableIndex.value)
  }
}

onMounted(() => {
  window.addEventListener('keydown', handleKeydown)
  void initReader()
})

onBeforeUnmount(() => {
  window.removeEventListener('keydown', handleKeydown)
})
</script>

<template>
  <div class="flex h-screen flex-col bg-[var(--md-sys-color-surface)] text-[var(--md-sys-color-on-surface)]">
    <!-- Top Reader Bar -->
    <header
      class="flex h-14 shrink-0 items-center justify-between gap-2 border-b border-[var(--md-sys-color-outline-variant)]/45 bg-[var(--md-sys-color-surface-container)]/90 px-3 sm:px-5"
    >
      <div class="flex min-w-0 items-center gap-2">
        <button
          type="button"
          class="md-icon-btn md-icon-btn-sm md-state"
          title="返回在线书源"
          @click="router.push('/book/online')"
        >
          <LuArrowLeft class="h-4 w-4" />
        </button>

        <div v-if="readerCtx" class="min-w-0">
          <div class="flex items-center gap-2">
            <span class="truncate text-sm font-semibold">{{ readerCtx.name }}</span>
            <span class="hidden rounded-full bg-[var(--md-sys-color-primary-container)] px-2 py-0.5 text-[10px] font-medium text-[var(--md-sys-color-on-primary-container)] sm:inline">
              {{ readerCtx.sourceName }}
            </span>
          </div>
          <p class="truncate text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
            {{ chapterTitle || '加载中…' }}
            <span v-if="chapters.length > 0">
              · {{ activeIndex + 1 }} / {{ chapters.length }}
            </span>
            <span v-if="contentCached" class="ml-1 opacity-75">· 已缓存</span>
          </p>
        </div>
      </div>

      <div class="flex shrink-0 items-center gap-1">
        <!-- Font size quick controls -->
        <div class="hidden items-center gap-0.5 rounded-full bg-[var(--md-sys-color-surface-container-high)] px-1.5 py-0.5 md:flex">
          <button
            type="button"
            class="md-icon-btn md-icon-btn-sm md-state !h-7 !w-7"
            title="减小字号"
            @click="fontSize = Math.max(13, fontSize - 1)"
          >
            <LuMinus class="h-3.5 w-3.5" />
          </button>
          <span class="px-1 font-mono text-[11px]">{{ fontSize }}</span>
          <button
            type="button"
            class="md-icon-btn md-icon-btn-sm md-state !h-7 !w-7"
            title="增大字号"
            @click="fontSize = Math.min(30, fontSize + 1)"
          >
            <LuPlus class="h-3.5 w-3.5" />
          </button>
        </div>

        <button
          v-if="readerCtx && !readerCtx.shelfId"
          type="button"
          :disabled="savingShelf"
          class="md-btn md-btn-sm md-btn-tonal md-state"
          @click="handleSaveToShelf"
        >
          <LuBookMarked class="h-3.5 w-3.5" />
          <span class="hidden sm:inline">加入书架</span>
        </button>

        <button
          type="button"
          class="md-icon-btn md-icon-btn-sm md-state"
          title="重新拉取本章正文（忽略缓存）"
          :disabled="loadingContent"
          @click="openChapter(activeIndex, true)"
        >
          <LuRefreshCw class="h-4 w-4" :class="loadingContent ? 'animate-spin' : ''" />
        </button>

        <button
          type="button"
          class="md-icon-btn md-icon-btn-sm md-state"
          title="换源"
          @click="openSwitchSource"
        >
          <LuRepeat class="h-4 w-4" />
        </button>

        <button
          type="button"
          class="md-icon-btn md-icon-btn-sm md-state"
          title="下载 / 打包入库"
          @click="downloadOpen = true"
        >
          <LuDownload class="h-4 w-4" />
        </button>

        <button
          type="button"
          class="md-btn md-btn-sm md-btn-tonal md-state"
          @click="tocOpen = !tocOpen"
        >
          <LuList class="h-3.5 w-3.5" />
          <span class="hidden sm:inline">目录</span>
        </button>
      </div>
    </header>

    <!-- Main Reading Area -->
    <div
      ref="contentScrollRef"
      class="flex-1 overflow-y-auto px-4 py-8 sm:px-8"
    >
      <div
        class="mx-auto"
        :style="{ maxWidth: `${maxWidthPx}px` }"
      >
        <div
          v-if="loadingToc || loadingContent"
          class="flex flex-col items-center justify-center py-24 text-xs text-[var(--md-sys-color-on-surface-variant)]"
        >
          <LuLoader2 class="mb-3 h-6 w-6 animate-spin text-[var(--md-sys-color-primary)]" />
          <span>{{ loadingToc ? '正在获取章节目录…' : '正在拉取章节正文…' }}</span>
        </div>

        <div
          v-else-if="errorMsg"
          class="rounded-2xl bg-[var(--md-sys-color-error-container)] p-5 text-center text-xs text-[var(--md-sys-color-on-error-container)]"
        >
          <p class="font-semibold">加载失败</p>
          <p class="mt-1">{{ errorMsg }}</p>
          <div class="mt-4 flex justify-center gap-2">
            <button
              type="button"
              class="md-btn md-btn-sm md-btn-filled md-state"
              @click="currentChapter ? openChapter(activeIndex, true) : loadTocAndOpen(activeIndex)"
            >
              重试
            </button>
            <button
              type="button"
              class="md-btn md-btn-sm md-btn-tonal md-state"
              @click="openSwitchSource"
            >
              尝试换源
            </button>
          </div>
        </div>

        <article v-else class="pb-16">
          <h1
            class="mb-8 text-center font-bold tracking-tight"
            :style="{ fontSize: `${Math.round(fontSize * 1.3)}px` }"
          >
            {{ chapterTitle }}
          </h1>

          <div
            class="space-y-4 select-text"
            :style="{
              fontSize: `${fontSize}px`,
              lineHeight: String(lineHeight),
            }"
          >
            <p
              v-for="(para, idx) in paragraphs"
              :key="`p-${idx}`"
              class="indent-[2em] break-words"
            >
              {{ para }}
            </p>
          </div>

          <p
            v-if="paragraphs.length === 0"
            class="py-12 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
          >
            本章正文为空，可点击右上角刷新重试或切换书源。
          </p>

          <!-- Chapter Navigation Footer -->
          <div class="mt-12 flex items-center justify-between gap-3 border-t border-[var(--md-sys-color-outline-variant)]/45 pt-6">
            <button
              type="button"
              :disabled="prevReadableIndex === null"
              class="md-btn md-btn-tonal md-state disabled:opacity-40"
              @click="prevReadableIndex !== null && openChapter(prevReadableIndex)"
            >
              <LuChevronLeft class="h-4 w-4" />
              <span>上一章</span>
            </button>

            <button
              type="button"
              class="md-btn md-btn-text md-state"
              @click="tocOpen = true"
            >
              <LuList class="h-4 w-4" />
              <span>目录</span>
            </button>

            <button
              type="button"
              :disabled="nextReadableIndex === null"
              class="md-btn md-btn-filled md-state disabled:opacity-40"
              @click="nextReadableIndex !== null && openChapter(nextReadableIndex)"
            >
              <span>下一章</span>
              <LuChevronRight class="h-4 w-4" />
            </button>
          </div>
        </article>
      </div>
    </div>

    <!-- TOC Slide-Over Drawer -->
    <Teleport to="body">
      <div
        v-if="tocOpen"
        class="fixed inset-0 z-50 flex justify-end bg-black/50 backdrop-blur-[1px]"
        @click.self="tocOpen = false"
      >
        <div
          class="flex h-full w-full max-w-sm flex-col bg-[var(--md-sys-color-surface-container-low)] shadow-[var(--md-sys-elevation-3)]"
        >
          <div class="flex items-center justify-between border-b border-[var(--md-sys-color-outline-variant)]/45 px-4 py-3.5">
            <span class="text-sm font-semibold">目录 ({{ chapters.length }})</span>
            <button
              type="button"
              class="md-icon-btn md-icon-btn-sm md-state"
              @click="tocOpen = false"
            >
              <LuX class="h-4 w-4" />
            </button>
          </div>

          <div class="border-b border-[var(--md-sys-color-outline-variant)]/35 p-3">
            <div class="relative">
              <LuSearch class="pointer-events-none absolute top-1/2 left-3 h-3.5 w-3.5 -translate-y-1/2 text-[var(--md-sys-color-on-surface-variant)]" />
              <input
                v-model="tocQuery"
                type="text"
                placeholder="筛选章节标题…"
                class="md-input h-9 w-full !pl-9 text-xs"
              />
            </div>
          </div>

          <div class="flex-1 overflow-y-auto p-2">
            <button
              v-for="ch in filteredToc"
              :key="`toc-${ch.index}`"
              type="button"
              :disabled="ch.is_volume || !ch.url"
              class="flex w-full items-center justify-between gap-2 rounded-xl px-3 py-2 text-left text-xs transition"
              :class="
                ch.is_volume
                  ? 'mt-2 font-bold text-[var(--md-sys-color-primary)] opacity-90'
                  : ch.index === activeIndex
                    ? 'bg-[var(--md-sys-color-secondary-container)] font-semibold text-[var(--md-sys-color-on-secondary-container)]'
                    : 'text-[var(--md-sys-color-on-surface-variant)] hover:bg-[var(--md-sys-color-surface-container-high)] hover:text-[var(--md-sys-color-on-surface)]'
              "
              @click="openChapter(ch.index)"
            >
              <span class="truncate">{{ ch.title }}</span>
            </button>
          </div>
        </div>
      </div>
    </Teleport>

    <!-- Switch Source Modal -->
    <Teleport to="body">
      <div
        v-if="switchSourceOpen"
        class="fixed inset-0 z-50 flex items-center justify-center bg-black/55 p-4 backdrop-blur-[2px]"
        @click.self="switchSourceOpen = false"
      >
        <div
          class="flex max-h-[82vh] w-full max-w-lg flex-col overflow-hidden rounded-[28px] bg-[var(--md-sys-color-surface-container-low)] shadow-[var(--md-sys-elevation-3)]"
        >
          <div class="flex items-center justify-between border-b border-[var(--md-sys-color-outline-variant)]/45 px-5 py-4">
            <h2 class="text-sm font-semibold">换源 · {{ readerCtx?.name }}</h2>
            <button
              type="button"
              class="md-icon-btn md-icon-btn-sm md-state"
              @click="switchSourceOpen = false"
            >
              <LuX class="h-4 w-4" />
            </button>
          </div>

          <div class="flex-1 space-y-2 overflow-y-auto p-4 text-xs">
            <div v-if="searchingAlts" class="flex items-center justify-center py-10 text-[var(--md-sys-color-on-surface-variant)]">
              <LuLoader2 class="mr-2 h-4 w-4 animate-spin" />
              <span>正在聚合搜索其他书源…</span>
            </div>
            <div v-else-if="altCandidates.length === 0" class="py-10 text-center text-[var(--md-sys-color-on-surface-variant)]">
              暂未搜到其他书源的匹配条目
            </div>
            <button
              v-for="(item, idx) in altCandidates"
              :key="`alt-${item.source_id}-${idx}`"
              type="button"
              :disabled="switchingKey !== null"
              class="flex w-full items-center justify-between gap-3 rounded-2xl bg-[var(--md-sys-color-surface-container)] p-3 text-left transition hover:brightness-105"
              @click="handleSelectAltSource(item)"
            >
              <div class="min-w-0 flex-1">
                <div class="flex items-center gap-2">
                  <span class="truncate font-semibold text-[var(--md-sys-color-on-surface)]">
                    {{ item.book.name }}
                  </span>
                  <span class="shrink-0 rounded-full bg-[var(--md-sys-color-primary-container)] px-2 py-0.5 text-[10px] font-medium text-[var(--md-sys-color-on-primary-container)]">
                    {{ item.source_name }}
                  </span>
                </div>
                <p class="mt-0.5 truncate text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
                  {{ item.book.author || '佚名' }}
                  <span v-if="item.book.last_chapter"> · {{ item.book.last_chapter }}</span>
                </p>
              </div>
              <LuLoader2
                v-if="switchingKey === `${item.source_id}:${item.book.book_url}`"
                class="h-4 w-4 shrink-0 animate-spin"
              />
            </button>
          </div>
        </div>
      </div>
    </Teleport>

    <OnlineDownloadModal
      :open="downloadOpen"
      :target="downloadTarget"
      @close="downloadOpen = false"
    />
  </div>
</template>
