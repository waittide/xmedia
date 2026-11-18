import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import type { FloatingTaskItem } from '@/components/ui/FloatingTaskShelf.vue'

export interface ToolTaskEntry {
  id: string
  kind: 'browse' | 'rename' | 'scrape'
  title: string
  status: 'running' | 'completed' | 'failed'
  processed: number
  total: number | null
  detail: string
  visible: boolean
  ephemeral: boolean
  startedAt: number
}

export const useToolTaskStore = defineStore('tool-tasks', () => {
  const entries = ref<ToolTaskEntry[]>([])
  const collapsed = ref(false)
  const timers = new Map<string, ReturnType<typeof setTimeout>>()

  function clearTaskTimer(id: string): void {
    const existing = timers.get(id)
    if (existing) {
      clearTimeout(existing)
      timers.delete(id)
    }
  }

  /**
   * Start tracking a task.
   * Ephemeral tasks (directory browse / rename preview) only surface in the floating
   * panel if they take > 220ms or involve >= 30 files, avoiding visual noise on tiny folders.
   */
  function startTask(opts: {
    id: string
    kind: 'browse' | 'rename' | 'scrape'
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
    if (!found || found.status !== 'running') return
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
    found.detail = message
    collapsed.value = false
  }

  function clearCompleted(): void {
    for (const item of entries.value) {
      if (item.status === 'completed') clearTaskTimer(item.id)
    }
    entries.value = entries.value.filter((e) => e.status === 'running' || e.status === 'failed')
  }

  function dismissAll(): void {
    for (const item of entries.value) {
      if (item.status !== 'running') clearTaskTimer(item.id)
    }
    entries.value = entries.value.filter((e) => e.status === 'running')
  }

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
        }

        return {
          id: e.id,
          kind: e.kind,
          title: e.title,
          subtitle,
          status: e.status,
          progress,
        }
      }),
  )

  return {
    entries,
    collapsed,
    shelfItems,
    startTask,
    updateTask,
    completeTask,
    failTask,
    clearCompleted,
    dismissAll,
  }
})
