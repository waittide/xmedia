import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import type { FloatingTaskItem } from '@/components/ui/FloatingTaskShelf.vue'
import type { MediaKind } from '@/types/api'
import { toolApi } from './api'
import type { AutoScrapeOptionsDto, MediaFileDetail, MediaFileEntry } from './types'

export interface ScrapeSubItem {
  rootId: string
  relPath: string
  fileName: string
  mediaKind: MediaKind
  status: 'queued' | 'running' | 'matched' | 'unmatched' | 'failed' | 'cancelled'
  confidence?: number
  error?: string
}

export interface ToolTaskEntry {
  id: string
  kind: 'browse' | 'rename' | 'scrape'
  domain?: MediaKind
  title: string
  strategyLabel?: string
  scrapeOptions?: AutoScrapeOptionsDto
  status: 'queued' | 'running' | 'completed' | 'failed'
  processed: number
  total: number | null
  matchedCount?: number
  unmatchedCount?: number
  failedCount?: number
  detail: string
  visible: boolean
  ephemeral: boolean
  cancellable?: boolean
  cancelled?: boolean
  subItems?: ScrapeSubItem[]
  startedAt: number
  finishedAt?: number
}

type FileScrapedListener = (detail: MediaFileDetail) => void

const SCRAPE_CONCURRENCY = 2

