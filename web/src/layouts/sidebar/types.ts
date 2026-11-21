import type { Component } from 'vue'

export const SIDEBAR_PREVIEW_LIMIT = 5
export const FOLDED_STORAGE_KEY = 'xmedia.sidebar_folded_groups'

export interface PrimaryNavItem {
  key: string
  label: string
  icon: Component
  active: boolean
  onClick: () => void
}

export interface SidebarSubItem {
  id: string
  label: string
  count?: number | string
  icon: Component
  active: boolean
  scanning?: boolean
  onClick: () => void
  onDelete?: () => void
}

export interface SidebarGroupDescriptor {
  id: string
  key: string
  label: string
  icon: Component
  overviewActive: boolean
  onOpenOverview: () => void
  canCreate?: boolean
  createTitle?: string
  createPlaceholder?: string
  items: SidebarSubItem[]
}

export type SidebarGoFn = (target: string | { path: string; query?: Record<string, string> }) => void
