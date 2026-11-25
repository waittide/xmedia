import type { MediaKind } from '@/types/api'
import type { MediaFileEntry, RenameTemplatesDto } from './types'

export const DEFAULT_RENAME_TEMPLATES: RenameTemplatesDto = {
  book: '{author}/{series}/{subseries}/<Vol.{volume:2} >{title} ({year})/{title} - {author}',
  comic: '{series}/{series} <Vol.{volume:2}> <Ch.{number:2}> {title}',
  music: '{album_artist}/{album} ({year})/{track:2} - {title}',
  video_movie: '{title} ({year})/{title} ({year})',
  video_episode: '{show} ({year})/Season {season:2}/{show} - S{season:2}E{episode:2} - {title}',
}

export interface DomainRenameTemplateSpec {
  tokens: string[]
  quickChips: string[]
  spec: string
  sample: string
}

export const DOMAIN_RENAME_TEMPLATE_SPECS: Record<MediaKind, DomainRenameTemplateSpec> = {
  book: {
    tokens: [
      '{author}',
      '{series}',
      '{subseries}',
      '{volume:2}',
      '{title}',
      '{publisher}',
      '{year}',
      '{isbn}',
    ],
    quickChips: [
      '{author}',
      '{series}',
      '{subseries}',
      '<Vol.{volume:2} >',
      '{title}',
      '{publisher}',
      '{year}',
      '{isbn}',
    ],
    spec: 'Calibre / Kavita 规范',
    sample: '刘慈欣/地球往事/Vol.01 三体 (2008)/三体 - 刘慈欣.epub',
  },
  comic: {
    tokens: [
      '{series}',
      '{series_group}',
      '{volume:2}',
      '{number:2}',
      '{title}',
      '{publisher}',
      '{year}',
    ],
    quickChips: [
      '{series}',
      '{series_group}',
      '<Vol.{volume:2}>',
      '<Ch.{number:2}>',
      '{title}',
      '{publisher}',
      '{year}',
    ],
    spec: 'Komga / Kavita 规范',
    sample: '葬送的芙莉莲/葬送的芙莉莲 Vol.01 Ch.01 冒险的终点.cbz',
  },
  music: {
    tokens: [
      '{album_artist}',
      '{artist}',
      '{album}',
      '{disc}',
      '{track:2}',
      '{title}',
      '{year}',
      '{genre}',
    ],
    quickChips: [
      '{album_artist}',
      '{artist}',
      '{album}',
      '{disc}',
      '{track:2}',
      '{title}',
      '{year}',
      '{genre}',
    ],
    spec: 'MusicBrainz Picard / Navidrome 规范',
    sample: '米津玄師/LOST CORNER (2024)/01 - RED OUT.flac',
  },
  video: {
    tokens: [
      '{show}',
      '{season:2}',
      '{episode:2}',
      '{title}',
      '{original_title}',
      '{year}',
      '{jellyfin_id}',
      '{tmdbid}',
    ],
    quickChips: [
      '{show}',
      '{season:2}',
      '{episode:2}',
      '{title}',
      '{original_title}',
      '{year}',
      '{jellyfin_id}',
      '{tmdbid}',
    ],
    spec: 'Jellyfin / Kodi 官方目录规范',
    sample: '葬送的芙莉莲 (2023)/Season 01/葬送的芙莉莲 - S01E01 - 冒险的终点.mkv',
  },
}

export interface BatchTitleTemplateToken {
  token: string
  label: string
}

export const BATCH_TITLE_TEMPLATE_TOKENS: BatchTitleTemplateToken[] = [
  { token: '{filename}', label: '文件名' },
  { token: '{title}', label: '原标题' },
  { token: '{creator}', label: '作者/艺人' },
  { token: '{series}', label: '系列/专辑' },
  { token: '{year}', label: '年份' },
  { token: '{counter:2}', label: '序号(01)' },
]

/**
 * Converts unified UI `{token}` syntax into the backend `batch_metadata` `${token}` syntax,
 * while keeping legacy `${token}` inputs working cleanly without double-prefixing.
 */
export function normalizeTitleTemplateForBackend(rawTemplate: string): string {
  return rawTemplate.replace(
    /(?<!\$)\{(filename|title|creator|series|year|counter(?::[23])?)\}/g,
    (_match, key: string) => `\${${key}}`,
  )
}

export function renderTitleTemplatePreview(params: {
  template: string
  entry: MediaFileEntry
  creatorOverride?: string
  seriesOverride?: string
  yearOverride?: string
  counter?: number
}): string | null {
  const normalized = normalizeTitleTemplateForBackend(params.template.trim())
  if (!normalized) return null
  const { entry } = params
  const stem = entry.file_name.replace(/\.[^.]+$/, '')
  const count = params.counter ?? 1
  const out = normalized
    .replaceAll('${filename}', stem)
    .replaceAll('${title}', entry.display_title || stem)
    .replaceAll('${creator}', params.creatorOverride?.trim() || entry.display_subtitle || '')
    .replaceAll('${series}', params.seriesOverride?.trim() || '')
    .replaceAll(
      '${year}',
      params.yearOverride?.trim() || (entry.year != null ? String(entry.year) : ''),
    )
    .replaceAll('${counter}', String(count))
    .replaceAll('${counter:2}', String(count).padStart(2, '0'))
    .replaceAll('${counter:3}', String(count).padStart(3, '0'))
    .trim()
  return out || null
}
