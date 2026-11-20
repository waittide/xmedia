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
    nav: {
      home: '首页',
      items: '条目',
      tasks: '任务',
    },
    filters: {
      title: '完整度筛选',
      all: '全部文件',
      complete: '元信息齐全',
      incomplete: '待补齐项',
    },
    roots: {
      title: '媒体根目录',
      add: '添加当前领域目录',
      foldersTitle: '子文件夹',
      parentDir: '.. 返回上级目录',
    },
    cards: {
      totalFiles: '当前目录文件',
      completeFiles: '元信息已齐全',
    },
    actions: {
      selectAll: '全选',
      clearSelect: '取消选择',
      scrape: '刮削',
      onlineScrape: '手动匹配刮削',
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
    boot: {
      connecting: '正在连接 xmedia-tool…',
      failedTitle: '无法连接到 xmedia-tool 服务',
      failedDesc: '请确认 xmedia-tool 后端进程正在运行，且网络/代理配置正常。',
      retry: '重新连接',
    },
    sort: {
      name: '按文件名',
      title: '按标题',
      incomplete: '缺项优先',
      size: '按大小',
      modified: '按修改时间',
      asc: '升序',
      desc: '降序',
    },
    batch: {
      selectedCount: '已选 {count} 项',
      scrapeAllLabel: '刮削全部 ({count})',
      scrapeAllConfirmTitle: '确认刮削当前列表全部文件？',
      scrapeAllConfirmMsg: '当前未勾选具体条目，将把筛选结果中的全部 {count} 个文件加入刮削队列。是否继续？',
      scrapeAllConfirmBtn: '开始全部刮削',
      dryRunScrape: '预检刮削',
      comicSeriesTools: '漫画系列工具',
      musicTools: '专辑/响度工具',
      videoTools: '整季剧集映射',
      triggerRescan: '通知主站扫库',
      rescanning: '通知中…',
      loadMore: '加载更多 ({remaining} 项未显示)',
      showAll: '显示全部',
      showingOf: '已显示 {shown} / {total} 项',
    },
    tasks: {
      title: '后台任务',
      subtitle: '查看正在运行的批量刮削与重命名任务，支持取消或重试未命中项。',
      clearCompleted: '清除已完成',
      emptyTitle: '暂无后台任务',
      emptyDesc: '在条目列表中勾选文件并点击「刮削」或「批量重命名」后，任务进度会显示在此处。',
      cancel: '取消',
      retryUnfinished: '重试未完成项',
      details: '明细',
      removeRecord: '移除任务记录',
      statusMatched: '已补齐',
      statusUnmatched: '未命中',
      statusRunning: '刮削中…',
      statusFailed: '失败',
      statusCancelled: '已取消',
      statusQueued: '排队中',
      filterAll: '全部',
      filterActive: '进行中',
      filterCompleted: '已完成',
      filterFailed: '未命中 / 异常',
      runningNow: '正在处理',
      elapsed: '耗时',
      concurrency: '并发 {n}',
      reviewAllResults: '查看并编辑结果 ({count})',
      reviewUnmatched: '手动刮削',
      editMetadata: '编辑元信息',
      editBookContent: '目录/正文',
      matchedMetaPrefix: '已匹配',
      noSubItemsInFilter: '当前状态筛选下暂无明细记录',
      badgeHasCover: '✓ 封面',
      badgeNoCover: '缺封面',
      badgeHasSummary: '✓ 简介',
      badgeNoSummary: '缺简介',
      badgeHasTags: '✓ 标签',
      badgeNoTags: '缺标签',
    },
    settings: {
      subtitle: '管理界面外观、各媒体类型目录白名单、刮削源优先级、元数据缓存与重命名模板',
      backToWorkbench: '返回整理台',
      tabScrapers: '刮削源与缓存',
      tabRoots: '目录白名单',
      tabTemplates: '重命名模板',
      savedBanner: '设置已持久化保存并立即生效',
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
    nav: {
      home: 'Home',
      items: 'Items',
      tasks: 'Tasks',
    },
    filters: {
      title: 'Completeness',
      all: 'All Files',
      complete: 'Complete',
      incomplete: 'Incomplete',
    },
    roots: {
      title: 'Media Roots',
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
      scrape: 'Scrape',
      onlineScrape: 'Manual Match',
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
    boot: {
      connecting: 'Connecting to xmedia-tool…',
      failedTitle: 'Unable to reach xmedia-tool',
      failedDesc: 'Please ensure the xmedia-tool server is running and reachable.',
      retry: 'Retry Connection',
    },
    sort: {
      name: 'File Name',
      title: 'Title',
      incomplete: 'Incomplete First',
      size: 'File Size',
      modified: 'Modified Time',
      asc: 'Ascending',
      desc: 'Descending',
    },
    batch: {
      selectedCount: '{count} selected',
      scrapeAllLabel: 'Scrape All ({count})',
      scrapeAllConfirmTitle: 'Scrape all files in current view?',
      scrapeAllConfirmMsg: 'No individual items are checked. This will enqueue all {count} filtered files for scraping. Continue?',
      scrapeAllConfirmBtn: 'Scrape All',
      dryRunScrape: 'Dry-Run Scrape',
      comicSeriesTools: 'Comic Series Tools',
      musicTools: 'Album / Loudness',
      videoTools: 'Season Episode Map',
      triggerRescan: 'Notify Server Rescan',
      rescanning: 'Notifying…',
      loadMore: 'Load More ({remaining} remaining)',
      showAll: 'Show All',
      showingOf: 'Showing {shown} of {total}',
    },
    tasks: {
      title: 'Background Tasks',
      subtitle: 'Monitor active batch scraping and rename jobs, cancel or retry unmatched items.',
      clearCompleted: 'Clear Completed',
      emptyTitle: 'No Background Tasks',
      emptyDesc: 'Select files in Items and trigger Scrape or Batch Rename to see progress here.',
      cancel: 'Cancel',
      retryUnfinished: 'Retry Unfinished',
      details: 'Details',
      removeRecord: 'Remove task record',
      statusMatched: 'Matched',
      statusUnmatched: 'Unmatched',
      statusRunning: 'Scraping…',
      statusFailed: 'Failed',
      statusCancelled: 'Cancelled',
      statusQueued: 'Queued',
      filterAll: 'All',
      filterActive: 'Active',
      filterCompleted: 'Completed',
      filterFailed: 'Unmatched / Failed',
      runningNow: 'Processing',
      elapsed: 'Elapsed',
      concurrency: 'Concurrency {n}',
      reviewAllResults: 'Review & Edit Results ({count})',
      reviewUnmatched: 'Manual Scrape',
      editMetadata: 'Edit Metadata',
      editBookContent: 'TOC / Content',
      matchedMetaPrefix: 'Matched',
      noSubItemsInFilter: 'No items match this status filter',
      badgeHasCover: '✓ Cover',
      badgeNoCover: 'No Cover',
      badgeHasSummary: '✓ Summary',
      badgeNoSummary: 'No Summary',
      badgeHasTags: '✓ Tags',
      badgeNoTags: 'No Tags',
    },
    settings: {
      subtitle: 'Manage appearance, per-domain directory allowlists, scraper priority, metadata cache, and rename templates',
      backToWorkbench: 'Back to Workbench',
      tabScrapers: 'Scrapers & Cache',
      tabRoots: 'Directory Allowlist',
      tabTemplates: 'Rename Templates',
      savedBanner: 'Settings saved and applied immediately',
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
