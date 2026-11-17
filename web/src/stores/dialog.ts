import { ref } from 'vue'
import { defineStore } from 'pinia'

export interface DialogState {
  mode: 'confirm' | 'prompt'
  title: string
  message: string
  placeholder?: string
  danger?: boolean
  confirmText?: string
  cancelText?: string
  resolve: (val: boolean | string | null) => void
}

export const useDialogStore = defineStore('dialog', () => {
  const active = ref<DialogState | null>(null)
  const inputValue = ref<string>('')

  function confirm(options: {
    title?: string
    message?: string
    description?: string
    danger?: boolean
    confirmText?: string
    cancelText?: string
  }): Promise<boolean> {
    return new Promise((resolve) => {
      active.value = {
        mode: 'confirm',
        title: options.title ?? '',
        message: options.message ?? options.description ?? '',
        danger: options.danger ?? false,
        confirmText: options.confirmText,
        cancelText: options.cancelText,
        resolve: (val) => resolve(val === true),
      }
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
    inputValue.value = options.initialValue ?? options.defaultValue ?? ''
    return new Promise((resolve) => {
      active.value = {
        mode: 'prompt',
        title: options.title ?? '',
        message: options.message ?? options.description ?? '',
        placeholder: options.placeholder ?? '',
        confirmText: options.confirmText,
        cancelText: options.cancelText,
        resolve: (val) => resolve(typeof val === 'string' ? val : null),
      }
    })
  }

  function submit(): void {
    if (!active.value) return
    const current = active.value
    active.value = null
    if (current.mode === 'confirm') {
      current.resolve(true)
    } else {
      current.resolve(inputValue.value)
    }
  }

  function cancel(): void {
    if (!active.value) return
    const current = active.value
    active.value = null
    current.resolve(current.mode === 'confirm' ? false : null)
  }

  return {
    active,
    inputValue,
    confirm,
    prompt,
    submit,
    cancel,
  }
})
