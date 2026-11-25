import type { ScraperPrioritiesDto } from './types'

export const CN_WEBNOVEL_PROVIDER_IDS = new Set<string>([
  'qidian',
  'fanqie',
  'qimao',
  'zongheng',
  '17k',
  'jjwxc',
  'sfacg',
  'ciweimao',
  'faloo',
])

export const MUSIC_PROVIDER_IDS = new Set<string>([
  'qq',
  'qq_music',
  'netease',
  'kugou',
  'kuwo',
  'migu',
  'taihe',
  'qishui',
  'musicbrainz',
  'itunes',
  'lrclib',
])

export const ADULT_PROVIDER_IDS = new Set<string>(['nhentai', 'hanime1'])

export const SCRAPER_IMAGE_PROXY_DOMAINS = [
  'doubanio.com',
  'xinhuashudian.com',
  'nhentai.net',
  'hanime1.me',
  'hembed.com',
  'hanime1.com',
] as const

export const DEFAULT_SCRAPER_PRIORITIES: ScraperPrioritiesDto = {
  book: [
    'douban',
    'neodb',
    'xinhua',
    'qidian',
    'fanqie',
    'qimao',
    '17k',
    'ciweimao',
    'sfacg',
    'faloo',
    'jjwxc',
    'bangumi',
    'anilist',
    'openlibrary',
  ],
  comic: ['bangumi', 'douban', 'neodb', 'anilist', 'mangadex', 'nhentai'],
  music: [
    'qq',
    'netease',
    'kugou',
    'kuwo',
    'migu',
    'taihe',
    'qishui',
    'itunes',
    'bangumi',
    'musicbrainz',
    'lrclib',
  ],
  video: ['tmdb', 'bangumi', 'hanime1'],
}
