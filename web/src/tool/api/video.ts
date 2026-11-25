import type {
  BatchRemapEpisodesInput,
  BatchRemapEpisodesReportDto,
  ExtractSubtitleReportDto,
  MediaFileDetail,
  ProcessSubtitleInput,
  ProcessSubtitleReportDto,
  VideoSubtitleInfoDto,
} from '../types'
import { toolRequest } from './client'

export const videoApi = {
  listVideoSubtitles(rootId: string, relPath: string): Promise<VideoSubtitleInfoDto[]> {
    const q = new URLSearchParams({ root_id: rootId, path: relPath })
    return toolRequest<VideoSubtitleInfoDto[]>(`/video/subtitles?${q.toString()}`)
  },

  processVideoSubtitle(
    rootId: string,
    relPath: string,
    input: ProcessSubtitleInput,
  ): Promise<{ report: ProcessSubtitleReportDto; detail: MediaFileDetail }> {
    return toolRequest<{ report: ProcessSubtitleReportDto; detail: MediaFileDetail }>(
      '/video/subtitles/process',
      {
        method: 'POST',
        body: JSON.stringify({
          root_id: rootId,
          path: relPath,
          ...input,
        }),
      },
    )
  },

  extractVideoSubtitle(
    rootId: string,
    relPath: string,
    streamIndex: number,
    languageTag?: string,
    outputExt: 'srt' | 'ass' | 'vtt' = 'srt',
  ): Promise<{ report: ExtractSubtitleReportDto; detail: MediaFileDetail }> {
    return toolRequest<{ report: ExtractSubtitleReportDto; detail: MediaFileDetail }>(
      '/video/subtitles/extract',
      {
        method: 'POST',
        body: JSON.stringify({
          root_id: rootId,
          path: relPath,
          stream_index: streamIndex,
          language_tag: languageTag || undefined,
          output_ext: outputExt,
        }),
      },
    )
  },

  batchRemapVideoEpisodes(
    rootId: string,
    paths: string[],
    input: BatchRemapEpisodesInput,
  ): Promise<BatchRemapEpisodesReportDto> {
    return toolRequest<BatchRemapEpisodesReportDto>('/video/episodes/batch-remap', {
      method: 'POST',
      body: JSON.stringify({
        root_id: rootId,
        paths,
        ...input,
      }),
    })
  },
}
