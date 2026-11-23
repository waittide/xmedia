import type { Component } from 'vue'
import { LuBookOpen, LuFilm, LuImage, LuMusic } from 'vue-icons-plus/lu'
import type { MediaKind } from '@/types/api'

export type MediaItemCardAspect = 'poster' | 'square' | 'video'
export type MediaItemCardLayout = 'vertical' | 'horizontal'
export type MediaItemCoverStyle = 'plain' | 'book' | 'album'

export function resolveDomainAspect(
  domain: MediaKind,
  override?: MediaItemCardAspect,
): MediaItemCardAspect {
  if (override) return override
  if (domain === 'music') return 'square'
  if (domain === 'video') return 'video'
  return 'poster'
}

export function resolveDomainCoverStyle(
  domain: MediaKind,
  override?: MediaItemCoverStyle,
): MediaItemCoverStyle {
  if (override) return override
  if (domain === 'book' || domain === 'comic') return 'book'
  if (domain === 'music') return 'album'
  return 'plain'
}

export function resolveDomainIcon(domain: MediaKind, override?: Component): Component {
  if (override) return override
  switch (domain) {
    case 'comic':
      return LuImage
    case 'music':
      return LuMusic
    case 'video':
      return LuFilm
    case 'book':
    default:
      return LuBookOpen
  }
}

export function aspectClassFor(aspect: MediaItemCardAspect): string {
  switch (aspect) {
    case 'square':
      return 'aspect-square'
    case 'video':
      return 'aspect-video'
    case 'poster':
    default:
      return 'aspect-[2/3]'
  }
}

export function mediaGridClassFor(aspect: MediaItemCardAspect): string {
  return aspect === 'video'
    ? 'grid grid-cols-1 gap-5 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4'
    : 'grid grid-cols-2 gap-5 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6'
}
