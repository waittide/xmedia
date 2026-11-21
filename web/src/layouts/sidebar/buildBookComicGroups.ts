import type { RouteLocationNormalizedLoaded } from 'vue-router'
import {
  LuBookmark,
  LuFolder,
  LuLibrary,
  LuPalette,
  LuSparkles,
  LuTag,
  LuUser,
} from 'vue-icons-plus/lu'
import type { NavSection as BookNavSection, useCatalogStore } from '@/domains/book/store'
import type { BookBookmarkItemDto } from '@/domains/book/types'
import {
  resolveLibrarySeriesCount,
  type useSidebarStore as useComicSidebarStore,
} from '@/domains/comic/store'
import type { SidebarGoFn, SidebarGroupDescriptor } from './types'

export interface BookGroupBuilderContext {
  route: RouteLocationNormalizedLoaded
  bookStore: ReturnType<typeof useCatalogStore>
  bookT: (key: string) => string
  go: SidebarGoFn
  isBookSectionActive: (section: BookNavSection, id?: string) => boolean
  openBookSection: (section: BookNavSection, id?: string) => Promise<void>
  openBookBookmark: (item: BookBookmarkItemDto) => void
  onDeleteShelf: (id: string, name: string) => Promise<void>
  onDeleteBookmark: (item: BookBookmarkItemDto) => Promise<void>
}

export function buildBookSidebarGroup(
  id: string,
  key: string,
  label: string,
  ctx: BookGroupBuilderContext,
): SidebarGroupDescriptor | null {
  const { route, bookStore, bookT, go, isBookSectionActive, openBookSection } = ctx
  switch (key) {
    case 'authors':
      return {
        id,
        key,
        label,
        icon: LuUser,
        overviewActive: route.name === 'book-authors-overview',
        onOpenOverview: () => go('/book/authors'),
        items: bookStore.authors.map((au) => ({
          id: au.name,
          label: au.name,
          count: au.count,
          icon: LuUser,
          active: isBookSectionActive('author', au.name),
          onClick: () => void openBookSection('author', au.name),
        })),
      }
    case 'tags':
      return {
        id,
        key,
        label,
        icon: LuTag,
        overviewActive: route.name === 'book-tags-overview',
        onOpenOverview: () => go('/book/tags'),
        items: bookStore.tags.map((tg) => ({
          id: tg.name,
          label: tg.name,
          count: tg.count,
          icon: LuTag,
          active: isBookSectionActive('tag', tg.name),
          onClick: () => void openBookSection('tag', tg.name),
        })),
      }
    case 'shelves':
      return {
        id,
        key,
        label,
        icon: LuFolder,
        overviewActive: route.name === 'book-shelves-overview',
        onOpenOverview: () => go('/book/shelves'),
        canCreate: true,
        createTitle: bookT('nav.newShelf'),
        createPlaceholder: bookT('nav.shelfNamePlaceholder'),
        items: bookStore.shelves.map((sh) => ({
          id: sh.id,
          label: sh.name,
          count: sh.book_count,
          icon: LuFolder,
          active: isBookSectionActive('shelf', sh.id),
          onClick: () => void openBookSection('shelf', sh.id),
          onDelete: sh.is_favorite ? undefined : () => void ctx.onDeleteShelf(sh.id, sh.name),
        })),
      }
    case 'libraries':
      return {
        id,
        key,
        label,
        icon: LuLibrary,
        overviewActive: route.name === 'book-libraries-overview',
        onOpenOverview: () => go('/book/libraries'),
        items: bookStore.libraries.map((lib) => ({
          id: lib.id,
          label: lib.name,
          count: lib.book_count,
          icon: LuLibrary,
          active: isBookSectionActive('library', lib.id),
          scanning: lib.scan_status.state === 'scanning',
          onClick: () => void openBookSection('library', lib.id),
        })),
      }
    case 'bookmarks':
      return {
        id,
        key,
        label,
        icon: LuBookmark,
        overviewActive: route.name === 'book-bookmarks',
        onOpenOverview: () => go('/book/bookmarks'),
        items: bookStore.bookmarks.map((bm) => ({
          id: bm.bookmark.id,
          label: `${bm.book_title} · ${bm.bookmark.title}`,
          count: `${Math.round(bm.bookmark.location.progress_percent)}%`,
          icon: LuBookmark,
          active: false,
          onClick: () => ctx.openBookBookmark(bm),
          onDelete: () => void ctx.onDeleteBookmark(bm),
        })),
      }
    default:
      return null
  }
}

export interface ComicGroupBuilderContext {
  route: RouteLocationNormalizedLoaded
  comicSidebarStore: ReturnType<typeof useComicSidebarStore>
  comicT: (key: string) => string
  go: SidebarGoFn
  activeComicLibraryId: string
  activeComicShelfId: string
  activeComicTag: string
  activeComicGenre: string
  activeComicWriter: string
  activeComicPenciller: string
  onDeleteShelf: (id: string, name: string) => Promise<void>
  onDeleteBookmark: (id: string, label: string) => Promise<void>
}

