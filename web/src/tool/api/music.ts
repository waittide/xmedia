import type {
  BatchMusicAlbumSyncInput,
  BatchMusicAlbumSyncReportDto,
  CueSplitReportDto,
  MediaFileDetail,
  MusicCoverOptimizeReportDto,
  MusicLyricsToolInput,
  MusicLyricsToolReportDto,
  ReplayGainReportDto,
} from '../types'
import { toolRequest } from './client'

export const musicApi = {
  runMusicLyricsTool(
    rootId: string,
    relPath: string,
    input: MusicLyricsToolInput,
  ): Promise<{ report: MusicLyricsToolReportDto; detail: MediaFileDetail }> {
    return toolRequest<{ report: MusicLyricsToolReportDto; detail: MediaFileDetail }>(
      '/music/lyrics-tool',
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

  optimizeMusicCover(
    rootId: string,
    relPath: string,
    maxDimension = 1000,
    jpegQuality = 86,
  ): Promise<{ report: MusicCoverOptimizeReportDto; detail: MediaFileDetail }> {
    return toolRequest<{ report: MusicCoverOptimizeReportDto; detail: MediaFileDetail }>(
      '/music/optimize-cover',
      {
        method: 'POST',
        body: JSON.stringify({
          root_id: rootId,
          path: relPath,
          max_dimension: maxDimension,
          jpeg_quality: jpegQuality,
        }),
      },
    )
  },

  applyMusicReplayGain(rootId: string, paths: string[]): Promise<ReplayGainReportDto> {
    return toolRequest<ReplayGainReportDto>('/music/replaygain', {
      method: 'POST',
      body: JSON.stringify({
        root_id: rootId,
        paths,
      }),
    })
  },

  splitMusicCue(rootId: string, relPath: string): Promise<CueSplitReportDto> {
    return toolRequest<CueSplitReportDto>('/music/split-cue', {
      method: 'POST',
      body: JSON.stringify({
        root_id: rootId,
        path: relPath,
      }),
    })
  },

  batchSyncMusicAlbum(
    rootId: string,
    paths: string[],
    input: BatchMusicAlbumSyncInput,
  ): Promise<BatchMusicAlbumSyncReportDto> {
    return toolRequest<BatchMusicAlbumSyncReportDto>('/music/batch-sync', {
      method: 'POST',
      body: JSON.stringify({
        root_id: rootId,
        paths,
        ...input,
      }),
    })
  },
}
