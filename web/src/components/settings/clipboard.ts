import { onBeforeUnmount, ref, type Ref } from 'vue'

export async function copyToClipboard(val: string): Promise<void> {
  if (!val) return
  try {
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(val)
      return
    }
    throw new Error('clipboard API unavailable')
  } catch {
    const ta = document.createElement('textarea')
    ta.value = val
    ta.style.position = 'fixed'
    ta.style.opacity = '0'
    document.body.appendChild(ta)
    ta.select()
    document.execCommand('copy')
    document.body.removeChild(ta)
  }
}

export function useCopyFeedback(resetMs = 1500): {
  copiedKey: Ref<string>
  copyText: (key: string, val: string) => Promise<void>
} {
  const copiedKey = ref('')
  let timer: ReturnType<typeof setTimeout> | null = null

  async function copyText(key: string, val: string): Promise<void> {
    if (!val) return
    await copyToClipboard(val)
    copiedKey.value = key
    if (timer !== null) clearTimeout(timer)
    timer = setTimeout(() => {
      if (copiedKey.value === key) copiedKey.value = ''
      timer = null
    }, resetMs)
  }

  onBeforeUnmount(() => {
    if (timer !== null) {
      clearTimeout(timer)
      timer = null
    }
  })

  return { copiedKey, copyText }
}
