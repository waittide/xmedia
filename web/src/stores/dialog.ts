import { ref } from 'vue'
import { defineStore } from 'pinia'

export interface DialogState {
  mode: 'confirm' | 'prompt'
  title: string
  message: string
  placeholder?: string
  initialValue?: string
  danger?: boolean
  confirmText?: string
  cancelText?: string
  resolve: (val: boolean | string | null) => void
}

export const useDialogStore = defineStore('dialog', () => {
  const active = ref<DialogState | null>(null)
  const queue = ref<DialogState[]>([])
  const inputValue = ref<string>('')

  function activateNext(): void {
    if (active.value || queue.value.length === 0) return
    const next = queue.value.shift() ?? null
    active.value = next
    inputValue.value = next?.mode === 'prompt' ? (next.initialValue ?? '') : ''
  }

  function enqueue(item: DialogState): void {
    if (!active.value) {
      active.value = item
      inputValue.value = item.mode === 'prompt' ? (item.initialValue ?? '') : ''
    } else {
      queue.value.push(item)
    }
  }

  function confirm(options: {
    title?: string
    message?: string
    description?: string
    danger?: boolean
    confirmText?: string
    cancelText?: string
  }): Promise<boolean> {
    return new Promise((resolve) => {
      enqueue({
        mode: 'confirm',
        title: options.title ?? '',
        message: options.message ?? options.description ?? '',
        danger: options.danger ?? false,
        confirmText: options.confirmText,
        cancelText: options.cancelText,
        resolve: (val) => resolve(val === true),
      })
    })
  }

  function prompt(options: {
    title?: string
    message?: string
    description?: string
    placeholder?: string
    initialValue?: string
    defaultValue?: string
    confirmText?: string
    cancelText?: string
  }): Promise<string | null> {
    return new Promise((resolve) => {
      enqueue({
        mode: 'prompt',
        title: options.title ?? '',
        message: options.message ?? options.description ?? '',
        placeholder: options.placeholder ?? '',
        initialValue: options.initialValue ?? options.defaultValue ?? '',
        confirmText: options.confirmText,
        cancelText: options.cancelText,
        resolve: (val) => resolve(typeof val === 'string' ? val : null),
      })
    })
  }

  function submit(): void {
    if (!active.value) return
    const current = active.value
    const value = inputValue.value
    active.value = null
    if (current.mode === 'confirm') {
      current.resolve(true)
    } else {
      current.resolve(value)
    }
    activateNext()
  }

  function cancel(): void {
    if (!active.value) return
    const current = active.value
    active.value = null
    current.resolve(current.mode === 'confirm' ? false : null)
    activateNext()
  }

  return {
    active,
    queue,
    inputValue,
    confirm,
    prompt,
    submit,
    cancel,
  }
})
