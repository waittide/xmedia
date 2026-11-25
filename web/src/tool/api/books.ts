import type {
  BookContentDocumentDto,
  BookTextTransformOptions,
  ConvertTxtToEpubResponseDto,
  EditableBookChapterDto,
  InsertedEpubImageDto,
  MediaFileDetail,
  NormalizeBookToEpubOptionsDto,
  NormalizeBooksBatchReportDto,
  PolishBookResponseDto,
  SaveBookChapterInput,
  TransformBookContentResultDto,
} from '../types'
import { toolRequest } from './client'

export function buildEpubAssetUrl(rootId: string, relPath: string, assetPath: string): string {
  const params = new URLSearchParams({
    root_id: rootId,
    path: relPath,
    asset: assetPath,
  })
  return `/api/v1/tool/books/epub-asset?${params.toString()}`
}

export const booksApi = {
  getBookContent(rootId: string, relPath: string): Promise<BookContentDocumentDto> {
    const q = new URLSearchParams({ root_id: rootId, path: relPath })
    return toolRequest<BookContentDocumentDto>(`/books/content?${q.toString()}`)
  },

  resegmentTxtBook(
    rootId: string,
    relPath: string,
    chapterPattern?: string,
    volumePattern?: string,
  ): Promise<EditableBookChapterDto[]> {
    return toolRequest<EditableBookChapterDto[]>('/books/resegment-txt', {
      method: 'POST',
      body: JSON.stringify({
        root_id: rootId,
        path: relPath,
        chapter_pattern: chapterPattern || undefined,
        volume_pattern: volumePattern || undefined,
      }),
    })
  },

  saveBookContent(
    rootId: string,
    relPath: string,
    chapters: SaveBookChapterInput[],
    stylesheets?: string[],
    forceHorizontalSpine = false,
  ): Promise<MediaFileDetail> {
    return toolRequest<MediaFileDetail>('/books/content', {
      method: 'PUT',
      body: JSON.stringify({
        root_id: rootId,
        path: relPath,
        chapters,
        stylesheets,
        force_horizontal_spine: forceHorizontalSpine,
      }),
    })
  },

  transformBookContent(
    chapters: EditableBookChapterDto[],
    stylesheets: string[],
    options: BookTextTransformOptions,
  ): Promise<TransformBookContentResultDto> {
    return toolRequest<TransformBookContentResultDto>('/books/transform', {
      method: 'POST',
      body: JSON.stringify({
        chapters,
        stylesheets,
        options,
      }),
    })
  },

  polishBook(rootId: string, relPath: string): Promise<PolishBookResponseDto> {
    return toolRequest<PolishBookResponseDto>('/books/polish', {
      method: 'POST',
      body: JSON.stringify({
        root_id: rootId,
        path: relPath,
      }),
    })
  },

  normalizeBooksToEpub(
    rootId: string,
    paths: string[],
    options: NormalizeBookToEpubOptionsDto,
  ): Promise<NormalizeBooksBatchReportDto> {
    return toolRequest<NormalizeBooksBatchReportDto>('/books/normalize-epub', {
      method: 'POST',
      body: JSON.stringify({
        root_id: rootId,
        paths,
        options,
      }),
    })
  },

  uploadBookEpubImage(
    rootId: string,
    relPath: string,
    file: File,
    chapterSourceHref?: string | null,
  ): Promise<InsertedEpubImageDto> {
    const fd = new FormData()
    fd.append('root_id', rootId)
    fd.append('path', relPath)
    if (chapterSourceHref) {
      fd.append('chapter_source_href', chapterSourceHref)
    }
    fd.append('file', file)
    return toolRequest<InsertedEpubImageDto>('/books/epub-image', {
      method: 'POST',
      body: fd,
    })
  },

  convertTxtToEpub(
    rootId: string,
    relPath: string,
    chapters: SaveBookChapterInput[] | undefined,
    deleteSourceTxt: boolean,
    stylesheets?: string[],
  ): Promise<ConvertTxtToEpubResponseDto> {
    return toolRequest<ConvertTxtToEpubResponseDto>('/books/convert-epub', {
      method: 'POST',
      body: JSON.stringify({
        root_id: rootId,
        path: relPath,
        chapters,
        stylesheets,
        delete_source_txt: deleteSourceTxt,
      }),
    })
  },
}
