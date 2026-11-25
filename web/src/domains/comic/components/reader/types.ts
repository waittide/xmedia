export interface ComicReaderChapterItem {
  key: string
  label: string
  metaLabel?: string
  disabled?: boolean
}

export interface ComicReaderBookmarkItem {
  id: string
  page: number
  title: string
  note?: string | null
}

export type ReaderSheetId = 'chapters' | 'settings'
export type ChaptersSheetTab = 'chapters' | 'bookmarks'
