import { nextTick, onBeforeUnmount, watch, type Ref } from 'vue'

const FOCUSABLE_SELECTOR = [
  'a[href]',
  'area[href]',
  'button:not([disabled]):not([tabindex="-1"])',
  'input:not([disabled]):not([type="hidden"]):not([tabindex="-1"])',
  'select:not([disabled]):not([tabindex="-1"])',
  'textarea:not([disabled]):not([tabindex="-1"])',
  'iframe',
  '[tabindex]:not([tabindex="-1"])',
  '[contenteditable="true"]',
].join(',')

function isVisibleElement(el: HTMLElement): boolean {
  if (el.offsetParent !== null) return true
  const style = window.getComputedStyle(el)
  return style.position === 'fixed' && style.visibility !== 'hidden' && style.display !== 'none'
}

function getFocusableElements(container: HTMLElement): HTMLElement[] {
  return Array.from(container.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR)).filter(
    (el) => !el.hasAttribute('disabled') && el.getAttribute('aria-hidden') !== 'true' && isVisibleElement(el),
  )
}

export interface UseFocusTrapOptions {
  initialFocusRef?: Ref<HTMLElement | null>
  restoreFocus?: boolean
}

export function useFocusTrap(
  containerRef: Ref<HTMLElement | null>,
  active: () => boolean,
  options: UseFocusTrapOptions = {},
): void {
  const { initialFocusRef, restoreFocus = true } = options
  let previouslyFocused: HTMLElement | null = null
  let listenerBound = false

  function onKeydown(ev: KeyboardEvent): void {
    if (ev.key !== 'Tab' || !active()) return
    const container = containerRef.value
    if (!container) return

    const focusables = getFocusableElements(container)
    if (focusables.length === 0) {
      ev.preventDefault()
      container.focus()
      return
    }

    const first = focusables[0]!
    const last = focusables[focusables.length - 1]!
    const current = document.activeElement as HTMLElement | null

    if (ev.shiftKey) {
      if (!current || current === first || !container.contains(current)) {
        ev.preventDefault()
        last.focus()
      }
    } else {
      if (!current || current === last || !container.contains(current)) {
        ev.preventDefault()
        first.focus()
      }
    }
  }

  function activate(): void {
    if (typeof document === 'undefined') return
    previouslyFocused =
      document.activeElement instanceof HTMLElement ? document.activeElement : null
    if (!listenerBound) {
      document.addEventListener('keydown', onKeydown, true)
      listenerBound = true
    }
    void nextTick(() => {
      if (!active()) return
      const container = containerRef.value
      if (!container) return
      const explicit = initialFocusRef?.value
      if (explicit && container.contains(explicit)) {
        explicit.focus()
        return
      }
      if (document.activeElement && container.contains(document.activeElement)) {
        return
      }
      const focusables = getFocusableElements(container)
      if (focusables.length > 0) {
        focusables[0]!.focus()
      } else {
        if (!container.hasAttribute('tabindex')) {
          container.setAttribute('tabindex', '-1')
        }
        container.focus()
      }
    })
  }

  function deactivate(): void {
    if (listenerBound && typeof document !== 'undefined') {
      document.removeEventListener('keydown', onKeydown, true)
      listenerBound = false
    }
    if (restoreFocus && previouslyFocused && previouslyFocused.isConnected) {
      const target = previouslyFocused
      previouslyFocused = null
      target.focus()
    } else {
      previouslyFocused = null
    }
  }

  watch(
    active,
    (isActive, wasActive) => {
      if (isActive && !wasActive) {
        activate()
      } else if (!isActive && wasActive) {
        deactivate()
      }
    },
    { immediate: true },
  )

  onBeforeUnmount(() => {
    deactivate()
  })
}
