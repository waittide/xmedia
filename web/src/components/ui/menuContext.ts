import type { InjectionKey } from 'vue'

export interface MdMenuContext {
  close: () => void
}

export const MD_MENU_CONTEXT_KEY: InjectionKey<MdMenuContext> = Symbol('md-menu-context')
