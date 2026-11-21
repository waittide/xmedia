import type { RouteLocationNormalizedLoaded } from 'vue-router'
import {
  LuDisc3,
  LuFolderHeart,
  LuLibrary,
  LuListMusic,
  LuMic,
  LuRadio,
  LuSparkles,
  LuTag,
  LuTv,
} from 'vue-icons-plus/lu'
import type { useMusicStore } from '@/domains/music/store'
import type { PlaylistSummaryDto } from '@/domains/music/types'
import type { useVideoStore } from '@/domains/video/store'
import type { VideoShelfDto } from '@/domains/video/types'
import type { SidebarGoFn, SidebarGroupDescriptor } from './types'

export interface MusicGroupBuilderContext {
  route: RouteLocationNormalizedLoaded
  musicStore: ReturnType<typeof useMusicStore>
  musicT: (key: string) => string
  go: SidebarGoFn
  openMusicCatalog: (extraQuery?: Record<string, string>) => Promise<void>
  canEditPlaylist: (pl: PlaylistSummaryDto) => boolean
  onDeletePlaylist: (pl: PlaylistSummaryDto) => Promise<void>
}

export function buildMusicSidebarGroup(
  id: string,
  key: string,
  label: string,
  ctx: MusicGroupBuilderContext,
): SidebarGroupDescriptor | null {
  const { route, musicStore, musicT, go, openMusicCatalog } = ctx
  switch (key) {
    case 'artists':
      return {
        id,
        key,
        label,
        icon: LuMic,
        overviewActive: route.name === 'music-songs' && route.query.tab === 'artists',
        onOpenOverview: () => go({ path: '/music/songs', query: { tab: 'artists' } }),
        items: musicStore.artists.map((ar) => ({
          id: ar.id,
          label: ar.name,
          count: ar.track_count,
          icon: LuMic,
          active: route.name === 'music-artist-detail' && route.params.id === ar.id,
          onClick: () => go(`/music/artists/${ar.id}`),
        })),
      }
    case 'tags':
      return {
        id,
        key,
        label,
        icon: LuTag,
        overviewActive: route.name === 'music-tags-overview',
        onOpenOverview: () => go('/music/tags'),
        items: musicStore.tags.map((tg) => ({
          id: tg.name,
          label: `#${tg.name}`,
          count: tg.song_count,
          icon: LuTag,
          active: route.name === 'music-songs' && route.query.tag === tg.name,
          onClick: () => void openMusicCatalog({ tag: tg.name }),
        })),
      }
    case 'albums':
      return {
        id,
        key,
        label,
        icon: LuDisc3,
        overviewActive: route.name === 'music-songs' && route.query.tab === 'albums',
        onOpenOverview: () => go({ path: '/music/songs', query: { tab: 'albums' } }),
        items: musicStore.albums.map((al) => ({
          id: al.id,
          label: al.title,
          count: al.track_count,
          icon: LuDisc3,
          active: route.name === 'music-album-detail' && route.params.id === al.id,
          onClick: () => go(`/music/albums/${al.id}`),
        })),
      }
    case 'genres':
      return {
        id,
        key,
        label,
        icon: LuRadio,
        overviewActive: route.name === 'music-genres-overview',
        onOpenOverview: () => go('/music/genres'),
        items: musicStore.genres.map((g) => ({
          id: g.name,
          label: g.name,
          count: g.song_count,
          icon: LuRadio,
          active: route.name === 'music-songs' && route.query.genre === g.name,
          onClick: () => void openMusicCatalog({ genre: g.name }),
        })),
      }
    case 'playlists':
      return {
        id,
        key,
        label,
        icon: LuListMusic,
        overviewActive: route.name === 'music-playlists-overview',
        onOpenOverview: () => go('/music/playlists'),
        canCreate: true,
        createTitle: musicT('nav.newPlaylist'),
        createPlaceholder: musicT('nav.playlistNamePlaceholder'),
        items: musicStore.playlists.map((pl) => ({
          id: pl.id,
          label: pl.name,
          count: pl.track_count,
          icon: LuListMusic,
          active: route.path === `/music/playlists/${pl.id}`,
          onClick: () => go(`/music/playlists/${pl.id}`),
          onDelete: ctx.canEditPlaylist(pl) ? () => void ctx.onDeletePlaylist(pl) : undefined,
        })),
      }
    case 'libraries':
      return {
        id,
        key,
        label,
        icon: LuLibrary,
        overviewActive: route.name === 'music-libraries-overview',
        onOpenOverview: () => go('/music/libraries'),
        items: musicStore.libraries.map((lib) => ({
          id: lib.id,
          label: lib.name,
          count: lib.track_count,
          icon: LuLibrary,
          active: route.name === 'music-songs' && route.query.library === lib.id,
          scanning: lib.scan_status.state === 'scanning',
          onClick: () => void openMusicCatalog({ library: lib.id }),
        })),
      }
    default:
      return null
  }
}

