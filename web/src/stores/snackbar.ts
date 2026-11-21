import { defineStore } from 'pinia'
import { ref } from 'vue'

export interface SnackbarItem {
  id: number
  message: string
  actionLabel?: string
  onAction?: () => void | Promise<void>
  tone?: 'default' | 'error'
}

export interface ShowSnackbarOptions {
  message: string
  actionLabel?: string
  onAction?: () => void | Promise<void>
  durationMs?: number
  tone?: 'default' | 'error'
}

let nextId = 1

export const useSnackbarStore = defineStore('snackbar', () => {
  const current = ref<SnackbarItem | null>(null)
  const runningAction = ref(false)
  let timer: ReturnType<typeof setTimeout> | null = null

  function clearTimer(): void {
    if (timer !== null) {
      clearTimeout(timer)
      timer = null
    }
  }

  function dismiss(): void {
    clearTimer()
    current.value = null
  }

  function show(opts: ShowSnackbarOptions | string): void {
    clearTimer()
    const normalized: ShowSnackbarOptions =
      typeof opts === 'string' ? { message: opts } : opts

    current.value = {
      id: nextId++,
      message: normalized.message,
      actionLabel: normalized.actionLabel,
      onAction: normalized.onAction,
      tone: normalized.tone ?? 'default',
    }

    const isMultiline = normalized.message.includes('\n')
    const duration =
      normalized.durationMs ?? (isMultiline ? 9000 : normalized.actionLabel ? 6000 : 3800)
    if (duration > 0) {
      timer = setTimeout(() => {
        current.value = null
        timer = null
      }, duration)
    }
  }

  async function triggerAction(): Promise<void> {
    const item = current.value
    if (!item?.onAction || runningAction.value) return
    clearTimer()
    runningAction.value = true
    try {
      await item.onAction()
    } finally {
      runningAction.value = false
      if (current.value?.id === item.id) {
        current.value = null
      }
    }
  }

  return {
    current,
    runningAction,
    show,
    dismiss,
    triggerAction,
  }
})
