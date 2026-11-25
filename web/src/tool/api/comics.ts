import type {
  BatchComicSeriesSyncInput,
  BatchComicSeriesSyncReportDto,
  ComicImageReencodeFormat,
  ComicOptimizeReportDto,
  ComicPagesSnapshotDto,
  MediaFileDetail,
  MergeComicsReportDto,
  SaveComicPageInput,
} from '../types'
import { toolRequest } from './client'

export const comicsApi = {
  getComicPages(rootId: string, relPath: string): Promise<ComicPagesSnapshotDto> {
    const q = new URLSearchParams({ root_id: rootId, path: relPath })
    return toolRequest<ComicPagesSnapshotDto>(`/comics/pages?${q.toString()}`)
  },

  comicPageImageUrl(
    rootId: string,
    relPath: string,
    entryName: string,
    maxWidth = 320,
    version = 0,
  ): string {
    const q = new URLSearchParams({
      root_id: rootId,
      path: relPath,
      entry: entryName,
      max_width: String(maxWidth),
      v: String(version),
    })
    return `/api/v1/tool/comics/page-image?${q.toString()}`
  },

  saveComicPages(
    rootId: string,
    relPath: string,
    pages: SaveComicPageInput[],
    reencode: ComicImageReencodeFormat = 'keep',
    jpegQuality = 84,
    maxWidth?: number | null,
  ): Promise<{ report: ComicOptimizeReportDto; detail: MediaFileDetail }> {
    return toolRequest<{ report: ComicOptimizeReportDto; detail: MediaFileDetail }>('/comics/pages', {
      method: 'PUT',
      body: JSON.stringify({
        root_id: rootId,
        path: relPath,
        pages,
        reencode,
        jpeg_quality: jpegQuality,
        max_width: maxWidth || undefined,
      }),
    })
  },

  optimizeComic(
    rootId: string,
    relPath: string,
    reencode: ComicImageReencodeFormat,
    jpegQuality = 84,
    maxWidth?: number | null,
  ): Promise<{ report: ComicOptimizeReportDto; detail: MediaFileDetail }> {
    return toolRequest<{ report: ComicOptimizeReportDto; detail: MediaFileDetail }>(
      '/comics/optimize',
      {
        method: 'POST',
        body: JSON.stringify({
          root_id: rootId,
          path: relPath,
          reencode,
          jpeg_quality: jpegQuality,
          max_width: maxWidth || undefined,
        }),
      },
    )
  },

  mergeComics(
    rootId: string,
    sourcePaths: string[],
    outputRelPath: string,
    series?: string,
    title?: string,
    volume?: number | null,
  ): Promise<{ report: MergeComicsReportDto; detail: MediaFileDetail }> {
    return toolRequest<{ report: MergeComicsReportDto; detail: MediaFileDetail }>('/comics/merge', {
      method: 'POST',
      body: JSON.stringify({
        root_id: rootId,
        source_paths: sourcePaths,
        output_rel_path: outputRelPath,
        series: series || undefined,
        title: title || undefined,
        volume: volume ?? undefined,
      }),
    })
  },

  batchSyncComics(
    rootId: string,
    paths: string[],
    input: BatchComicSeriesSyncInput,
  ): Promise<BatchComicSeriesSyncReportDto> {
    return toolRequest<BatchComicSeriesSyncReportDto>('/comics/batch-sync', {
      method: 'POST',
      body: JSON.stringify({
        root_id: rootId,
        paths,
        ...input,
      }),
    })
  },
}
