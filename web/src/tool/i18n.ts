import { computed } from 'vue'
import { useThemeStore } from '@/stores/theme'

const MESSAGES = {
  'zh-CN': {
    brand: 'xmedia-tool',
    brandSub: '原文件刮削与整理工作台',
    allDomains: '全部领域',
    domains: {
      book: '图书',
      comic: '漫画',
      music: '音乐',
      video: '影视',
    },
    filters: {
      title: '完整度筛选',
      all: '全部文件',
      incomplete: '待补齐项',
      missingCover: '缺少封面 / 海报',
      missingSummary: '缺少简介 / 歌词',
      missingTagBlock: '缺少标签 / NFO',
    },
    roots: {
      title: '白名单根目录',
      add: '添加根目录',
      empty: '尚未配置媒体根目录',
      emptyDesc: '在设置中添加本地目录白名单（或通过启动参数 --root 指定）后即可直接浏览与整理原文件。',
      foldersTitle: '当前子目录',
    },
    cards: {
      totalFiles: '当前目录文件',
      completeFiles: '元信息已齐全',
      missingCover: '缺少封面 / 海报',
      missingLyricsOrNfo: '缺少歌词 / 简介',
    },
    actions: {
      searchPlaceholder: '过滤当前目录文件名、标题、作者或歌手…',
      selectAll: '全选',
      clearSelect: '取消选择',
      onlineScrape: '在线刮削',
      batchRename: '批量重命名',
      refresh: '刷新',
      settings: '工作台设置',
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
    allDomains: 'All',
    domains: {
      book: 'Books',
      comic: 'Comics',
      music: 'Music',
      video: 'Video',
    },
    filters: {
      title: 'Completeness',
      all: 'All Files',
      incomplete: 'Incomplete',
      missingCover: 'Missing Cover',
      missingSummary: 'Missing Lyrics / Plot',
      missingTagBlock: 'Missing Tags / NFO',
    },
    roots: {
      title: 'Managed Roots',
      add: 'Add Root',
      empty: 'No Root Directories Configured',
      emptyDesc: 'Add a whitelisted directory in Settings (or pass --root on startup) to inspect and scrape source files.',
      foldersTitle: 'Subfolders',
    },
    cards: {
      totalFiles: 'Files in Folder',
      completeFiles: 'Complete Metadata',
      missingCover: 'Missing Artwork',
      missingLyricsOrNfo: 'Missing Lyrics / Plot',
    },
    actions: {
      searchPlaceholder: 'Filter files by name, title, artist or author…',
      selectAll: 'Select All',
      clearSelect: 'Clear',
      onlineScrape: 'Online Scrape',
      batchRename: 'Batch Rename',
      refresh: 'Refresh',
      settings: 'Settings',
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
