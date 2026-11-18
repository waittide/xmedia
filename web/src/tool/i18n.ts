import { computed } from 'vue'
import { useThemeStore } from '@/stores/theme'
import type { MediaKind } from '@/types/api'

interface DomainSpecificLabels {
  missingCover: string
  missingSummary: string
  missingTagBlock: string
  emptyRootsTitle: string
  emptyRootsDesc: string
  searchPlaceholder: string
}

const MESSAGES = {
  'zh-CN': {
    brand: 'xmedia-tool',
    brandSub: '原文件刮削与整理工作台',
    domains: {
      book: '图书',
      comic: '漫画',
      music: '音乐',
      video: '影视',
    },
    domainLabels: {
      book: {
        missingCover: '缺少图书封面',
        missingSummary: '缺少内容简介',
        missingTagBlock: '缺少出版社 / 标签',
        emptyRootsTitle: '尚未配置图书根目录',
        emptyRootsDesc: '请点击下方按钮为「图书」领域添加专属本地目录白名单，各领域目录彼此完全独立。',
        searchPlaceholder: '搜索当前目录图书文件名、书名或作者…',
      },
      comic: {
        missingCover: '缺少漫画封面',
        missingSummary: '缺少漫画简介',
        missingTagBlock: '缺少 ComicInfo.xml',
        emptyRootsTitle: '尚未配置漫画根目录',
        emptyRootsDesc: '请点击下方按钮为「漫画」领域添加专属本地目录白名单，各领域目录彼此完全独立。',
        searchPlaceholder: '搜索当前目录漫画文件名、系列名或作者…',
      },
      music: {
        missingCover: '缺少内嵌封面',
        missingSummary: '缺少歌词 (.lrc / 内嵌)',
        missingTagBlock: '缺少专辑标签',
        emptyRootsTitle: '尚未配置音乐根目录',
        emptyRootsDesc: '请点击下方按钮为「音乐」领域添加专属本地目录白名单，各领域目录彼此完全独立。',
        searchPlaceholder: '搜索当前目录音频文件名、曲名、歌手或专辑…',
      },
      video: {
        missingCover: '缺少影视海报',
        missingSummary: '缺少剧情简介',
        missingTagBlock: '缺少 NFO 文件',
        emptyRootsTitle: '尚未配置影视根目录',
        emptyRootsDesc: '请点击下方按钮为「影视」领域添加专属本地目录白名单，各领域目录彼此完全独立。',
        searchPlaceholder: '搜索当前目录视频文件名、片名或剧集名…',
      },
    } satisfies Record<MediaKind, DomainSpecificLabels>,
    filters: {
      title: '完整度筛选',
      all: '全部文件',
      complete: '元信息齐全',
      incomplete: '待补齐项',
    },
    roots: {
      title: '当前领域根目录',
      add: '添加当前领域目录',
      foldersTitle: '当前子目录',
      parentDir: '.. 返回上级目录',
    },
    cards: {
      totalFiles: '当前目录文件',
      completeFiles: '元信息已齐全',
    },
    actions: {
      selectAll: '全选',
      clearSelect: '取消选择',
      onlineScrape: '在线刮削',
      batchRename: '批量重命名',
      refresh: '刷新',
      settings: '设置',
      lock: '锁定',
      save: '原子写入原文件',
      saving: '正在写入…',
      saved: '已写入原文件',
      uploadCover: '上传替换封面',
      uploadPoster: '上传海报',
      uploadFanart: '上传背景图',
      close: '关闭',
    },
    unlock: {
      title: '解锁 xmedia-tool 工作台',
      subtitle: '当前实例启用了访问口令保护，请输入口令继续管理本地媒体原文件。',
      placeholder: '输入访问口令…',
      submit: '解锁进入',
    },
  },
  'en-US': {
    brand: 'xmedia-tool',
    brandSub: 'Source File Scraper & Tag Workbench',
    domains: {
      book: 'Books',
      comic: 'Comics',
      music: 'Music',
      video: 'Video',
    },
    domainLabels: {
      book: {
        missingCover: 'Missing Cover',
        missingSummary: 'Missing Description',
        missingTagBlock: 'Missing Publisher / Tags',
        emptyRootsTitle: 'No Book Root Configured',
        emptyRootsDesc: 'Add a dedicated directory for Books. Each media domain manages its own isolated roots.',
        searchPlaceholder: 'Filter books by filename, title or author…',
      },
      comic: {
        missingCover: 'Missing Cover',
        missingSummary: 'Missing Summary',
        missingTagBlock: 'Missing ComicInfo.xml',
        emptyRootsTitle: 'No Comic Root Configured',
        emptyRootsDesc: 'Add a dedicated directory for Comics. Each media domain manages its own isolated roots.',
        searchPlaceholder: 'Filter comics by filename, series or writer…',
      },
      music: {
        missingCover: 'Missing Cover Art',
        missingSummary: 'Missing Lyrics',
        missingTagBlock: 'Missing Album Tag',
        emptyRootsTitle: 'No Music Root Configured',
        emptyRootsDesc: 'Add a dedicated directory for Music. Each media domain manages its own isolated roots.',
        searchPlaceholder: 'Filter tracks by filename, title, artist or album…',
      },
      video: {
        missingCover: 'Missing Poster',
        missingSummary: 'Missing Plot',
        missingTagBlock: 'Missing .nfo File',
        emptyRootsTitle: 'No Video Root Configured',
        emptyRootsDesc: 'Add a dedicated directory for Video. Each media domain manages its own isolated roots.',
        searchPlaceholder: 'Filter videos by filename, movie or show title…',
      },
    } satisfies Record<MediaKind, DomainSpecificLabels>,
    filters: {
      title: 'Completeness',
      all: 'All Files',
      complete: 'Complete',
      incomplete: 'Incomplete',
    },
    roots: {
      title: 'Domain Roots',
      add: 'Add Domain Root',
      foldersTitle: 'Subfolders',
      parentDir: '.. Parent Directory',
    },
    cards: {
      totalFiles: 'Files in Folder',
      completeFiles: 'Complete Metadata',
    },
    actions: {
      selectAll: 'Select All',
      clearSelect: 'Clear',
      onlineScrape: 'Online Scrape',
      batchRename: 'Batch Rename',
      refresh: 'Refresh',
      settings: 'Settings',
      lock: 'Lock',
      save: 'Save to Source File',
      saving: 'Writing…',
      saved: 'Saved',
      uploadCover: 'Replace Cover',
      uploadPoster: 'Upload Poster',
      uploadFanart: 'Upload Fanart',
      close: 'Close',
    },
    unlock: {
      title: 'Unlock xmedia-tool',
      subtitle: 'This instance is protected by an access password.',
      placeholder: 'Enter access password…',
      submit: 'Unlock Workbench',
    },
  },
} as const

export function useToolText() {
  const themeStore = useThemeStore()
  return computed(() => MESSAGES[themeStore.locale] ?? MESSAGES['zh-CN'])
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`
}

export function formatDuration(secs: number): string {
  if (!secs || secs <= 0) return '—'
  const h = Math.floor(secs / 3600)
  const m = Math.floor((secs % 3600) / 60)
  const s = secs % 60
  if (h > 0) {
    return `${h}:${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`
  }
  return `${m}:${String(s).padStart(2, '0')}`
}