export interface VideoGroupBuilderContext {
  route: RouteLocationNormalizedLoaded
  videoStore: ReturnType<typeof useVideoStore>
  videoT: (key: string) => string
  go: SidebarGoFn
  openVideoCatalog: (extraQuery?: Record<string, string>) => Promise<void>
  onDeleteShelf: (sh: VideoShelfDto) => Promise<void>
}

export function buildVideoSidebarGroup(
  id: string,
  key: string,
  label: string,
  ctx: VideoGroupBuilderContext,
): SidebarGroupDescriptor | null {
  const { route, videoStore, videoT, go, openVideoCatalog } = ctx
  switch (key) {
    case 'shows':
      return {
        id,
        key,
        label,
        icon: LuTv,
        overviewActive: route.name === 'video-catalog' && route.query.tab === 'shows',
        onOpenOverview: () => go({ path: '/video/catalog', query: { tab: 'shows' } }),
        items: videoStore.shows.map((sh) => ({
          id: sh.id,
          label: sh.title,
          count: sh.episode_count,
          icon: LuTv,
          active: route.name === 'video-show-detail' && route.params.id === sh.id,
          onClick: () => go(`/video/shows/${sh.id}`),
        })),
      }
    case 'genres':
      return {
        id,
        key,
        label,
        icon: LuSparkles,
        overviewActive: route.name === 'video-genres-overview',
        onOpenOverview: () => go('/video/genres'),
        items: videoStore.genres.map((g) => ({
          id: g.name,
          label: g.name,
          count: g.count,
          icon: LuSparkles,
          active: route.name === 'video-catalog' && route.query.genre === g.name,
          onClick: () => void openVideoCatalog({ genre: g.name }),
        })),
      }
    case 'tags':
      return {
        id,
        key,
        label,
        icon: LuTag,
        overviewActive: route.name === 'video-tags-overview',
        onOpenOverview: () => go('/video/tags'),
        items: videoStore.tags.map((tg) => ({
          id: tg.name,
          label: `#${tg.name}`,
          count: tg.count,
          icon: LuTag,
          active: route.name === 'video-catalog' && route.query.tag === tg.name,
          onClick: () => void openVideoCatalog({ tag: tg.name }),
        })),
      }
    case 'shelves':
      return {
        id,
        key,
        label,
        icon: LuFolderHeart,
        overviewActive: route.name === 'video-shelves-overview',
        onOpenOverview: () => go('/video/shelves'),
        canCreate: true,
        createTitle: videoT('nav.newShelf'),
        createPlaceholder: videoT('nav.shelfNamePlaceholder'),
        items: videoStore.shelves.map((sh) => ({
          id: sh.id,
          label: sh.name,
          count: sh.video_count,
          icon: LuFolderHeart,
          active: route.path === `/video/shelves/${sh.id}`,
          onClick: () => go(`/video/shelves/${sh.id}`),
          onDelete: () => void ctx.onDeleteShelf(sh),
        })),
      }
    case 'libraries':
      return {
        id,
        key,
        label,
        icon: LuLibrary,
        overviewActive: route.name === 'video-libraries-overview',
        onOpenOverview: () => go('/video/libraries'),
        items: videoStore.libraries.map((lib) => ({
          id: lib.id,
          label: lib.name,
          count: lib.video_count,
          icon: LuLibrary,
          active: route.name === 'video-catalog' && route.query.library === lib.id,
          scanning: lib.scan_status.state === 'scanning',
          onClick: () => void openVideoCatalog({ library: lib.id }),
        })),
      }
    default:
      return null
  }
}