export const useToolTaskStore = defineStore('tool-tasks', () => {
  const entries = ref<ToolTaskEntry[]>([])
  const collapsed = ref(false)
  const timers = new Map<string, ReturnType<typeof setTimeout>>()
  const fileListeners = new Set<FileScrapedListener>()

  let workerRunning = false
  const pendingScrapeBatchIds: string[] = []

  function onFileScraped(fn: FileScrapedListener): () => void {
    fileListeners.add(fn)
    return () => {
      fileListeners.delete(fn)
    }
  }

  function notifyFileScraped(detail: MediaFileDetail): void {
    for (const fn of fileListeners) {
      try {
        fn(detail)
      } catch {
        // ignore listener error
      }
    }
  }

  function clearTaskTimer(id: string): void {
    const existing = timers.get(id)
    if (existing) {
      clearTimeout(existing)
      timers.delete(id)
    }
  }

  function startTask(opts: {
    id: string
    kind: 'browse' | 'rename' | 'scrape'
    domain?: MediaKind
    title: string
    total?: number | null
    detail?: string
    ephemeral?: boolean
  }): string {
    clearTaskTimer(opts.id)
    const ephemeral = Boolean(opts.ephemeral)
    const total = opts.total ?? null
    const showImmediately = !ephemeral || (total !== null && total >= 30)

    const next: ToolTaskEntry = {
      id: opts.id,
      kind: opts.kind,
      domain: opts.domain,
      title: opts.title,
      status: 'running',
      processed: 0,
      total,
      detail: opts.detail ?? '正在准备…',
      visible: showImmediately,
      ephemeral,
      startedAt: Date.now(),
    }

    const idx = entries.value.findIndex((e) => e.id === opts.id)
    if (idx >= 0) {
      entries.value[idx] = next
    } else {
      entries.value.unshift(next)
    }

    if (!showImmediately) {
      timers.set(
        opts.id,
        setTimeout(() => {
          const found = entries.value.find((e) => e.id === opts.id)
          if (found && found.status === 'running') {
            found.visible = true
          }
          timers.delete(opts.id)
        }, 220),
      )
    } else {
      collapsed.value = false
    }

    return opts.id
  }

  function updateTask(
    id: string,
    patch: { processed: number; total: number | null; detail?: string },
  ): void {
    const found = entries.value.find((e) => e.id === id)
    if (!found || (found.status !== 'running' && found.status !== 'queued')) return
    found.processed = patch.processed
    found.total = patch.total
    if (patch.detail !== undefined && patch.detail !== '') {
      found.detail = patch.detail
    }
    if (!found.visible && ((patch.total !== null && patch.total >= 30) || patch.processed >= 30)) {
      clearTaskTimer(id)
      found.visible = true
    }
  }

  function completeTask(id: string, summary: string): void {
    clearTaskTimer(id)
    const idx = entries.value.findIndex((e) => e.id === id)
    if (idx < 0) return
    const found = entries.value[idx]!
    if (!found.visible) {
      entries.value.splice(idx, 1)
      return
    }
    found.status = 'completed'
    found.finishedAt = Date.now()
    if (found.total !== null) {
      found.processed = found.total
    }
    found.detail = summary

    if (found.ephemeral) {
      timers.set(
        id,
        setTimeout(() => {
          const curIdx = entries.value.findIndex((e) => e.id === id && e.status === 'completed')
          if (curIdx >= 0) {
            entries.value.splice(curIdx, 1)
          }
          timers.delete(id)
        }, 2400),
      )
    }
  }

  function failTask(id: string, message: string): void {
    clearTaskTimer(id)
    const idx = entries.value.findIndex((e) => e.id === id)
    if (idx < 0) return
    const found = entries.value[idx]!
    if (found.ephemeral) {
      entries.value.splice(idx, 1)
      return
    }
    found.visible = true
    found.status = 'failed'
    found.finishedAt = Date.now()
    found.detail = message
    collapsed.value = false
  }

  /**
   * Enqueues a background waterfall scrape task:
   * Priority order: Local non-empty metadata -> High-priority provider -> Low-priority provider
   */
  function formatStrategyLabel(opts?: AutoScrapeOptionsDto): string | undefined {
    if (!opts) return undefined
    const modeMap: Record<string, string> = {
      strict: '严格',
      standard: '标准',
      loose: '宽松',
    }
    const parts: string[] = [modeMap[opts.match_mode ?? 'standard'] ?? '标准']
    parts.push(opts.only_fill_missing === false ? '强制覆盖' : '仅补空')
    if (opts.organize_by_template) parts.push('自动归档')
    return parts.join(' · ')
  }

  function enqueueScrapeTask(
    files: MediaFileEntry[],
    labelHint?: string,
    options?: AutoScrapeOptionsDto,
  ): string | null {
    if (files.length === 0) return null
    const id = `scrape:queue:${Date.now()}-${Math.random().toString(36).slice(2, 6)}`
    const dom = files[0]?.media_kind
    const subItems: ScrapeSubItem[] = files.map((f) => ({
      rootId: f.root_id,
      relPath: f.rel_path,
      fileName: f.file_name,
      mediaKind: f.media_kind,
      status: 'queued',
    }))

    const title =
      files.length === 1
        ? `刮削 · ${files[0]!.file_name}`
        : `刮削任务 (${files.length} 项)${labelHint ? ` · ${labelHint}` : ''}`

    const entry: ToolTaskEntry = {
      id,
      kind: 'scrape',
      domain: dom,
      title,
      strategyLabel: formatStrategyLabel(options),
      scrapeOptions: options,
      status: workerRunning ? 'queued' : 'running',
      processed: 0,
      total: files.length,
      matchedCount: 0,
      unmatchedCount: 0,
      failedCount: 0,
      detail: workerRunning ? '排队等待执行…' : `准备刮削 ${files[0]!.file_name}…`,
      visible: true,
      ephemeral: false,
      cancellable: true,
      cancelled: false,
      subItems,
      startedAt: Date.now(),
    }

    entries.value.unshift(entry)
    collapsed.value = false
    pendingScrapeBatchIds.push(id)
    void pumpScrapeWorker()
    return id
  }

  async function pumpScrapeWorker(): Promise<void> {
    if (workerRunning) return
    workerRunning = true
    try {
      while (pendingScrapeBatchIds.length > 0) {
        const batchId = pendingScrapeBatchIds.shift()!
        const task = entries.value.find((e) => e.id === batchId)
        if (!task || !task.subItems) continue
        if (task.cancelled) {
          settleBatchTask(task)
          continue
        }

        task.status = 'running'
        let cursor = 0
        const items = task.subItems

        async function runLane(): Promise<void> {
          while (cursor < items.length) {
            if (task!.cancelled) break
            const idx = cursor
            cursor += 1
            const sub = items[idx]!
            if (sub.status !== 'queued') continue

            sub.status = 'running'
            task!.detail = sub.fileName
            try {
              const res = await toolApi.autoScrapeFile(
                sub.rootId,
                sub.relPath,
                sub.mediaKind,
                task!.scrapeOptions,
              )
              sub.confidence = res.confidence
              if (res.matched) {
                sub.status = 'matched'
                task!.matchedCount = (task!.matchedCount ?? 0) + 1
                notifyFileScraped(res.detail)
              } else {
                sub.status = 'unmatched'
                task!.unmatchedCount = (task!.unmatchedCount ?? 0) + 1
              }
            } catch (err) {
              sub.status = 'failed'
              sub.error = err instanceof Error ? err.message : String(err)
              task!.failedCount = (task!.failedCount ?? 0) + 1
            } finally {
              task!.processed = items.filter(
                (it) =>
                  it.status === 'matched' ||
                  it.status === 'unmatched' ||
                  it.status === 'failed' ||
                  it.status === 'cancelled',
              ).length
            }
          }
        }

        const laneCount = Math.min(SCRAPE_CONCURRENCY, items.length)
        await Promise.all(Array.from({ length: laneCount }, () => runLane()))
        settleBatchTask(task)
      }
    } finally {
      workerRunning = false
    }
  }

  function settleBatchTask(task: ToolTaskEntry): void {
    if (task.subItems && task.cancelled) {
      for (const it of task.subItems) {
        if (it.status === 'queued') {
          it.status = 'cancelled'
        }
      }
    }
    const matched = task.matchedCount ?? 0
    const unmatched = task.unmatchedCount ?? 0
    const failed = task.failedCount ?? 0
    const cancelledCount =
      task.subItems?.filter((it) => it.status === 'cancelled').length ?? 0

    task.finishedAt = Date.now()
    task.cancellable = false

    const parts: string[] = [`补齐 ${matched} 项`]
    if (unmatched > 0) parts.push(`未命中 ${unmatched} 项`)
    if (failed > 0) parts.push(`失败 ${failed} 项`)
    if (cancelledCount > 0) parts.push(`已取消 ${cancelledCount} 项`)

    task.detail = parts.join(' · ')
    if (failed > 0 && matched === 0) {
      task.status = 'failed'
    } else {
      task.status = 'completed'
    }
  }

  function cancelTask(id: string): void {
    const found = entries.value.find((e) => e.id === id)
    if (!found) return
    found.cancelled = true
    if (found.status === 'queued') {
      const qIdx = pendingScrapeBatchIds.indexOf(id)
      if (qIdx >= 0) pendingScrapeBatchIds.splice(qIdx, 1)
      settleBatchTask(found)
    }
  }

  function retryTaskFailedOrUnmatched(id: string): void {
    const found = entries.value.find((e) => e.id === id)
    if (!found || !found.subItems) return
    const retryTargets = found.subItems.filter(
      (it) => it.status === 'failed' || it.status === 'unmatched' || it.status === 'cancelled',
    )
    if (retryTargets.length === 0) return

    for (const it of retryTargets) {
      it.status = 'queued'
      it.error = undefined
    }
    found.cancelled = false
    found.cancellable = true
    found.finishedAt = undefined
    found.unmatchedCount = 0
    found.failedCount = 0
    found.processed = found.subItems.filter((it) => it.status === 'matched').length
    found.status = workerRunning ? 'queued' : 'running'
    found.detail = '正在重试未完成项…'
    pendingScrapeBatchIds.push(found.id)
    void pumpScrapeWorker()
  }

  function removeTask(id: string): void {
    clearTaskTimer(id)
    const found = entries.value.find((e) => e.id === id)
    if (found && (found.status === 'running' || found.status === 'queued')) {
      cancelTask(id)
    }
    entries.value = entries.value.filter((e) => e.id !== id)
  }

  function clearCompleted(): void {
    for (const item of entries.value) {
      if (item.status === 'completed') clearTaskTimer(item.id)
    }
    entries.value = entries.value.filter(
      (e) => e.status === 'running' || e.status === 'queued' || e.status === 'failed',
    )
  }

  function dismissAll(): void {
    for (const item of entries.value) {
      if (item.status !== 'running' && item.status !== 'queued') clearTaskTimer(item.id)
    }
    entries.value = entries.value.filter((e) => e.status === 'running' || e.status === 'queued')
  }

  const activeCount = computed(
    () => entries.value.filter((e) => e.status === 'running' || e.status === 'queued').length,
  )

  const persistentTasks = computed(() => entries.value.filter((e) => !e.ephemeral || e.visible))

  const shelfItems = computed<FloatingTaskItem[]>(() =>
    entries.value
      .filter((e) => e.visible)
      .map((e) => {
        let progress: number | null = null
        if (e.status === 'completed') {
          progress = 100
        } else if (e.total !== null && e.total > 0) {
          progress = Math.min(100, Math.round((e.processed / e.total) * 100))
        }

        let subtitle = e.detail
        if (e.status === 'running') {
          if (e.total !== null && e.total > 0) {
            subtitle = e.detail
              ? `${e.processed.toLocaleString()} / ${e.total.toLocaleString()} · ${e.detail}`
              : `${e.processed.toLocaleString()} / ${e.total.toLocaleString()}`
          } else if (e.processed > 0) {
            subtitle = `已清点 ${e.processed.toLocaleString()} 项 · ${e.detail}`
          } else {
            subtitle = `正在清点目录文件 · ${e.detail}`
          }
        } else if (e.status === 'queued') {
          subtitle = `排队中 (${e.total ?? 0} 项)`
        }

        return {
          id: e.id,
          kind: e.kind,
          title: e.title,
          subtitle,
          status: e.status,
          progress,
          clickable: true,
        }
      }),
  )

  return {
    entries,
    persistentTasks,
    activeCount,
    collapsed,
    shelfItems,
    onFileScraped,
    startTask,
    updateTask,
    completeTask,
    failTask,
    enqueueScrapeTask,
    cancelTask,
    retryTaskFailedOrUnmatched,
    removeTask,
    clearCompleted,
    dismissAll,
  }
})
