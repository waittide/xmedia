import { onBeforeUnmount, watch, type WatchSource } from 'vue'

interface EscapeEntry {
  id: symbol
  handler: () => boolean | void
}

const stack: EscapeEntry[] = []
let listenerAttached = false

function handleWindowKeydown(ev: KeyboardEvent): void {
  if (ev.key !== 'Escape' || ev.defaultPrevented || stack.length === 0) return

  for (let i = stack.length - 1; i >= 0; i -= 1) {
    const entry = stack[i]
    if (!entry) continue
    const handled = entry.handler()
    if (handled !== false) {
      ev.preventDefault()
      ev.stopImmediatePropagation()
      return
    }
  }
}

function ensureListener(): void {
  if (listenerAttached || typeof window === 'undefined') return
  window.addEventListener('keydown', handleWindowKeydown)
  listenerAttached = true
}

export function pushEscapeHandler(handler: () => boolean | void): () => void {
  ensureListener()
  const id = Symbol('escape-layer')
  stack.push({ id, handler })
  return () => {
    const idx = stack.findIndex((item) => item.id === id)
    if (idx !== -1) {
      stack.splice(idx, 1)
    }
  }
}

/**
 * 轻量全局 Escape 浮层栈（LIFO）。
 * 按下 Escape 时仅触发并消费栈顶最上层的一个浮层关闭回调。
 */
export function useEscapeStack() {
  ensureListener()
  const cleanups = new Set<() => void>()

  function push(handler: () => boolean | void): () => void {
    const remove = pushEscapeHandler(handler)
    const wrapped = () => {
      remove()
      cleanups.delete(wrapped)
    }
    cleanups.add(wrapped)
    return wrapped
  }

  function register(active: WatchSource<boolean>, onClose: () => void): void {
    let removeCurrent: (() => void) | null = null

    const stopWatch = watch(
      active,
      (isActive) => {
        if (isActive) {
          if (!removeCurrent) {
            removeCurrent = push(() => {
              onClose()
            })
          }
        } else if (removeCurrent) {
          removeCurrent()
          removeCurrent = null
        }
      },
      { immediate: true },
    )

    const cleanup = () => {
      stopWatch()
      if (removeCurrent) {
        removeCurrent()
        removeCurrent = null
      }
      cleanups.delete(cleanup)
    }
    cleanups.add(cleanup)
  }

  onBeforeUnmount(() => {
    for (const fn of Array.from(cleanups)) {
      fn()
    }
    cleanups.clear()
  })

  return {
    push,
    register,
  }
}
