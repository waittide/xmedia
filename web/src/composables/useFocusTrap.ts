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
  const style = window.getComputedStyle(el)
  if (
    style.display === 'none' ||
    style.visibility === 'hidden' ||
    style.visibility === 'collapse'
  ) {
    return false
  }
  if (el.closest('[inert], [aria-hidden="true"]')) return false
  if (el.offsetParent !== null) return true
  return style.position === 'fixed'
}

function getFocusableElements(container: HTMLElement): HTMLElement[] {
  return Array.from(container.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR)).filter(
    (el) => !el.hasAttribute('disabled') && el.getAttribute('aria-hidden') !== 'true' && isVisibleElement(el),
  )
}

const activeTrapStack: symbol[] = []

function isTopTrap(id: symbol): boolean {
  return activeTrapStack.length > 0 && activeTrapStack[activeTrapStack.length - 1] === id
}

function pushTrap(id: symbol): void {
  const idx = activeTrapStack.indexOf(id)
  if (idx !== -1) activeTrapStack.splice(idx, 1)
  activeTrapStack.push(id)
}

function removeTrap(id: symbol): void {
  const idx = activeTrapStack.indexOf(id)
  if (idx !== -1) activeTrapStack.splice(idx, 1)
}

export interface UseFocusTrapOptions {
  initialFocusRef?: Ref<HTMLElement | null>
  restoreFocus?: boolean
  /** Reactive key whose change re-applies initial focus while trap stays active (e.g. queued dialogs). */
  focusKey?: () => unknown
}

export function useFocusTrap(
  containerRef: Ref<HTMLElement | null>,
  active: () => boolean,
  options: UseFocusTrapOptions = {},
): void {
  const { initialFocusRef, restoreFocus = true, focusKey } = options
  const trapId = Symbol('focus-trap')
  let previouslyFocused: HTMLElement | null = null
  let listenerBound = false

  function focusInitial(): void {
    void nextTick(() => {
      if (!active() || !isTopTrap(trapId)) return
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

  function onKeydown(ev: KeyboardEvent): void {
    if (ev.key !== 'Tab' || !active() || !isTopTrap(trapId)) return
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

  function onFocusIn(ev: FocusEvent): void {
    if (!active() || !isTopTrap(trapId)) return
    const container = containerRef.value
    if (!container) return
    const target = ev.target
    if (!(target instanceof HTMLElement) || container.contains(target)) return
    // Allow teleported popovers (MdSelect listbox / MdMenu / higher dialog) to receive focus
    if (target.closest('[role="listbox"], [role="menu"], [role="dialog"]')) return
    const focusables = getFocusableElements(container)
    if (focusables.length > 0) {
      focusables[0]!.focus()
    } else {
      container.focus()
    }
  }

  function activate(): void {
    if (typeof document === 'undefined') return
    previouslyFocused =
      document.activeElement instanceof HTMLElement ? document.activeElement : null
    pushTrap(trapId)
    if (!listenerBound) {
      document.addEventListener('keydown', onKeydown, true)
      document.addEventListener('focusin', onFocusIn, true)
      listenerBound = true
    }
    focusInitial()
  }

  function deactivate(): void {
    removeTrap(trapId)
    if (listenerBound && typeof document !== 'undefined') {
      document.removeEventListener('keydown', onKeydown, true)
      document.removeEventListener('focusin', onFocusIn, true)
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

  if (focusKey) {
    watch(focusKey, (next, prev) => {
      if (next !== prev && active()) {
        pushTrap(trapId)
        focusInitial()
      }
    })
  }

  onBeforeUnmount(() => {
    deactivate()
  })
}
