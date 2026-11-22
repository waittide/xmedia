import type { Component } from 'vue'
import type { LocationQuery, RouteLocationNormalizedLoaded } from 'vue-router'
import {
  LuBookOpen,
  LuClapperboard,
  LuDisc3,
  LuGlobe,
  LuInfo,
  LuKeyRound,
  LuLayers,
  LuLibrary,
  LuPalette,
  LuSlidersHorizontal,
} from 'vue-icons-plus/lu'
import { isMediaKind } from '@/sources'
import type { MediaKind } from '@/types/api'

export type SettingsSection =
  | 'appearance'
  | 'playback'
  | 'libraries'
  | 'sources'
  | 'account'
  | 'about'

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
    id: 'playback',
    labelKey: 'settings.navPlayback',
    labelZh: '阅读与播放',
    labelEn: 'Reading & Playback',
    summaryKey: 'settings.summaryPlayback',
    summaryZh: '书籍、漫画、音乐与视频默认体验',
    summaryEn: 'Reader, comic, audio, and video defaults',
    icon: LuSlidersHorizontal,
    testId: 'playback',
  },
  {
    id: 'libraries',
    labelKey: 'settings.navLibraries',
    labelZh: '媒体库',
    labelEn: 'Libraries',
    summaryKey: 'settings.summaryLibraries',
    summaryZh: '挂载文件夹、扫描与库管理',
    summaryEn: 'Mounted folders, scanning, and libraries',
    icon: LuLibrary,
    adminOnly: true,
    testId: 'libraries',
  },
  {
    id: 'sources',
    labelKey: 'settings.navSources',
    labelZh: '来源',
    labelEn: 'Sources',
    summaryKey: 'settings.summarySources',
    summaryZh: '在线数据源与远程服务器配置',
    summaryEn: 'Online data sources and remote servers',
    icon: LuGlobe,
    adminOnly: true,
    testId: 'sources',
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
    summaryZh: '版本、FFmpeg 状态与对外协议地址',
    summaryEn: 'Version, FFmpeg status, and protocol endpoints',
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
  isStandaloneApp: boolean,
): { section: SettingsSection; derivedHash: string; derivedKind: string } {
  const paramSection = typeof route.params.section === 'string' ? route.params.section : ''
  if (isSettingsSection(paramSection)) {
    return {
      section: paramSection,
      derivedHash: route.hash,
      derivedKind: readSingleQuery(route.query, 'kind'),
    }
  }

  const rawTab = readSingleQuery(route.query, 'tab')
  const rawDomain = readSingleQuery(route.query, 'domain')
  const domain = isMediaKind(rawDomain) ? rawDomain : null

  if (rawTab === 'general' || rawTab === 'about') {
    return { section: 'about', derivedHash: route.hash, derivedKind: '' }
  }

  if (rawTab === 'account' || rawTab === 'profile' || rawTab === 'users') {
    return { section: 'account', derivedHash: route.hash, derivedKind: '' }
  }

  if (
    rawTab === 'playback' ||
    rawTab === 'domain-general' ||
    rawTab === 'preferences' ||
    rawTab === 'reader'
  ) {
    return {
      section: 'playback',
      derivedHash: route.hash || (domain ? `#${domain}` : ''),
      derivedKind: '',
    }
  }

  if (rawTab === 'domain-protocols' || rawTab === 'protocols' || rawTab === 'opds') {
    if (isStandaloneApp && (domain === 'book' || domain === 'comic' || domain === 'music')) {
      return {
        section: 'sources',
        derivedHash: route.hash,
        derivedKind: 'remote',
      }
    }
    return {
      section: 'about',
      derivedHash: route.hash || '#protocols',
      derivedKind: '',
    }
  }

  if (rawTab === 'sources' || rawTab === 'domain-datasources' || rawTab === 'datasources') {
    return {
      section: 'sources',
      derivedHash: route.hash,
      derivedKind: readSingleQuery(route.query, 'kind'),
    }
  }

  if (rawTab === 'libraries' || rawTab === 'domain-libraries') {
    return { section: 'libraries', derivedHash: route.hash, derivedKind: '' }
  }

  return { section: 'appearance', derivedHash: route.hash, derivedKind: '' }
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

export function isLegacyDatasourcesDeepLink(route: RouteLocationNormalizedLoaded): boolean {
  const rawTab = readSingleQuery(route.query, 'tab')
  return rawTab === 'domain-datasources' || rawTab === 'datasources'
}