export function buildComicSidebarGroup(
  id: string,
  key: string,
  label: string,
  ctx: ComicGroupBuilderContext,
): SidebarGroupDescriptor | null {
  const { route, comicSidebarStore, comicT, go } = ctx
  const comicTagsSection =
    typeof route.query.section === 'string' && route.query.section ? route.query.section : 'genres'

  switch (key) {
    case 'writers':
      return {
        id,
        key,
        label,
        icon: LuUser,
        overviewActive: route.name === 'comic-tags' && comicTagsSection === 'writers',
        onOpenOverview: () => go({ path: '/comic/tags', query: { section: 'writers' } }),
        items: comicSidebarStore.writers.map((w) => ({
          id: w.name,
          label: w.name,
          count: w.count,
          icon: LuUser,
          active: ctx.activeComicWriter === w.name,
          onClick: () => go({ path: '/comic/series', query: { writer: w.name } }),
        })),
      }
    case 'pencillers':
      return {
        id,
        key,
        label,
        icon: LuPalette,
        overviewActive: route.name === 'comic-tags' && comicTagsSection === 'pencillers',
        onOpenOverview: () => go({ path: '/comic/tags', query: { section: 'pencillers' } }),
        items: comicSidebarStore.pencillers.map((p) => ({
          id: p.name,
          label: p.name,
          count: p.count,
          icon: LuPalette,
          active: ctx.activeComicPenciller === p.name,
          onClick: () => go({ path: '/comic/series', query: { penciller: p.name } }),
        })),
      }
    case 'genres':
      return {
        id,
        key,
        label,
        icon: LuSparkles,
        overviewActive: route.name === 'comic-tags' && comicTagsSection === 'genres',
        onOpenOverview: () => go({ path: '/comic/tags', query: { section: 'genres' } }),
        items: comicSidebarStore.genres.map((g) => ({
          id: g.name,
          label: g.name,
          count: g.count,
          icon: LuSparkles,
          active: ctx.activeComicGenre === g.name,
          onClick: () => go({ path: '/comic/series', query: { genre: g.name } }),
        })),
      }
    case 'tags':
      return {
        id,
        key,
        label,
        icon: LuTag,
        overviewActive: route.name === 'comic-tags' && comicTagsSection === 'tags',
        onOpenOverview: () => go({ path: '/comic/tags', query: { section: 'tags' } }),
        items: comicSidebarStore.tags.map((tg) => ({
          id: tg.name,
          label: `#${tg.name}`,
          count: tg.count,
          icon: LuTag,
          active: ctx.activeComicTag === tg.name,
          onClick: () => go({ path: '/comic/series', query: { tag: tg.name } }),
        })),
      }
    case 'shelves':
      return {
        id,
        key,
        label,
        icon: LuFolder,
        overviewActive: route.name === 'comic-shelves',
        onOpenOverview: () => go('/comic/shelves'),
        canCreate: true,
        createTitle: comicT('shelves.newShelf'),
        createPlaceholder: comicT('shelves.shelfNamePlaceholder'),
        items: comicSidebarStore.shelves.map((s) => ({
          id: s.shelf.id,
          label: s.shelf.name,
          count: s.series.length,
          icon: LuFolder,
          active: ctx.activeComicShelfId === s.shelf.id,
          onClick: () => go({ path: '/comic/series', query: { shelf_id: s.shelf.id } }),
          onDelete: () => void ctx.onDeleteShelf(s.shelf.id, s.shelf.name),
        })),
      }
    case 'libraries':
      return {
        id,
        key,
        label,
        icon: LuLibrary,
        overviewActive: route.name === 'comic-libraries',
        onOpenOverview: () => go('/comic/libraries'),
        items: comicSidebarStore.libraries.map((lib) => ({
          id: lib.id,
          label: lib.name,
          count: resolveLibrarySeriesCount(lib),
          icon: LuLibrary,
          active: ctx.activeComicLibraryId === lib.id,
          scanning: lib.scan_state.state === 'scanning',
          onClick: () => go({ path: '/comic/series', query: { library_id: lib.id } }),
        })),
      }
    case 'bookmarks':
      return {
        id,
        key,
        label,
        icon: LuBookmark,
        overviewActive: route.name === 'comic-bookmarks',
        onOpenOverview: () => go('/comic/bookmarks'),
        items: comicSidebarStore.bookmarks.map((bm) => ({
          id: bm.bookmark.id,
          label: `${bm.series_title} #${bm.chapter_number}`,
          count: `P${bm.bookmark.page_number}`,
          icon: LuBookmark,
          active: false,
          onClick: () =>
            go(
              `/comic/read/${encodeURIComponent(bm.bookmark.chapter_id)}?page=${bm.bookmark.page_number}`,
            ),
          onDelete: () =>
            void ctx.onDeleteBookmark(
              bm.bookmark.id,
              `${bm.series_title} #${bm.chapter_number} (P${bm.bookmark.page_number})`,
            ),
        })),
      }
    default:
      return null
  }
}
