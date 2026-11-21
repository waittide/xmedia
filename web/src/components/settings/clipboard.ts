import { onBeforeUnmount, ref, type Ref } from 'vue'

export async function copyToClipboard(val: string): Promise<boolean> {
  if (!val) return false
  try {
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(val)
      return true
    }
    throw new Error('clipboard API unavailable')
  } catch {
    const ta = document.createElement('textarea')
    ta.value = val
    ta.style.position = 'fixed'
    ta.style.opacity = '0'
    document.body.appendChild(ta)
    ta.select()
    let ok = false
    try {
      ok = document.execCommand('copy')
    } catch {
      ok = false
    } finally {
      document.body.removeChild(ta)
    }
    return ok
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
    const ok = await copyToClipboard(val)
    if (!ok) return
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
