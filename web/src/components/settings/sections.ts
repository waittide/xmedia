import type { Component } from 'vue'
import type { LocationQuery, RouteLocationNormalizedLoaded } from 'vue-router'
import {
  LuBookOpen,
  LuClapperboard,
  LuDisc3,
  LuInfo,
  LuKeyRound,
  LuLayers,
  LuPalette,
} from 'vue-icons-plus/lu'
import { isMediaKind } from '@/sources'
import type { MediaKind } from '@/types/api'

export type SettingsSection = 'appearance' | 'account' | 'about'

export interface SettingsSectionDef {
  id: SettingsSection
  labelKey: string
  labelZh: string
  labelEn: string
  summaryKey: string
  summaryZh: string
  summaryEn: string
  icon: Component
  adminOnly?: boolean
  /** `data-settings-tab` attribute value */
  testId: string
}

export const SETTINGS_SECTIONS: readonly SettingsSectionDef[] = [
  {
    id: 'appearance',
    labelKey: 'settings.navAppearance',
    labelZh: '外观',
    labelEn: 'Appearance',
    summaryKey: 'settings.summaryAppearance',
    summaryZh: '主题模式、动态配色与语言',
    summaryEn: 'Theme mode, color palette, and language',
    icon: LuPalette,
    testId: 'appearance',
  },
  {
    id: 'account',
    labelKey: 'settings.navAccountSecurity',
    labelZh: '用户与安全',
    labelEn: 'Account & Security',
    summaryKey: 'settings.summaryAccount',
    summaryZh: '个人资料、密码、API Key 与成员管理',
    summaryEn: 'Profile, password, API Key, and users',
    icon: LuKeyRound,
    testId: 'account',
  },
  {
    id: 'about',
    labelKey: 'settings.navAbout',
    labelZh: '关于',
    labelEn: 'About',
    summaryKey: 'settings.summaryAbout',
    summaryZh: '版本、已启用媒体域与 FFmpeg 状态',
    summaryEn: 'Version, enabled domains, and FFmpeg status',
    icon: LuInfo,
    testId: 'about',
  },
]

export const DOMAIN_ICONS: Record<MediaKind, Component> = {
  book: LuBookOpen,
  comic: LuLayers,
  music: LuDisc3,
  video: LuClapperboard,
}

export const SOURCES_ADVANCED_STORAGE_KEY = 'xmedia.sources.advanced'

const SECTION_SET = new Set<string>(SETTINGS_SECTIONS.map((s) => s.id))

export function isSettingsSection(value: unknown): value is SettingsSection {
  return typeof value === 'string' && SECTION_SET.has(value)
}

export function readSingleQuery(query: LocationQuery, key: string): string {
  const raw = query[key]
  if (typeof raw === 'string') return raw.trim()
  if (Array.isArray(raw)) {
    const first = raw.find((v): v is string => typeof v === 'string' && v.trim().length > 0)
    return first ? first.trim() : ''
  }
  return ''
}

export function hasExplicitSectionInRoute(route: RouteLocationNormalizedLoaded): boolean {
  const paramSection = typeof route.params.section === 'string' ? route.params.section : ''
  if (isSettingsSection(paramSection)) return true
  const rawTab = readSingleQuery(route.query, 'tab')
  return rawTab.length > 0
}

export function resolveSectionFromRoute(
  route: RouteLocationNormalizedLoaded,
): SettingsSection {
  const paramSection = typeof route.params.section === 'string' ? route.params.section : ''
  if (isSettingsSection(paramSection)) {
    return paramSection
  }

  const rawTab = readSingleQuery(route.query, 'tab')
  if (rawTab === 'general' || rawTab === 'about') {
    return 'about'
  }
  if (rawTab === 'account' || rawTab === 'profile' || rawTab === 'users') {
    return 'account'
  }
  return 'appearance'
}

export function resolveDomainFromQuery(
  route: RouteLocationNormalizedLoaded,
  enabledModules: MediaKind[],
  fallbackDomain: MediaKind,
): MediaKind {
  const rawDomain = readSingleQuery(route.query, 'domain')
  if (isMediaKind(rawDomain) && enabledModules.includes(rawDomain)) {
    return rawDomain
  }
  if (enabledModules.includes(fallbackDomain)) {
    return fallbackDomain
  }
  return enabledModules[0] ?? 'book'
}
