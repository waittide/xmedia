import { computed, onScopeDispose, ref } from 'vue'

export type ActionFeedbackState = 'idle' | 'running' | 'done' | 'error'

export interface RunFeedbackOptions {
  /** Minimum visible spin duration in ms so sub-10ms local responses still animate smoothly. */
  minSpinMs?: number
  /** Duration in ms to display the completion checkmark state. */
  doneMs?: number
  /** Whether to rethrow errors after transitioning to the 'error' state. */
  rethrow?: boolean
}

function sleep(ms: number): Promise<void> {
  if (ms <= 0) return Promise.resolve()
  return new Promise((resolve) => setTimeout(resolve, ms))
}

export function useActionFeedback(defaults: RunFeedbackOptions = {}) {
  const state = ref<ActionFeedbackState>('idle')
  let resetTimer: ReturnType<typeof setTimeout> | null = null
  let disposed = false

  function clearResetTimer(): void {
    if (resetTimer !== null) {
      clearTimeout(resetTimer)
      resetTimer = null
    }
  }

  function scheduleReset(ms: number): void {
    clearResetTimer()
    if (disposed) return
    if (ms > 0) {
      resetTimer = setTimeout(() => {
        if (!disposed) {
          state.value = 'idle'
        }
        resetTimer = null
      }, ms)
    } else {
      state.value = 'idle'
    }
  }

  async function run<T>(
    task: () => Promise<T> | T,
    options: RunFeedbackOptions = {},
  ): Promise<T | undefined> {
    if (state.value === 'running' || disposed) return undefined
    const minSpinMs = options.minSpinMs ?? defaults.minSpinMs ?? 420
    const doneMs = options.doneMs ?? defaults.doneMs ?? 1100
    const rethrow = options.rethrow ?? defaults.rethrow ?? false

    clearResetTimer()
    state.value = 'running'

    try {
      const [result] = await Promise.all([Promise.resolve().then(task), sleep(minSpinMs)])
      if (!disposed) {
        state.value = 'done'
        scheduleReset(doneMs)
      }
      return result
    } catch (err) {
      if (!disposed) {
        state.value = 'error'
        scheduleReset(1500)
      }
      if (rethrow) throw err
      return undefined
    }
  }

  function flashDone(doneMs = defaults.doneMs ?? 1100): void {
    if (disposed) return
    state.value = 'done'
    scheduleReset(doneMs)
  }

  onScopeDispose(() => {
    disposed = true
    clearResetTimer()
  })

  return {
    state,
    running: computed(() => state.value === 'running'),
    done: computed(() => state.value === 'done'),
    errored: computed(() => state.value === 'error'),
    run,
    flashDone,
  }
}
