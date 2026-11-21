<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import {
  LuAlertCircle,
  LuArrowLeft,
  LuBookOpen,
  LuCheck,
  LuCheckCircle2,
  LuChevronRight,
  LuCloud,
  LuDisc3,
  LuDownload,
  LuFolder,
  LuHome,
  LuInfo,
  LuLoader2,
  LuMusic,
  LuPause,
  LuPencil,
  LuPlay,
  LuPlus,
  LuRefreshCw,
  LuSearch,
  LuSettings2,
  LuShieldAlert,
  LuSkipBack,
  LuSkipForward,
  LuTrash2,
  LuWifi,
  LuX,
} from 'vue-icons-plus/lu'
import {
  appHostApi,
  type OpdsDownloadLinkDto,
  type OpdsEntryDto,
  type OpdsFeedDto,
  type RemoteProtocol,
  type RemoteSourceSummaryDto,
  type SubsonicAlbumDto,
  type SubsonicAlbumListType,
  type SubsonicSongDto,
  type TestRemoteSourceResponse,
} from '@/api/appHost'
import RemoteSourceFormDialog from '@/components/remote/RemoteSourceFormDialog.vue'
import AppSelect, { type SelectOption } from '@/components/ui/AppSelect.vue'
import MdSegmentedButton, { type SegmentedOption } from '@/components/ui/MdSegmentedButton.vue'
import MdSideSheet from '@/components/ui/MdSideSheet.vue'
import MediaCoverImage from '@/components/ui/MediaCoverImage.vue'
import { usePlayerStore } from '@/domains/music/player'
import { useAppClientStore } from '@/stores/appServers'
import { useDialogStore } from '@/stores/dialog'
import { useSnackbarStore } from '@/stores/snackbar'
import { useUploadStore } from '@/stores/uploads'
import { formatBytes, formatDuration, formatTime } from '@/utils/format'

type ActiveTab = 'opds' | 'subsonic' | 'manage'

const props = defineProps<{
  embeddedProtocol?: RemoteProtocol
}>()

const ALBUM_PAGE_SIZE = 30

const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const appClientStore = useAppClientStore()
const dialogStore = useDialogStore()
const snackbarStore = useSnackbarStore()
const uploadStore = useUploadStore()
const playerStore = usePlayerStore()

const isEmbedded = computed(() => Boolean(props.embeddedProtocol))
const activeTab = ref<ActiveTab>(props.embeddedProtocol ?? 'opds')
const initLoading = ref(false)

// Source form dialog state
const formDialogOpen = ref(false)
const editingSource = ref<RemoteSourceSummaryDto | null>(null)
const formDefaultProtocol = ref<RemoteProtocol>(props.embeddedProtocol ?? 'opds')

// Source management list actions
const testingSourceId = ref<string | null>(null)
const deletingSourceId = ref<string | null>(null)
const cardTestResults = ref<Record<string, TestRemoteSourceResponse>>({})

// ---------------- OPDS Browser State ----------------
const selectedOpdsId = ref('')
const opdsFeed = ref<OpdsFeedDto | null>(null)
const opdsLoading = ref(false)
const opdsError = ref('')
const opdsHistory = ref<string[]>([])
const opdsCurrentUrl = ref<string | undefined>(undefined)
const opdsSearchInput = ref('')
const opdsActiveQuery = ref('')
const opdsCoverFailed = ref<Record<string, boolean>>({})
const opdsSelectedFormatIdx = ref<Record<string, number>>({})
const opdsImportingKeys = ref<Record<string, boolean>>({})
const opdsImportedKeys = ref<Record<string, boolean>>({})
const opdsDetailEntry = ref<OpdsEntryDto | null>(null)
let opdsRequestSeq = 0

// ---------------- OpenSubsonic Browser State ----------------
const selectedSubsonicId = ref('')
const subsonicAlbumType = ref<SubsonicAlbumListType>('newest')
const subsonicAlbums = ref<SubsonicAlbumDto[]>([])
const subsonicHasMore = ref(false)
const subsonicLoading = ref(false)
const subsonicLoadingMore = ref(false)
const subsonicError = ref('')
const subsonicSearchInput = ref('')
const subsonicSearchActive = ref(false)
const subsonicSearchAlbums = ref<SubsonicAlbumDto[]>([])
const subsonicSearchSongs = ref<SubsonicSongDto[]>([])
const subsonicImportingIds = ref<Record<string, boolean>>({})
const subsonicImportedIds = ref<Record<string, boolean>>({})
let subsonicRequestSeq = 0

// Subsonic Album Side Sheet
const albumSheetOpen = ref(false)
const activeAlbum = ref<SubsonicAlbumDto | null>(null)
const albumSongs = ref<SubsonicSongDto[]>([])
const albumDetailLoading = ref(false)
const albumDetailError = ref('')
let albumDetailSeq = 0

// Lightweight MD3 Audio Preview Bar
const previewAudioRef = ref<HTMLAudioElement | null>(null)
const previewQueue = ref<SubsonicSongDto[]>([])
const previewIndex = ref(-1)
const previewPlaying = ref(false)
const previewCurrentTime = ref(0)
const previewDuration = ref(0)
const previewBuffering = ref(false)

const previewSong = computed<SubsonicSongDto | null>(() => {
  if (previewIndex.value < 0 || previewIndex.value >= previewQueue.value.length) return null
  return previewQueue.value[previewIndex.value] ?? null
})

// ---------------- Computed Options ----------------
const tabOptions = computed<SegmentedOption<ActiveTab>[]>(() => {
  if (props.embeddedProtocol === 'opds') {
    return [
      { value: 'manage', label: t('remote.tabManage'), icon: LuSettings2 },
      { value: 'opds', label: t('remote.browseBtn'), icon: LuBookOpen },
    ]
  }
  if (props.embeddedProtocol === 'subsonic') {
    return [
      { value: 'manage', label: t('remote.tabManage'), icon: LuSettings2 },
      { value: 'subsonic', label: t('remote.browseBtn'), icon: LuDisc3 },
    ]
  }
  return [
    { value: 'opds', label: t('remote.tabOpds'), icon: LuBookOpen },
    { value: 'subsonic', label: t('remote.tabSubsonic'), icon: LuDisc3 },
    { value: 'manage', label: t('remote.tabManage'), icon: LuSettings2 },
  ]
})

const managedSources = computed<RemoteSourceSummaryDto[]>(() => {
  if (props.embeddedProtocol === 'opds') return appClientStore.opdsSources
  if (props.embeddedProtocol === 'subsonic') return appClientStore.subsonicSources
  return appClientStore.remoteSources
})

const embeddedHeaderTitle = computed(() => {
  if (props.embeddedProtocol === 'opds') return t('settings.remoteOpdsTitle')
  if (props.embeddedProtocol === 'subsonic') return t('settings.remoteSubsonicTitle')
  return t('remote.title')
})

const opdsSourceOptions = computed<SelectOption[]>(() =>
  appClientStore.opdsSources.map((src) => ({
    value: src.id,
    label: src.name || t('remote.unnamed'),
  })),
)

const subsonicSourceOptions = computed<SelectOption[]>(() =>
  appClientStore.subsonicSources.map((src) => ({
    value: src.id,
    label: src.name || t('remote.unnamed'),
  })),
)

const subsonicSortOptions = computed<SegmentedOption<SubsonicAlbumListType>[]>(() => [
  { value: 'newest', label: t('remote.subsonicSortNewest') },
  { value: 'random', label: t('remote.subsonicSortRandom') },
  { value: 'alphabeticalByName', label: t('remote.subsonicSortName') },
])

// ---------------- OPDS Helpers ----------------
function getEntrySelectedDownload(entry: OpdsEntryDto): OpdsDownloadLinkDto | null {
  if (entry.downloads.length === 0) return null
  const idx = opdsSelectedFormatIdx.value[entry.id] ?? 0
  return entry.downloads[idx] ?? entry.downloads[0] ?? null
}

function opdsImportKey(sourceId: string, href: string): string {
  return `${sourceId}::${href}`
}

function buildOpdsFilename(entry: OpdsEntryDto, dl: OpdsDownloadLinkDto): string {
  const safeBase = (entry.title || 'book')
    .replace(/[\\/:*?"<>|]+/g, '_')
    .trim()
    .slice(0, 120)
  const ext = dl.format.replace(/^\.+/, '').toLowerCase()
  return ext ? `${safeBase}.${ext}` : safeBase
}

async function fetchOpdsFeed(
  opts: { url?: string; q?: string; pushHistory?: boolean } = {},
): Promise<void> {
  const sourceId = selectedOpdsId.value
  if (!sourceId) {
    opdsFeed.value = null
    return
  }

  if (opts.pushHistory && opdsFeed.value?.url) {
    opdsHistory.value = [...opdsHistory.value, opdsFeed.value.url]
  }

  const seq = ++opdsRequestSeq
  opdsLoading.value = true
  opdsError.value = ''
  try {
    const feed = await appHostApi.browseOpds(sourceId, {
      url: opts.url,
      q: opts.q,
    })
    if (seq !== opdsRequestSeq) return
    opdsFeed.value = feed
    opdsCurrentUrl.value = opts.url ?? feed.url
    opdsCoverFailed.value = {}
  } catch (err) {
    if (seq !== opdsRequestSeq) return
    opdsError.value = err instanceof Error ? err.message : String(err)
  } finally {
    if (seq === opdsRequestSeq) {
      opdsLoading.value = false
    }
  }
}

function handleOpdsSelectSource(nextId: string): void {
  selectedOpdsId.value = nextId
  opdsHistory.value = []
  opdsCurrentUrl.value = undefined
  opdsSearchInput.value = ''
  opdsActiveQuery.value = ''
  opdsFeed.value = null
  void fetchOpdsFeed()
}

function handleOpdsNavClick(href: string): void {
  opdsActiveQuery.value = ''
  void fetchOpdsFeed({ url: href, pushHistory: true })
}

function handleOpdsBack(): void {
  if (opdsHistory.value.length === 0) return
  const nextHistory = opdsHistory.value.slice(0, -1)
  const prevUrl = opdsHistory.value[opdsHistory.value.length - 1]
  opdsHistory.value = nextHistory
  opdsActiveQuery.value = ''
  void fetchOpdsFeed({ url: prevUrl })
}

function handleOpdsRoot(): void {
  opdsHistory.value = []
  opdsCurrentUrl.value = undefined
  opdsSearchInput.value = ''
  opdsActiveQuery.value = ''
  void fetchOpdsFeed()
}

function handleOpdsSearchSubmit(): void {
  const q = opdsSearchInput.value.trim()
  if (!q) {
    if (opdsActiveQuery.value) {
      handleOpdsRoot()
    }
    return
  }
  opdsActiveQuery.value = q
  void fetchOpdsFeed({ q, pushHistory: true })
}

function handleOpdsClearSearch(): void {
  opdsSearchInput.value = ''
  if (opdsActiveQuery.value) {
    handleOpdsRoot()
  }
}

function handleOpdsNextPage(): void {
  const nextUrl = opdsFeed.value?.next_url
  if (!nextUrl) return
  void fetchOpdsFeed({ url: nextUrl, pushHistory: true })
}

async function handleImportOpds(entry: OpdsEntryDto, dl: OpdsDownloadLinkDto): Promise<void> {
  const sourceId = selectedOpdsId.value
  if (!sourceId) return
  const key = opdsImportKey(sourceId, dl.href)
  if (opdsImportingKeys.value[key]) return

  opdsImportingKeys.value = { ...opdsImportingKeys.value, [key]: true }
  try {
    const res = await appHostApi.importOpdsEntry(sourceId, {
      url: dl.href,
      filename: buildOpdsFilename(entry, dl),
    })
    opdsImportedKeys.value = { ...opdsImportedKeys.value, [key]: true }
    uploadStore.notifyCatalogChanged(res.media_kind)
    const targetRoute =
      res.media_kind === 'comic'
        ? `/comic/series/${encodeURIComponent(res.imported_id)}`
        : `/book/items/${encodeURIComponent(res.imported_id)}`
    snackbarStore.show({
      message: t('remote.importSuccess', { title: res.title || entry.title }),
      actionLabel: t('remote.viewInLibrary'),
      onAction: () => {
        void router.push(targetRoute)
      },
    })
  } catch (err) {
    snackbarStore.show({
      message: t('remote.importFailed', {
        message: err instanceof Error ? err.message : String(err),
      }),
      tone: 'error',
    })
  } finally {
    const next = { ...opdsImportingKeys.value }
    delete next[key]
    opdsImportingKeys.value = next
  }
}

// ---------------- OpenSubsonic Helpers ----------------
async function fetchSubsonicAlbums(reset = true): Promise<void> {
  const sourceId = selectedSubsonicId.value
  if (!sourceId) {
    subsonicAlbums.value = []
    subsonicHasMore.value = false
    return
  }

  const offset = reset ? 0 : subsonicAlbums.value.length
  const seq = ++subsonicRequestSeq
  if (reset) {
    subsonicLoading.value = true
    subsonicError.value = ''
  } else {
    subsonicLoadingMore.value = true
  }

  try {
    const res = await appHostApi.listSubsonicAlbums(sourceId, {
      type: subsonicAlbumType.value,
      size: ALBUM_PAGE_SIZE,
      offset,
    })
    if (seq !== subsonicRequestSeq) return
    subsonicAlbums.value = reset ? res.albums : [...subsonicAlbums.value, ...res.albums]
    subsonicHasMore.value = res.albums.length >= ALBUM_PAGE_SIZE
  } catch (err) {
    if (seq !== subsonicRequestSeq) return
    subsonicError.value = err instanceof Error ? err.message : String(err)
  } finally {
    if (seq === subsonicRequestSeq) {
      subsonicLoading.value = false
      subsonicLoadingMore.value = false
    }
  }
}

function handleSubsonicSelectSource(nextId: string): void {
  selectedSubsonicId.value = nextId
  subsonicSearchInput.value = ''
  subsonicSearchActive.value = false
  subsonicSearchAlbums.value = []
  subsonicSearchSongs.value = []
  albumSheetOpen.value = false
  activeAlbum.value = null
  albumSongs.value = []
  void fetchSubsonicAlbums(true)
}

function handleSubsonicSortChange(nextType: SubsonicAlbumListType): void {
  subsonicAlbumType.value = nextType
  subsonicSearchActive.value = false
  subsonicSearchInput.value = ''
  void fetchSubsonicAlbums(true)
}

async function handleSubsonicSearchSubmit(): Promise<void> {
  const q = subsonicSearchInput.value.trim()
  if (!q) {
    handleSubsonicClearSearch()
    return
  }
  const sourceId = selectedSubsonicId.value
  if (!sourceId) return

  const seq = ++subsonicRequestSeq
  subsonicSearchActive.value = true
  subsonicLoading.value = true
  subsonicError.value = ''
  try {
    const res = await appHostApi.searchSubsonic(sourceId, q)
    if (seq !== subsonicRequestSeq) return
    subsonicSearchAlbums.value = res.albums
    subsonicSearchSongs.value = res.songs
  } catch (err) {
    if (seq !== subsonicRequestSeq) return
    subsonicError.value = err instanceof Error ? err.message : String(err)
  } finally {
    if (seq === subsonicRequestSeq) {
      subsonicLoading.value = false
    }
  }
}

function handleSubsonicClearSearch(): void {
  subsonicSearchInput.value = ''
  if (subsonicSearchActive.value) {
    subsonicSearchActive.value = false
    subsonicSearchAlbums.value = []
    subsonicSearchSongs.value = []
    void fetchSubsonicAlbums(true)
  }
}

async function openSubsonicAlbum(album: SubsonicAlbumDto): Promise<void> {
  const sourceId = selectedSubsonicId.value
  if (!sourceId) return

  activeAlbum.value = album
  albumSongs.value = []
  albumDetailError.value = ''
  albumSheetOpen.value = true
  albumDetailLoading.value = true
  const seq = ++albumDetailSeq

  try {
    const res = await appHostApi.getSubsonicAlbum(sourceId, album.id)
    if (seq !== albumDetailSeq) return
    activeAlbum.value = res.album
    albumSongs.value = res.songs
  } catch (err) {
    if (seq !== albumDetailSeq) return
    albumDetailError.value = err instanceof Error ? err.message : String(err)
  } finally {
    if (seq === albumDetailSeq) {
      albumDetailLoading.value = false
    }
  }
}

function subsonicImportKey(sourceId: string, songId: string): string {
  return `${sourceId}::${songId}`
}

function buildSongFilename(song: SubsonicSongDto): string {
  const parts = [song.artist, song.title].filter(Boolean).join(' - ') || 'track'
  const safeBase = parts.replace(/[\\/:*?"<>|]+/g, '_').trim().slice(0, 120)
  const ext = song.suffix?.replace(/^\.+/, '').toLowerCase()
  return ext ? `${safeBase}.${ext}` : safeBase
}

async function handleImportSong(song: SubsonicSongDto): Promise<void> {
  const sourceId = selectedSubsonicId.value
  if (!sourceId) return
  const key = subsonicImportKey(sourceId, song.id)
  if (subsonicImportingIds.value[key]) return

  subsonicImportingIds.value = { ...subsonicImportingIds.value, [key]: true }
  try {
    const res = await appHostApi.importSubsonicSong(sourceId, song.id, buildSongFilename(song))
    subsonicImportedIds.value = { ...subsonicImportedIds.value, [key]: true }
    uploadStore.notifyCatalogChanged('music')
    snackbarStore.show({
      message: t('remote.importSuccess', { title: res.title || song.title }),
      actionLabel: t('remote.viewInLibrary'),
      onAction: () => {
        void router.push('/music/songs')
      },
    })
  } catch (err) {
    snackbarStore.show({
      message: t('remote.importFailed', {
        message: err instanceof Error ? err.message : String(err),
      }),
      tone: 'error',
    })
  } finally {
    const next = { ...subsonicImportingIds.value }
    delete next[key]
    subsonicImportingIds.value = next
  }
}

// ---------------- Audio Preview Bar Controls ----------------
function isPreviewingSong(song: SubsonicSongDto): boolean {
  return previewSong.value?.id === song.id && previewPlaying.value
}

async function playPreviewTrack(song: SubsonicSongDto, queue: SubsonicSongDto[]): Promise<void> {
  if (previewSong.value?.id === song.id && previewAudioRef.value) {
    if (previewPlaying.value) {
      previewAudioRef.value.pause()
    } else {
      playerStore.pause()
      await previewAudioRef.value.play().catch(() => {
        snackbarStore.show({ message: t('remote.subsonicPreviewFailed'), tone: 'error' })
      })
    }
    return
  }

  playerStore.pause()
  const idx = queue.findIndex((item) => item.id === song.id)
  previewQueue.value = queue
  previewIndex.value = idx >= 0 ? idx : 0
  previewCurrentTime.value = 0
  previewDuration.value = song.duration ?? 0
  previewBuffering.value = true

  await nextTick()
  const audio = previewAudioRef.value
  if (!audio) return
  audio.src = song.stream_url
  audio.currentTime = 0
  try {
    await audio.play()
  } catch {
    previewPlaying.value = false
    previewBuffering.value = false
    snackbarStore.show({ message: t('remote.subsonicPreviewFailed'), tone: 'error' })
  }
}

function togglePreviewPlay(): void {
  const audio = previewAudioRef.value
  if (!audio || !previewSong.value) return
  if (previewPlaying.value) {
    audio.pause()
  } else {
    playerStore.pause()
    void audio.play().catch(() => {
      snackbarStore.show({ message: t('remote.subsonicPreviewFailed'), tone: 'error' })
    })
  }
}

function stepPreviewTrack(delta: -1 | 1): void {
  if (previewQueue.value.length === 0) return
  const nextIdx = previewIndex.value + delta
  if (nextIdx < 0 || nextIdx >= previewQueue.value.length) return
  const nextSong = previewQueue.value[nextIdx]
  if (nextSong) {
    void playPreviewTrack(nextSong, previewQueue.value)
  }
}

function seekPreview(ev: Event): void {
  const input = ev.target as HTMLInputElement
  const target = Number(input.value)
  if (!Number.isFinite(target) || !previewAudioRef.value) return
  previewAudioRef.value.currentTime = target
  previewCurrentTime.value = target
}

function closePreviewBar(): void {
  const audio = previewAudioRef.value
  if (audio) {
    audio.pause()
    audio.removeAttribute('src')
    audio.load()
  }
  previewQueue.value = []
  previewIndex.value = -1
  previewPlaying.value = false
  previewCurrentTime.value = 0
  previewDuration.value = 0
  previewBuffering.value = false
}

function onPreviewTimeUpdate(): void {
  const audio = previewAudioRef.value
  if (!audio) return
  previewCurrentTime.value = audio.currentTime || 0
  if (Number.isFinite(audio.duration) && audio.duration > 0) {
    previewDuration.value = audio.duration
  }
}

function onPreviewEnded(): void {
  if (previewIndex.value + 1 < previewQueue.value.length) {
    stepPreviewTrack(1)
  } else {
    previewPlaying.value = false
  }
}

// ---------------- Manage Tab Actions ----------------
function openAddSourceDialog(protocol: RemoteProtocol = 'opds'): void {
  editingSource.value = null
  formDefaultProtocol.value = protocol
  formDialogOpen.value = true
}

function openEditSourceDialog(source: RemoteSourceSummaryDto): void {
  editingSource.value = source
  formDefaultProtocol.value = source.protocol
  formDialogOpen.value = true
}

function handleSourceSaved(source: RemoteSourceSummaryDto): void {
  if (source.protocol === 'opds' && !selectedOpdsId.value) {
    selectedOpdsId.value = source.id
    if (activeTab.value === 'opds') {
      void fetchOpdsFeed()
    }
  } else if (source.protocol === 'subsonic' && !selectedSubsonicId.value) {
    selectedSubsonicId.value = source.id
    if (activeTab.value === 'subsonic') {
      void fetchSubsonicAlbums(true)
    }
  }
}

function handleBrowseSource(source: RemoteSourceSummaryDto): void {
  if (source.protocol === 'opds') {
    activeTab.value = 'opds'
    if (selectedOpdsId.value !== source.id || !opdsFeed.value) {
      handleOpdsSelectSource(source.id)
    }
  } else {
    activeTab.value = 'subsonic'
    if (selectedSubsonicId.value !== source.id || subsonicAlbums.value.length === 0) {
      handleSubsonicSelectSource(source.id)
    }
  }
}

async function handleTestSavedSource(source: RemoteSourceSummaryDto): Promise<void> {
  if (testingSourceId.value) return
  testingSourceId.value = source.id
  const startedAt = performance.now()
  try {
    let result: TestRemoteSourceResponse
    if (source.has_password) {
      // Use saved backend credentials via lightweight protocol probe
      if (source.protocol === 'opds') {
        const feed = await appHostApi.browseOpds(source.id)
        result = {
          ok: true,
          latency_ms: Math.max(1, Math.round(performance.now() - startedAt)),
          protocol: feed.opds_version || 'OPDS',
          detail: feed.title || feed.opds_version,
        }
      } else {
        const res = await appHostApi.listSubsonicAlbums(source.id, {
          type: 'newest',
          size: 1,
          offset: 0,
        })
        result = {
          ok: true,
          latency_ms: Math.max(1, Math.round(performance.now() - startedAt)),
          protocol: 'OpenSubsonic',
          detail: res.albums.length > 0 ? res.albums[0]!.name : 'OpenSubsonic',
        }
      }
    } else {
      result = await appClientStore.testSource({
        protocol: source.protocol,
        url: source.url,
        username: source.username || undefined,
        allow_insecure_tls: source.allow_insecure_tls,
      })
    }
    cardTestResults.value = { ...cardTestResults.value, [source.id]: result }
  } catch (err) {
    cardTestResults.value = {
      ...cardTestResults.value,
      [source.id]: {
        ok: false,
        latency_ms: Math.max(1, Math.round(performance.now() - startedAt)),
        protocol: source.protocol,
        error: err instanceof Error ? err.message : String(err),
      },
    }
  } finally {
    testingSourceId.value = null
  }
}

async function handleDeleteSource(source: RemoteSourceSummaryDto): Promise<void> {
  const ok = await dialogStore.confirm({
    title: t('remote.deleteTitle'),
    message: t('remote.deleteConfirm', { name: source.name || t('remote.unnamed') }),
    confirmText: t('common.delete'),
    cancelText: t('common.cancel'),
    danger: true,
  })
  if (!ok) return

  deletingSourceId.value = source.id
  try {
    await appClientStore.deleteSource(source.id)
    snackbarStore.show(t('remote.deletedMsg'))

    if (selectedOpdsId.value === source.id) {
      selectedOpdsId.value = appClientStore.opdsSources[0]?.id ?? ''
      opdsFeed.value = null
      opdsHistory.value = []
      if (selectedOpdsId.value && activeTab.value === 'opds') {
        void fetchOpdsFeed()
      }
    }
    if (selectedSubsonicId.value === source.id) {
      selectedSubsonicId.value = appClientStore.subsonicSources[0]?.id ?? ''
      subsonicAlbums.value = []
      albumSheetOpen.value = false
      if (selectedSubsonicId.value && activeTab.value === 'subsonic') {
        void fetchSubsonicAlbums(true)
      }
    }
  } catch (err) {
    snackbarStore.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    deletingSourceId.value = null
  }
}

// ---------------- Lifecycle & Sync ----------------
watch(activeTab, (tab) => {
  if (tab === 'opds') {
    if (!selectedOpdsId.value && appClientStore.opdsSources.length > 0) {
      selectedOpdsId.value = appClientStore.opdsSources[0]!.id
    }
    if (selectedOpdsId.value && !opdsFeed.value && !opdsLoading.value) {
      void fetchOpdsFeed()
    }
  } else if (tab === 'subsonic') {
    if (!selectedSubsonicId.value && appClientStore.subsonicSources.length > 0) {
      selectedSubsonicId.value = appClientStore.subsonicSources[0]!.id
    }
    if (
      selectedSubsonicId.value &&
      subsonicAlbums.value.length === 0 &&
      !subsonicSearchActive.value &&
      !subsonicLoading.value
    ) {
      void fetchSubsonicAlbums(true)
    }
  }
})

watch(
  () => props.embeddedProtocol,
  (proto) => {
    if (!proto) return
    formDefaultProtocol.value = proto
    if (proto === 'opds') {
      activeTab.value = appClientStore.opdsSources.length > 0 ? 'opds' : 'manage'
      if (appClientStore.opdsSources.length > 0 && !selectedOpdsId.value) {
        selectedOpdsId.value = appClientStore.opdsSources[0]!.id
      }
    } else {
      activeTab.value = appClientStore.subsonicSources.length > 0 ? 'subsonic' : 'manage'
      if (appClientStore.subsonicSources.length > 0 && !selectedSubsonicId.value) {
        selectedSubsonicId.value = appClientStore.subsonicSources[0]!.id
      }
    }
  },
)

onMounted(async () => {
  initLoading.value = true
  try {
    await appClientStore.refreshSources()
  } catch (err) {
    snackbarStore.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    initLoading.value = false
  }

  if (props.embeddedProtocol === 'opds') {
    activeTab.value = appClientStore.opdsSources.length > 0 ? 'opds' : 'manage'
  } else if (props.embeddedProtocol === 'subsonic') {
    activeTab.value = appClientStore.subsonicSources.length > 0 ? 'subsonic' : 'manage'
  } else {
    const queryTab = route.query.tab
    if (queryTab === 'opds' || queryTab === 'subsonic' || queryTab === 'manage') {
      activeTab.value = queryTab
    } else if (appClientStore.opdsSources.length > 0) {
      activeTab.value = 'opds'
    } else if (appClientStore.subsonicSources.length > 0) {
      activeTab.value = 'subsonic'
    } else {
      activeTab.value = 'manage'
    }
  }

  if (appClientStore.opdsSources.length > 0) {
    selectedOpdsId.value = appClientStore.opdsSources[0]!.id
  }
  if (appClientStore.subsonicSources.length > 0) {
    selectedSubsonicId.value = appClientStore.subsonicSources[0]!.id
  }

  if (activeTab.value === 'opds' && selectedOpdsId.value) {
    void fetchOpdsFeed()
  } else if (activeTab.value === 'subsonic' && selectedSubsonicId.value) {
    void fetchSubsonicAlbums(true)
  }
})

onBeforeUnmount(() => {
  closePreviewBar()
})
</script>

<template>
  <div :class="isEmbedded ? 'space-y-4' : 'mx-auto max-w-6xl space-y-5 px-4 py-6 sm:px-6'">
    <!-- Header + MD3 Segmented Tabs -->
    <div class="flex flex-wrap items-center justify-between gap-3">
      <h2
        v-if="isEmbedded"
        class="md-title-small flex items-center gap-2 text-[var(--md-sys-color-on-surface)]"
      >
        <LuCloud class="h-4 w-4 text-[var(--md-sys-color-primary)]" />
        <span>{{ embeddedHeaderTitle }}</span>
      </h2>
      <h1
        v-else
        class="flex items-center gap-2 text-2xl font-bold tracking-tight text-[var(--md-sys-color-on-surface)]"
      >
        <LuCloud class="h-6 w-6 text-[var(--md-sys-color-primary)]" />
        <span>{{ t('remote.title') }}</span>
      </h1>

      <MdSegmentedButton
        v-model="activeTab"
        :options="tabOptions"
        size="sm"
        :show-check="false"
      />
    </div>

    <div
      v-if="initLoading"
      class="flex items-center justify-center py-20 text-xs text-[var(--md-sys-color-on-surface-variant)]"
    >
      <LuLoader2 class="mr-2 h-4 w-4 animate-spin" />
      <span>{{ t('common.loading') }}</span>
    </div>

    <!-- ================= TAB 1: OPDS Book Catalog ================= -->
    <template v-else-if="activeTab === 'opds'">
      <div
        v-if="appClientStore.opdsSources.length === 0"
        class="rounded-[24px] bg-[var(--md-sys-color-surface-container-low)] px-6 py-16 text-center"
      >
        <div
          class="mx-auto mb-3 flex h-12 w-12 items-center justify-center rounded-2xl bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)]"
        >
          <LuBookOpen class="h-6 w-6" />
        </div>
        <p class="text-sm font-semibold text-[var(--md-sys-color-on-surface)]">
          {{ t('remote.opdsEmptySourcesTitle') }}
        </p>
        <p class="mx-auto mt-1 max-w-md text-xs text-[var(--md-sys-color-on-surface-variant)]">
          {{ t('remote.opdsEmptySourcesHint') }}
        </p>
        <button
          type="button"
          class="md-btn md-btn-sm md-btn-filled md-state mt-5"
          @click="openAddSourceDialog('opds')"
        >
          <LuPlus class="h-4 w-4" />
          <span>{{ t('remote.opdsAddBtn') }}</span>
        </button>
      </div>

      <template v-else>
        <!-- OPDS Toolbar -->
        <div class="flex flex-wrap items-center gap-2.5 lg:flex-nowrap">
          <div class="w-44 shrink-0 sm:w-52">
            <AppSelect
              :model-value="selectedOpdsId"
              :options="opdsSourceOptions"
              :placeholder="t('remote.opdsSelectSource')"
              size="sm"
              @update:model-value="handleOpdsSelectSource"
            />
          </div>

          <div class="flex shrink-0 items-center gap-1">
            <button
              type="button"
              class="md-icon-btn md-icon-btn-sm md-state"
              :disabled="opdsHistory.length === 0 || opdsLoading"
              :title="t('remote.opdsBack')"
              :aria-label="t('remote.opdsBack')"
              @click="handleOpdsBack"
            >
              <LuArrowLeft class="h-4 w-4" />
            </button>
            <button
              type="button"
              class="md-icon-btn md-icon-btn-sm md-state"
              :disabled="(opdsHistory.length === 0 && !opdsActiveQuery) || opdsLoading"
              :title="t('remote.opdsRoot')"
              :aria-label="t('remote.opdsRoot')"
              @click="handleOpdsRoot"
            >
              <LuHome class="h-4 w-4" />
            </button>
            <button
              type="button"
              class="md-icon-btn md-icon-btn-sm md-state"
              :disabled="opdsLoading"
              :title="t('common.refresh')"
              :aria-label="t('common.refresh')"
              @click="fetchOpdsFeed({ url: opdsCurrentUrl, q: opdsActiveQuery || undefined })"
            >
              <LuRefreshCw class="h-4 w-4" :class="opdsLoading ? 'animate-spin' : ''" />
            </button>
          </div>

          <div v-if="opdsFeed" class="flex min-w-0 flex-1 items-center gap-2">
            <span class="truncate text-sm font-semibold text-[var(--md-sys-color-on-surface)]">
              {{ opdsFeed.title }}
            </span>
            <span
              v-if="opdsFeed.opds_version"
              class="shrink-0 rounded-full bg-[var(--md-sys-color-secondary-container)] px-2 py-0.5 font-mono text-[10px] font-medium text-[var(--md-sys-color-on-secondary-container)]"
            >
              {{ opdsFeed.opds_version }}
            </span>
          </div>

          <form
            v-if="opdsFeed?.search_url || opdsActiveQuery"
            class="relative ml-auto w-full shrink-0 sm:w-60 lg:w-64"
            @submit.prevent="handleOpdsSearchSubmit"
          >
            <LuSearch
              class="pointer-events-none absolute top-1/2 left-3 h-3.5 w-3.5 -translate-y-1/2 text-[var(--md-sys-color-on-surface-variant)]"
            />
            <input
              v-model="opdsSearchInput"
              type="text"
              :placeholder="t('remote.opdsSearchPlaceholder')"
              class="md-input h-9 w-full !pr-8 !pl-8 text-xs"
            />
            <button
              v-if="opdsSearchInput || opdsActiveQuery"
              type="button"
              class="md-icon-btn md-icon-btn-sm absolute top-1/2 right-1 -translate-y-1/2 !h-7 !w-7"
              :aria-label="t('common.cancel')"
              @click="handleOpdsClearSearch"
            >
              <LuX class="h-3.5 w-3.5" />
            </button>
          </form>
        </div>

        <!-- Error Banner -->
        <div
          v-if="opdsError"
          class="flex items-center justify-between gap-3 rounded-2xl bg-[var(--md-sys-color-error-container)] px-4 py-3 text-xs font-medium text-[var(--md-sys-color-on-error-container)]"
        >
          <div class="flex min-w-0 items-center gap-2">
            <LuAlertCircle class="h-4 w-4 shrink-0" />
            <span class="break-all">{{ opdsError }}</span>
          </div>
          <button
            type="button"
            class="md-btn md-btn-sm md-btn-text shrink-0 !text-[var(--md-sys-color-on-error-container)]"
            @click="fetchOpdsFeed({ url: opdsCurrentUrl, q: opdsActiveQuery || undefined })"
          >
            {{ t('remote.retryBtn') }}
          </button>
        </div>

        <!-- Loading -->
        <div
          v-else-if="opdsLoading && !opdsFeed"
          class="flex items-center justify-center py-20 text-xs text-[var(--md-sys-color-on-surface-variant)]"
        >
          <LuLoader2 class="mr-2 h-4 w-4 animate-spin" />
          <span>{{ t('common.loading') }}</span>
        </div>

        <!-- Feed Content -->
        <template v-else-if="opdsFeed">
          <!-- Navigation Chips -->
          <section v-if="opdsFeed.nav_links.length > 0" class="space-y-2.5">
            <h2
              class="md-label-medium text-[var(--md-sys-color-on-surface-variant)]"
            >
              {{ t('remote.opdsCategories') }}
            </h2>
            <div class="flex flex-wrap gap-2">
              <button
                v-for="link in opdsFeed.nav_links"
                :key="`${link.href}-${link.title}`"
                type="button"
                class="md-state inline-flex cursor-pointer items-center gap-2 rounded-full border border-[var(--md-sys-color-outline-variant)] bg-[var(--md-sys-color-surface-container-low)] px-3.5 py-2 text-xs font-medium text-[var(--md-sys-color-on-surface)] transition hover:border-[var(--md-sys-color-primary)]/50 hover:bg-[var(--md-sys-color-surface-container-high)]"
                @click="handleOpdsNavClick(link.href)"
              >
                <LuFolder class="h-3.5 w-3.5 shrink-0 text-[var(--md-sys-color-primary)]" />
                <span class="max-w-[16rem] truncate">{{ link.title }}</span>
                <span
                  v-if="link.count !== null && link.count !== undefined"
                  class="rounded-full bg-[var(--md-sys-color-surface-container-highest)] px-1.5 py-0.5 text-[10px] tabular-nums text-[var(--md-sys-color-on-surface-variant)]"
                >
                  {{ link.count }}
                </span>
                <LuChevronRight class="h-3.5 w-3.5 shrink-0 opacity-60" />
              </button>
            </div>
          </section>

          <!-- Entries 2:3 Portrait Cover Grid -->
          <section v-if="opdsFeed.entries.length > 0" class="space-y-3">
            <h2
              v-if="opdsFeed.nav_links.length > 0"
              class="md-label-medium text-[var(--md-sys-color-on-surface-variant)]"
            >
              {{ t('remote.opdsBooks') }}
            </h2>

            <div
              class="grid grid-cols-2 gap-4 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6"
            >
              <article
                v-for="entry in opdsFeed.entries"
                :key="entry.id"
                class="group flex flex-col"
              >
                <!-- 2:3 Portrait Cover -->
                <div
                  role="button"
                  tabindex="0"
                  :aria-label="entry.title"
                  class="book-shadow relative aspect-[2/3] w-full cursor-pointer overflow-hidden rounded-xl bg-[var(--md-sys-color-surface-container-low)] outline-none transition duration-200 group-hover:-translate-y-1 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--md-sys-color-primary)]"
                  @click="opdsDetailEntry = entry"
                  @keydown.enter.prevent="opdsDetailEntry = entry"
                  @keydown.space.prevent="opdsDetailEntry = entry"
                >
                  <img
                    v-if="entry.cover_url && !opdsCoverFailed[entry.id]"
                    :src="entry.cover_url"
                    :alt="entry.title"
                    loading="lazy"
                    decoding="async"
                    class="h-full w-full object-cover transition duration-300 group-hover:scale-[1.03]"
                    @error="opdsCoverFailed = { ...opdsCoverFailed, [entry.id]: true }"
                  />
                  <div
                    v-else
                    class="flex h-full w-full flex-col justify-end bg-gradient-to-br from-[var(--md-sys-color-surface-container-high)] via-[var(--md-sys-color-surface-container)] to-[var(--md-sys-color-primary-container)]/35 p-3.5 text-left"
                  >
                    <p
                      class="line-clamp-3 text-sm font-bold leading-snug text-[var(--md-sys-color-on-surface)]"
                    >
                      {{ entry.title }}
                    </p>
                    <p class="mt-1 truncate text-xs text-[var(--md-sys-color-on-surface-variant)]">
                      {{ entry.authors.join(', ') || t('remote.opdsUnknownAuthor') }}
                    </p>
                  </div>

                  <!-- Format badge -->
                  <span
                    v-if="getEntrySelectedDownload(entry)"
                    class="pointer-events-none absolute top-2 left-2 rounded-md bg-black/60 px-1.5 py-0.5 font-mono text-[10px] font-semibold uppercase text-white backdrop-blur-sm"
                  >
                    {{ getEntrySelectedDownload(entry)!.format || 'FILE' }}
                  </span>

                  <!-- Info button -->
                  <button
                    type="button"
                    :title="t('remote.opdsDetailTitle')"
                    :aria-label="t('remote.opdsDetailTitle')"
                    class="absolute top-2 right-2 flex h-7 w-7 cursor-pointer items-center justify-center rounded-full bg-black/55 text-white opacity-100 backdrop-blur-sm transition hover:bg-black/75 focus-visible:opacity-100 sm:opacity-0 sm:group-hover:opacity-100"
                    @click.stop="opdsDetailEntry = entry"
                  >
                    <LuInfo class="h-3.5 w-3.5" />
                  </button>
                </div>

                <!-- Title & Author -->
                <div class="mt-2.5 min-w-0 px-0.5">
                  <button
                    type="button"
                    class="block w-full cursor-pointer truncate text-left text-sm font-semibold text-[var(--md-sys-color-on-surface)] transition-colors group-hover:text-[var(--md-sys-color-primary)]"
                    :title="entry.title"
                    @click="opdsDetailEntry = entry"
                  >
                    {{ entry.title }}
                  </button>
                  <p class="mt-0.5 truncate text-xs text-[var(--md-sys-color-on-surface-variant)]">
                    {{ entry.authors.join(', ') || t('remote.opdsUnknownAuthor') }}
                  </p>
                </div>

                <!-- Compact Import Action -->
                <div class="mt-2 flex items-center gap-1.5">
                  <template v-if="getEntrySelectedDownload(entry)">
                    <button
                      v-if="entry.downloads.length > 1"
                      type="button"
                      class="h-7 shrink-0 rounded-full border border-[var(--md-sys-color-outline-variant)] bg-[var(--md-sys-color-surface-container-low)] px-2 font-mono text-[10px] font-semibold uppercase text-[var(--md-sys-color-on-surface-variant)] hover:border-[var(--md-sys-color-primary)] hover:text-[var(--md-sys-color-on-surface)]"
                      :title="t('remote.opdsFormats')"
                      @click="
                        opdsSelectedFormatIdx = {
                          ...opdsSelectedFormatIdx,
                          [entry.id]:
                            ((opdsSelectedFormatIdx[entry.id] ?? 0) + 1) % entry.downloads.length,
                        }
                      "
                    >
                      {{ getEntrySelectedDownload(entry)!.format || 'FILE' }}
                    </button>

                    <button
                      type="button"
                      class="md-btn md-btn-sm md-state !h-7 flex-1 !px-2.5 !text-[11px]"
                      :class="
                        opdsImportedKeys[
                          opdsImportKey(selectedOpdsId, getEntrySelectedDownload(entry)!.href)
                        ]
                          ? 'md-btn-outlined'
                          : 'md-btn-tonal'
                      "
                      :disabled="
                        Boolean(
                          opdsImportingKeys[
                            opdsImportKey(selectedOpdsId, getEntrySelectedDownload(entry)!.href)
                          ],
                        )
                      "
                      @click="handleImportOpds(entry, getEntrySelectedDownload(entry)!)"
                    >
                      <LuLoader2
                        v-if="
                          opdsImportingKeys[
                            opdsImportKey(selectedOpdsId, getEntrySelectedDownload(entry)!.href)
                          ]
                        "
                        class="h-3 w-3 animate-spin"
                      />
                      <LuCheck
                        v-else-if="
                          opdsImportedKeys[
                            opdsImportKey(selectedOpdsId, getEntrySelectedDownload(entry)!.href)
                          ]
                        "
                        class="h-3 w-3"
                      />
                      <LuDownload v-else class="h-3 w-3" />
                      <span>
                        {{
                          opdsImportingKeys[
                            opdsImportKey(selectedOpdsId, getEntrySelectedDownload(entry)!.href)
                          ]
                            ? t('remote.importingBtn')
                            : opdsImportedKeys[
                                  opdsImportKey(
                                    selectedOpdsId,
                                    getEntrySelectedDownload(entry)!.href,
                                  )
                                ]
                              ? t('remote.importedBtn')
                              : t('remote.importBtn')
                        }}
                      </span>
                    </button>
                  </template>
                  <span
                    v-else
                    class="w-full truncate text-center text-[11px] text-[var(--md-sys-color-on-surface-variant)]/70"
                  >
                    {{ t('remote.opdsNoDownload') }}
                  </span>
                </div>
              </article>
            </div>
          </section>

          <!-- Empty Feed -->
          <div
            v-if="opdsFeed.nav_links.length === 0 && opdsFeed.entries.length === 0"
            class="rounded-[24px] bg-[var(--md-sys-color-surface-container-low)] py-14 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
          >
            {{ t('remote.opdsEmptyFeed') }}
          </div>

          <!-- Next Page -->
          <div v-if="opdsFeed.next_url" class="flex justify-center pt-2">
            <button
              type="button"
              :disabled="opdsLoading"
              class="md-btn md-btn-tonal md-btn-sm"
              @click="handleOpdsNextPage"
            >
              <LuLoader2 v-if="opdsLoading" class="h-3.5 w-3.5 animate-spin" />
              <span>{{ t('remote.opdsNextPage') }}</span>
              <LuChevronRight v-if="!opdsLoading" class="h-4 w-4" />
            </button>
          </div>
        </template>
      </template>
    </template>

    <!-- ================= TAB 2: OpenSubsonic Music ================= -->
    <template v-else-if="activeTab === 'subsonic'">
      <div
        v-if="appClientStore.subsonicSources.length === 0"
        class="rounded-[24px] bg-[var(--md-sys-color-surface-container-low)] px-6 py-16 text-center"
      >
        <div
          class="mx-auto mb-3 flex h-12 w-12 items-center justify-center rounded-2xl bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)]"
        >
          <LuDisc3 class="h-6 w-6" />
        </div>
        <p class="text-sm font-semibold text-[var(--md-sys-color-on-surface)]">
          {{ t('remote.subsonicEmptySourcesTitle') }}
        </p>
        <p class="mx-auto mt-1 max-w-md text-xs text-[var(--md-sys-color-on-surface-variant)]">
          {{ t('remote.subsonicEmptySourcesHint') }}
        </p>
        <button
          type="button"
          class="md-btn md-btn-sm md-btn-filled md-state mt-5"
          @click="openAddSourceDialog('subsonic')"
        >
          <LuPlus class="h-4 w-4" />
          <span>{{ t('remote.subsonicAddBtn') }}</span>
        </button>
      </div>

      <template v-else>
        <!-- Subsonic Toolbar -->
        <div class="flex flex-wrap items-center gap-2.5">
          <div class="w-48 shrink-0 sm:w-56">
            <AppSelect
              :model-value="selectedSubsonicId"
              :options="subsonicSourceOptions"
              :placeholder="t('remote.subsonicSelectSource')"
              size="sm"
              @update:model-value="handleSubsonicSelectSource"
            />
          </div>

          <MdSegmentedButton
            :model-value="subsonicAlbumType"
            :options="subsonicSortOptions"
            size="sm"
            :show-check="false"
            @update:model-value="handleSubsonicSortChange"
          />

          <button
            type="button"
            class="md-icon-btn md-icon-btn-sm md-state"
            :disabled="subsonicLoading"
            :title="t('common.refresh')"
            :aria-label="t('common.refresh')"
            @click="subsonicSearchActive ? handleSubsonicSearchSubmit() : fetchSubsonicAlbums(true)"
          >
            <LuRefreshCw class="h-4 w-4" :class="subsonicLoading ? 'animate-spin' : ''" />
          </button>

          <form
            class="relative ml-auto w-full sm:w-64"
            @submit.prevent="handleSubsonicSearchSubmit"
          >
            <LuSearch
              class="pointer-events-none absolute top-1/2 left-3 h-3.5 w-3.5 -translate-y-1/2 text-[var(--md-sys-color-on-surface-variant)]"
            />
            <input
              v-model="subsonicSearchInput"
              type="text"
              :placeholder="t('remote.subsonicSearchPlaceholder')"
              class="md-input h-9 w-full !pr-8 !pl-8 text-xs"
            />
            <button
              v-if="subsonicSearchInput || subsonicSearchActive"
              type="button"
              class="md-icon-btn md-icon-btn-sm absolute top-1/2 right-1 -translate-y-1/2 !h-7 !w-7"
              :aria-label="t('common.cancel')"
              @click="handleSubsonicClearSearch"
            >
              <LuX class="h-3.5 w-3.5" />
            </button>
          </form>
        </div>

        <!-- Error Banner -->
        <div
          v-if="subsonicError"
          class="flex items-center justify-between gap-3 rounded-2xl bg-[var(--md-sys-color-error-container)] px-4 py-3 text-xs font-medium text-[var(--md-sys-color-on-error-container)]"
        >
          <div class="flex min-w-0 items-center gap-2">
            <LuAlertCircle class="h-4 w-4 shrink-0" />
            <span class="break-all">{{ subsonicError }}</span>
          </div>
          <button
            type="button"
            class="md-btn md-btn-sm md-btn-text shrink-0 !text-[var(--md-sys-color-on-error-container)]"
            @click="subsonicSearchActive ? handleSubsonicSearchSubmit() : fetchSubsonicAlbums(true)"
          >
            {{ t('remote.retryBtn') }}
          </button>
        </div>

        <!-- Loading -->
        <div
          v-else-if="subsonicLoading"
          class="flex items-center justify-center py-20 text-xs text-[var(--md-sys-color-on-surface-variant)]"
        >
          <LuLoader2 class="mr-2 h-4 w-4 animate-spin" />
          <span>{{ t('common.loading') }}</span>
        </div>

        <!-- Search Results View -->
        <template v-else-if="subsonicSearchActive">
          <div
            v-if="subsonicSearchSongs.length === 0 && subsonicSearchAlbums.length === 0"
            class="rounded-[24px] bg-[var(--md-sys-color-surface-container-low)] py-14 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
          >
            {{ t('common.noResults') }}
          </div>

          <!-- Matched Songs -->
          <section v-if="subsonicSearchSongs.length > 0" class="space-y-2.5">
            <h2 class="md-label-medium text-[var(--md-sys-color-on-surface-variant)]">
              {{ t('remote.subsonicSearchSongs') }}
            </h2>
            <div class="divide-y divide-[var(--md-sys-color-outline-variant)]/35 overflow-hidden rounded-2xl bg-[var(--md-sys-color-surface-container-low)]">
              <div
                v-for="song in subsonicSearchSongs"
                :key="song.id"
                class="flex items-center gap-3 px-3.5 py-2.5 transition-colors hover:bg-[var(--md-sys-color-surface-container)]"
              >
                <button
                  type="button"
                  class="md-icon-btn md-icon-btn-sm md-state shrink-0"
                  :class="
                    previewSong?.id === song.id
                      ? 'bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)]'
                      : ''
                  "
                  :title="isPreviewingSong(song) ? t('remote.subsonicPause') : t('remote.subsonicPlay')"
                  @click="playPreviewTrack(song, subsonicSearchSongs)"
                >
                  <LuPause v-if="isPreviewingSong(song)" class="h-4 w-4" />
                  <LuPlay v-else class="h-4 w-4" />
                </button>

                <div class="min-w-0 flex-1">
                  <p class="truncate text-sm font-medium text-[var(--md-sys-color-on-surface)]">
                    {{ song.title }}
                  </p>
                  <p class="truncate text-xs text-[var(--md-sys-color-on-surface-variant)]">
                    {{ song.artist }}
                    <span v-if="song.album"> · {{ song.album }}</span>
                  </p>
                </div>

                <span
                  v-if="song.suffix"
                  class="hidden shrink-0 rounded-md bg-[var(--md-sys-color-surface-container-high)] px-1.5 py-0.5 font-mono text-[10px] uppercase text-[var(--md-sys-color-on-surface-variant)] sm:inline"
                >
                  {{ song.suffix }}
                </span>

                <span
                  v-if="song.duration"
                  class="shrink-0 font-mono text-xs tabular-nums text-[var(--md-sys-color-on-surface-variant)]"
                >
                  {{ formatDuration(song.duration) }}
                </span>

                <button
                  type="button"
                  class="md-btn md-btn-sm md-state shrink-0"
                  :class="
                    subsonicImportedIds[subsonicImportKey(selectedSubsonicId, song.id)]
                      ? 'md-btn-outlined'
                      : 'md-btn-tonal'
                  "
                  :disabled="Boolean(subsonicImportingIds[subsonicImportKey(selectedSubsonicId, song.id)])"
                  @click="handleImportSong(song)"
                >
                  <LuLoader2
                    v-if="subsonicImportingIds[subsonicImportKey(selectedSubsonicId, song.id)]"
                    class="h-3.5 w-3.5 animate-spin"
                  />
                  <LuCheck
                    v-else-if="subsonicImportedIds[subsonicImportKey(selectedSubsonicId, song.id)]"
                    class="h-3.5 w-3.5"
                  />
                  <LuDownload v-else class="h-3.5 w-3.5" />
                  <span>
                    {{
                      subsonicImportedIds[subsonicImportKey(selectedSubsonicId, song.id)]
                        ? t('remote.importedBtn')
                        : t('remote.importBtn')
                    }}
                  </span>
                </button>
              </div>
            </div>
          </section>

          <!-- Matched Albums -->
          <section v-if="subsonicSearchAlbums.length > 0" class="space-y-3">
            <h2 class="md-label-medium text-[var(--md-sys-color-on-surface-variant)]">
              {{ t('remote.subsonicSearchAlbums') }}
            </h2>
            <div
              class="grid grid-cols-2 gap-4 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6"
            >
              <button
                v-for="album in subsonicSearchAlbums"
                :key="album.id"
                type="button"
                class="group flex cursor-pointer flex-col text-left outline-none"
                @click="openSubsonicAlbum(album)"
              >
                <MediaCoverImage
                  :src="album.cover_url"
                  :alt="album.name"
                  aspect-class="aspect-square w-full"
                  rounded="rounded-2xl"
                  :fallback-icon="LuDisc3"
                  img-class="h-full w-full object-cover transition duration-300 group-hover:scale-[1.03]"
                  class="shadow-[var(--md-sys-elevation-1)] transition duration-200 group-hover:-translate-y-1"
                />
                <p
                  class="mt-2 truncate text-sm font-semibold text-[var(--md-sys-color-on-surface)] group-hover:text-[var(--md-sys-color-primary)]"
                >
                  {{ album.name }}
                </p>
                <p class="truncate text-xs text-[var(--md-sys-color-on-surface-variant)]">
                  {{ album.artist }}
                </p>
              </button>
            </div>
          </section>
        </template>

        <!-- Album Grid Browse View -->
        <template v-else>
          <div
            v-if="subsonicAlbums.length === 0"
            class="rounded-[24px] bg-[var(--md-sys-color-surface-container-low)] py-14 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
          >
            {{ t('remote.subsonicAlbumsEmpty') }}
          </div>

          <template v-else>
            <div
              class="grid grid-cols-2 gap-4 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6"
            >
              <button
                v-for="album in subsonicAlbums"
                :key="album.id"
                type="button"
                class="group flex cursor-pointer flex-col text-left outline-none"
                @click="openSubsonicAlbum(album)"
              >
                <MediaCoverImage
                  :src="album.cover_url"
                  :alt="album.name"
                  aspect-class="aspect-square w-full"
                  rounded="rounded-2xl"
                  :fallback-icon="LuDisc3"
                  img-class="h-full w-full object-cover transition duration-300 group-hover:scale-[1.03]"
                  class="shadow-[var(--md-sys-elevation-1)] transition duration-200 group-hover:-translate-y-1"
                />
                <div class="mt-2 min-w-0 px-0.5">
                  <p
                    class="truncate text-sm font-semibold text-[var(--md-sys-color-on-surface)] transition-colors group-hover:text-[var(--md-sys-color-primary)]"
                    :title="album.name"
                  >
                    {{ album.name }}
                  </p>
                  <p class="mt-0.5 truncate text-xs text-[var(--md-sys-color-on-surface-variant)]">
                    {{ album.artist }}
                  </p>
                  <p
                    v-if="album.year || album.song_count"
                    class="mt-0.5 truncate text-[11px] tabular-nums text-[var(--md-sys-color-on-surface-variant)]/80"
                  >
                    <span v-if="album.year">{{ album.year }}</span>
                    <span v-if="album.year && album.song_count"> · </span>
                    <span v-if="album.song_count">
                      {{ t('remote.subsonicTracksCount', { count: album.song_count }) }}
                    </span>
                  </p>
                </div>
              </button>
            </div>

            <div v-if="subsonicHasMore" class="flex justify-center pt-2">
              <button
                type="button"
                :disabled="subsonicLoadingMore"
                class="md-btn md-btn-tonal md-btn-sm"
                @click="fetchSubsonicAlbums(false)"
              >
                <LuLoader2 v-if="subsonicLoadingMore" class="h-3.5 w-3.5 animate-spin" />
                <span>{{ t('remote.subsonicLoadMore') }}</span>
              </button>
            </div>
          </template>
        </template>
      </template>
    </template>

    <!-- ================= TAB 3: Manage Remote Sources ================= -->
    <template v-else>
      <div class="flex items-center justify-between gap-3">
        <div class="flex items-center gap-2">
          <h2 class="md-title-small text-[var(--md-sys-color-on-surface)]">
            {{ t('remote.savedSourcesTitle') }}
          </h2>
          <span
            class="md-label-small rounded-full bg-[var(--md-sys-color-surface-container-high)] px-2.5 py-0.5 tabular-nums text-[var(--md-sys-color-on-surface-variant)]"
          >
            {{ managedSources.length }}
          </span>
        </div>

        <button
          type="button"
          class="md-btn md-btn-filled md-btn-sm"
          @click="openAddSourceDialog(embeddedProtocol ?? 'opds')"
        >
          <LuPlus class="h-4 w-4" />
          <span>{{ t('remote.addSource') }}</span>
        </button>
      </div>

      <div
        v-if="managedSources.length === 0"
        class="rounded-[24px] bg-[var(--md-sys-color-surface-container-low)] px-6 py-16 text-center"
      >
        <div
          class="mx-auto mb-3 flex h-12 w-12 items-center justify-center rounded-2xl bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)]"
        >
          <LuBookOpen v-if="embeddedProtocol === 'opds'" class="h-6 w-6" />
          <LuDisc3 v-else-if="embeddedProtocol === 'subsonic'" class="h-6 w-6" />
          <LuCloud v-else class="h-6 w-6" />
        </div>
        <p class="text-sm font-semibold text-[var(--md-sys-color-on-surface)]">
          {{
            embeddedProtocol === 'opds'
              ? t('remote.opdsEmptySourcesTitle')
              : embeddedProtocol === 'subsonic'
                ? t('remote.subsonicEmptySourcesTitle')
                : t('remote.emptySourcesTitle')
          }}
        </p>
        <p class="mx-auto mt-1 max-w-md text-xs text-[var(--md-sys-color-on-surface-variant)]">
          {{
            embeddedProtocol === 'opds'
              ? t('remote.opdsEmptySourcesHint')
              : embeddedProtocol === 'subsonic'
                ? t('remote.subsonicEmptySourcesHint')
                : t('remote.emptySourcesHint')
          }}
        </p>
        <div class="mt-5 flex flex-wrap justify-center gap-2">
          <button
            v-if="!embeddedProtocol || embeddedProtocol === 'opds'"
            type="button"
            class="md-btn md-btn-sm md-btn-filled"
            @click="openAddSourceDialog('opds')"
          >
            <LuBookOpen class="h-3.5 w-3.5" />
            <span>{{ t('remote.opdsAddBtn') }}</span>
          </button>
          <button
            v-if="!embeddedProtocol || embeddedProtocol === 'subsonic'"
            type="button"
            class="md-btn md-btn-sm"
            :class="embeddedProtocol === 'subsonic' ? 'md-btn-filled' : 'md-btn-tonal'"
            @click="openAddSourceDialog('subsonic')"
          >
            <LuDisc3 class="h-3.5 w-3.5" />
            <span>{{ t('remote.subsonicAddBtn') }}</span>
          </button>
        </div>
      </div>

      <div v-else class="grid grid-cols-1 gap-3 md:grid-cols-2">
        <article
          v-for="source in managedSources"
          :key="source.id"
          class="flex flex-col justify-between gap-3 rounded-[22px] border border-[var(--md-sys-color-outline-variant)]/50 bg-[var(--md-sys-color-surface-container-low)] p-4 transition-colors hover:bg-[var(--md-sys-color-surface-container)]"
        >
          <div class="min-w-0 space-y-1.5">
            <div class="flex items-center gap-2">
              <span
                class="inline-flex shrink-0 items-center gap-1 rounded-full px-2.5 py-0.5 text-[11px] font-semibold"
                :class="
                  source.protocol === 'opds'
                    ? 'bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)]'
                    : 'bg-[var(--md-sys-color-tertiary-container)] text-[var(--md-sys-color-on-tertiary-container)]'
                "
              >
                <LuBookOpen v-if="source.protocol === 'opds'" class="h-3 w-3" />
                <LuDisc3 v-else class="h-3 w-3" />
                <span>{{ source.protocol === 'opds' ? 'OPDS' : 'OpenSubsonic' }}</span>
              </span>

              <h3 class="truncate text-sm font-semibold text-[var(--md-sys-color-on-surface)]">
                {{ source.name || t('remote.unnamed') }}
              </h3>

              <span
                v-if="source.allow_insecure_tls"
                class="inline-flex shrink-0 items-center text-[var(--md-sys-color-on-surface-variant)]"
                :title="t('remote.insecureBadge')"
              >
                <LuShieldAlert class="h-3.5 w-3.5" />
              </span>
            </div>

            <p class="truncate font-mono text-xs text-[var(--md-sys-color-on-surface-variant)]">
              {{ source.url }}
            </p>

            <div
              v-if="source.username || source.has_password"
              class="flex flex-wrap items-center gap-1.5 text-[11px] text-[var(--md-sys-color-on-surface-variant)]"
            >
              <span
                v-if="source.username"
                class="rounded-md bg-[var(--md-sys-color-surface-container-high)] px-2 py-0.5 font-mono"
              >
                @{{ source.username }}
              </span>
              <span
                v-if="source.has_password"
                class="rounded-md bg-[var(--md-sys-color-surface-container-high)] px-2 py-0.5"
              >
                {{ t('remote.passwordSavedBadge') }}
              </span>
            </div>

            <!-- Inline Test Result -->
            <div
              v-if="cardTestResults[source.id]"
              class="mt-1 flex items-center gap-1.5 rounded-xl px-2.5 py-1.5 text-[11px] font-medium"
              :class="
                cardTestResults[source.id]!.ok
                  ? 'bg-[color-mix(in_srgb,var(--md-sys-color-success,#16a34a)_14%,transparent)] text-[var(--md-sys-color-success,#15803d)]'
                  : 'bg-[var(--md-sys-color-error-container)] text-[var(--md-sys-color-on-error-container)]'
              "
            >
              <LuCheckCircle2 v-if="cardTestResults[source.id]!.ok" class="h-3.5 w-3.5 shrink-0" />
              <LuAlertCircle v-else class="h-3.5 w-3.5 shrink-0" />
              <span class="truncate">
                <template v-if="cardTestResults[source.id]!.ok">
                  {{ t('remote.testOk', { latency: cardTestResults[source.id]!.latency_ms }) }}
                  <span v-if="cardTestResults[source.id]!.detail" class="opacity-85">
                    · {{ cardTestResults[source.id]!.detail }}
                  </span>
                </template>
                <template v-else>
                  {{ cardTestResults[source.id]!.error || t('remote.testFailed') }}
                </template>
              </span>
            </div>
          </div>

          <div class="flex items-center justify-between gap-2 pt-1">
            <button
              type="button"
              class="md-btn md-btn-tonal md-btn-sm"
              @click="handleBrowseSource(source)"
            >
              <span>{{ t('remote.browseBtn') }}</span>
              <LuChevronRight class="h-3.5 w-3.5" />
            </button>

            <div class="flex items-center gap-1">
              <button
                type="button"
                class="md-icon-btn md-icon-btn-sm md-state"
                :disabled="testingSourceId === source.id"
                :title="t('remote.testBtn')"
                :aria-label="t('remote.testBtn')"
                @click="handleTestSavedSource(source)"
              >
                <LuLoader2 v-if="testingSourceId === source.id" class="h-4 w-4 animate-spin" />
                <LuWifi v-else class="h-4 w-4" />
              </button>

              <button
                type="button"
                class="md-icon-btn md-icon-btn-sm md-state"
                :title="t('remote.editBtn')"
                :aria-label="t('remote.editBtn')"
                @click="openEditSourceDialog(source)"
              >
                <LuPencil class="h-4 w-4" />
              </button>

              <button
                type="button"
                class="md-icon-btn md-icon-btn-sm md-state hover:text-[var(--md-sys-color-error)]"
                :disabled="deletingSourceId === source.id"
                :title="t('common.delete')"
                :aria-label="t('common.delete')"
                @click="handleDeleteSource(source)"
              >
                <LuLoader2 v-if="deletingSourceId === source.id" class="h-4 w-4 animate-spin" />
                <LuTrash2 v-else class="h-4 w-4" />
              </button>
            </div>
          </div>
        </article>
      </div>
    </template>

    <!-- ================= Built-in MD3 Subsonic Audio Preview Bar ================= -->
    <audio
      ref="previewAudioRef"
      class="hidden"
      @play="previewPlaying = true; previewBuffering = false"
      @pause="previewPlaying = false"
      @waiting="previewBuffering = true"
      @canplay="previewBuffering = false"
      @timeupdate="onPreviewTimeUpdate"
      @loadedmetadata="onPreviewTimeUpdate"
      @ended="onPreviewEnded"
    />

    <Transition name="md-dialog">
      <div
        v-if="previewSong"
        role="region"
        :aria-label="t('remote.subsonicPreviewPlaying')"
        class="sticky bottom-4 z-20 mx-auto max-w-3xl rounded-2xl border border-[var(--md-sys-color-outline-variant)]/50 bg-[var(--md-sys-color-surface-container-high)]/95 px-4 py-3 shadow-[var(--md-sys-elevation-3)] backdrop-blur-md"
      >
        <div class="flex flex-wrap items-center gap-3">
          <MediaCoverImage
            :src="previewSong.cover_url"
            :alt="previewSong.title"
            aspect-class="h-10 w-10 shrink-0"
            rounded="rounded-xl"
            :fallback-icon="LuMusic"
          />

          <div class="min-w-0 flex-1">
            <p class="truncate text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
              {{ previewSong.title }}
            </p>
            <p class="truncate text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
              {{ previewSong.artist }}
              <span v-if="previewSong.album"> · {{ previewSong.album }}</span>
            </p>
          </div>

          <!-- Transport Controls -->
          <div class="flex items-center gap-1">
            <button
              type="button"
              class="md-icon-btn md-icon-btn-sm md-state"
              :disabled="previewIndex <= 0"
              :title="t('remote.subsonicPrevTrack')"
              :aria-label="t('remote.subsonicPrevTrack')"
              @click="stepPreviewTrack(-1)"
            >
              <LuSkipBack class="h-4 w-4" />
            </button>

            <button
              type="button"
              class="md-state flex h-9 w-9 cursor-pointer items-center justify-center rounded-full bg-[var(--md-sys-color-primary)] text-[var(--md-sys-color-on-primary)]"
              :title="previewPlaying ? t('remote.subsonicPause') : t('remote.subsonicPlay')"
              :aria-label="previewPlaying ? t('remote.subsonicPause') : t('remote.subsonicPlay')"
              @click="togglePreviewPlay"
            >
              <LuLoader2 v-if="previewBuffering" class="h-4 w-4 animate-spin" />
              <LuPause v-else-if="previewPlaying" class="h-4 w-4" />
              <LuPlay v-else class="h-4 w-4 translate-x-0.5" />
            </button>

            <button
              type="button"
              class="md-icon-btn md-icon-btn-sm md-state"
              :disabled="previewIndex >= previewQueue.length - 1"
              :title="t('remote.subsonicNextTrack')"
              :aria-label="t('remote.subsonicNextTrack')"
              @click="stepPreviewTrack(1)"
            >
              <LuSkipForward class="h-4 w-4" />
            </button>
          </div>

          <div class="flex items-center gap-1">
            <button
              type="button"
              class="md-btn md-btn-sm md-btn-tonal"
              :disabled="
                Boolean(subsonicImportingIds[subsonicImportKey(selectedSubsonicId, previewSong.id)])
              "
              @click="handleImportSong(previewSong)"
            >
              <LuLoader2
                v-if="subsonicImportingIds[subsonicImportKey(selectedSubsonicId, previewSong.id)]"
                class="h-3.5 w-3.5 animate-spin"
              />
              <LuCheck
                v-else-if="
                  subsonicImportedIds[subsonicImportKey(selectedSubsonicId, previewSong.id)]
                "
                class="h-3.5 w-3.5"
              />
              <LuDownload v-else class="h-3.5 w-3.5" />
              <span class="hidden sm:inline">
                {{
                  subsonicImportedIds[subsonicImportKey(selectedSubsonicId, previewSong.id)]
                    ? t('remote.importedBtn')
                    : t('remote.importBtn')
                }}
              </span>
            </button>

            <button
              type="button"
              class="md-icon-btn md-icon-btn-sm md-state"
              :title="t('remote.subsonicClosePreview')"
              :aria-label="t('remote.subsonicClosePreview')"
              @click="closePreviewBar"
            >
              <LuX class="h-4 w-4" />
            </button>
          </div>
        </div>

        <!-- Seek Bar -->
        <div class="mt-2 flex items-center gap-2.5">
          <span class="w-9 shrink-0 font-mono text-[10px] tabular-nums text-[var(--md-sys-color-on-surface-variant)]">
            {{ formatTime(previewCurrentTime) }}
          </span>
          <input
            type="range"
            min="0"
            :max="Math.max(1, previewDuration)"
            step="0.5"
            :value="previewCurrentTime"
            class="h-1.5 flex-1 cursor-pointer accent-[var(--md-sys-color-primary)]"
            @input="seekPreview"
          />
          <span class="w-9 shrink-0 text-right font-mono text-[10px] tabular-nums text-[var(--md-sys-color-on-surface-variant)]">
            {{ formatTime(previewDuration) }}
          </span>
        </div>
      </div>
    </Transition>

    <!-- ================= OPDS Entry Detail Side Sheet ================= -->
    <MdSideSheet
      :open="opdsDetailEntry !== null"
      :title="opdsDetailEntry?.title || t('remote.opdsDetailTitle')"
      :subtitle="opdsDetailEntry?.authors.join(', ') || t('remote.opdsUnknownAuthor')"
      width-class="w-full max-w-md"
      @close="opdsDetailEntry = null"
    >
      <template v-if="opdsDetailEntry">
        <div class="space-y-5">
          <div class="flex gap-4">
            <div
              class="book-shadow relative aspect-[2/3] w-28 shrink-0 overflow-hidden rounded-xl bg-[var(--md-sys-color-surface-container)]"
            >
              <img
                v-if="opdsDetailEntry.cover_url && !opdsCoverFailed[opdsDetailEntry.id]"
                :src="opdsDetailEntry.cover_url"
                :alt="opdsDetailEntry.title"
                class="h-full w-full object-cover"
                @error="opdsCoverFailed = { ...opdsCoverFailed, [opdsDetailEntry.id]: true }"
              />
              <div
                v-else
                class="flex h-full w-full items-center justify-center text-[var(--md-sys-color-on-surface-variant)]/50"
              >
                <LuBookOpen class="h-8 w-8" />
              </div>
            </div>

            <div class="min-w-0 flex-1 space-y-2">
              <p
                v-if="opdsDetailEntry.updated"
                class="font-mono text-[11px] text-[var(--md-sys-color-on-surface-variant)]"
              >
                {{ opdsDetailEntry.updated }}
              </p>
              <p
                v-if="opdsDetailEntry.summary"
                class="whitespace-pre-line text-xs leading-relaxed text-[var(--md-sys-color-on-surface-variant)]"
              >
                {{ opdsDetailEntry.summary }}
              </p>
            </div>
          </div>

          <div class="space-y-2">
            <h4 class="md-label-medium text-[var(--md-sys-color-on-surface-variant)]">
              {{ t('remote.opdsFormats') }}
            </h4>
            <div
              v-if="opdsDetailEntry.downloads.length === 0"
              class="rounded-2xl bg-[var(--md-sys-color-surface-container)] p-4 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
            >
              {{ t('remote.opdsNoDownload') }}
            </div>
            <div v-else class="space-y-2">
              <div
                v-for="dl in opdsDetailEntry.downloads"
                :key="dl.href"
                class="flex items-center justify-between gap-3 rounded-2xl bg-[var(--md-sys-color-surface-container)] px-3.5 py-2.5"
              >
                <div class="min-w-0">
                  <div class="flex items-center gap-2">
                    <span
                      class="rounded-md bg-[var(--md-sys-color-primary-container)] px-1.5 py-0.5 font-mono text-[10px] font-semibold uppercase text-[var(--md-sys-color-on-primary-container)]"
                    >
                      {{ dl.format || 'FILE' }}
                    </span>
                    <span class="truncate text-xs font-medium text-[var(--md-sys-color-on-surface)]">
                      {{ dl.title || dl.format }}
                    </span>
                  </div>
                  <p class="mt-0.5 truncate font-mono text-[10px] text-[var(--md-sys-color-on-surface-variant)]">
                    {{ dl.media_type }}
                  </p>
                </div>

                <button
                  type="button"
                  class="md-btn md-btn-sm md-btn-filled shrink-0"
                  :disabled="Boolean(opdsImportingKeys[opdsImportKey(selectedOpdsId, dl.href)])"
                  @click="handleImportOpds(opdsDetailEntry, dl)"
                >
                  <LuLoader2
                    v-if="opdsImportingKeys[opdsImportKey(selectedOpdsId, dl.href)]"
                    class="h-3.5 w-3.5 animate-spin"
                  />
                  <LuCheck
                    v-else-if="opdsImportedKeys[opdsImportKey(selectedOpdsId, dl.href)]"
                    class="h-3.5 w-3.5"
                  />
                  <LuDownload v-else class="h-3.5 w-3.5" />
                  <span>
                    {{
                      opdsImportedKeys[opdsImportKey(selectedOpdsId, dl.href)]
                        ? t('remote.importedBtn')
                        : t('remote.importBtn')
                    }}
                  </span>
                </button>
              </div>
            </div>
          </div>
        </div>
      </template>
    </MdSideSheet>

    <!-- ================= Subsonic Album Tracks Side Sheet ================= -->
    <MdSideSheet
      :open="albumSheetOpen"
      :title="activeAlbum?.name || ''"
      :subtitle="activeAlbum?.artist || ''"
      width-class="w-full max-w-lg"
      @close="albumSheetOpen = false"
    >
      <template v-if="activeAlbum" #subheader>
        <div class="flex items-center gap-3.5 rounded-2xl bg-[var(--md-sys-color-surface-container)] p-3">
          <MediaCoverImage
            :src="activeAlbum.cover_url"
            :alt="activeAlbum.name"
            aspect-class="h-16 w-16 shrink-0"
            rounded="rounded-xl"
            :fallback-icon="LuDisc3"
          />
          <div class="min-w-0 flex-1 space-y-1">
            <p class="truncate text-xs text-[var(--md-sys-color-on-surface-variant)]">
              <span v-if="activeAlbum.year">{{ activeAlbum.year }}</span>
              <span v-if="activeAlbum.year && albumSongs.length"> · </span>
              <span v-if="albumSongs.length">
                {{ t('remote.subsonicTracksCount', { count: albumSongs.length }) }}
              </span>
              <span v-if="activeAlbum.duration">
                · {{ formatDuration(activeAlbum.duration) }}
              </span>
            </p>
            <button
              v-if="albumSongs.length > 0"
              type="button"
              class="md-btn md-btn-filled md-btn-sm"
              @click="playPreviewTrack(albumSongs[0]!, albumSongs)"
            >
              <LuPlay class="h-3.5 w-3.5" />
              <span>{{ t('remote.subsonicPlayAlbum') }}</span>
            </button>
          </div>
        </div>
      </template>

      <div
        v-if="albumDetailLoading"
        class="flex items-center justify-center py-14 text-xs text-[var(--md-sys-color-on-surface-variant)]"
      >
        <LuLoader2 class="mr-2 h-4 w-4 animate-spin" />
        <span>{{ t('common.loading') }}</span>
      </div>

      <div
        v-else-if="albumDetailError"
        class="flex items-center gap-2 rounded-2xl bg-[var(--md-sys-color-error-container)] px-4 py-3 text-xs font-medium text-[var(--md-sys-color-on-error-container)]"
      >
        <LuAlertCircle class="h-4 w-4 shrink-0" />
        <span class="break-all">{{ albumDetailError }}</span>
      </div>

      <div
        v-else-if="albumSongs.length === 0"
        class="py-12 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
      >
        {{ t('remote.subsonicTracksEmpty') }}
      </div>

      <div v-else class="space-y-1">
        <div
          v-for="(song, idx) in albumSongs"
          :key="song.id"
          class="flex items-center gap-2.5 rounded-2xl px-2.5 py-2 transition-colors"
          :class="
            previewSong?.id === song.id
              ? 'bg-[var(--md-sys-color-secondary-container)]/55'
              : 'hover:bg-[var(--md-sys-color-surface-container)]'
          "
        >
          <button
            type="button"
            class="md-icon-btn md-icon-btn-sm md-state shrink-0"
            :class="
              previewSong?.id === song.id
                ? 'bg-[var(--md-sys-color-primary)] text-[var(--md-sys-color-on-primary)]'
                : ''
            "
            :title="isPreviewingSong(song) ? t('remote.subsonicPause') : t('remote.subsonicPlay')"
            @click="playPreviewTrack(song, albumSongs)"
          >
            <LuPause v-if="isPreviewingSong(song)" class="h-3.5 w-3.5" />
            <LuPlay v-else class="h-3.5 w-3.5" />
          </button>

          <span class="w-5 shrink-0 text-right font-mono text-[11px] tabular-nums text-[var(--md-sys-color-on-surface-variant)]">
            {{ song.track ?? idx + 1 }}
          </span>

          <div class="min-w-0 flex-1">
            <p class="truncate text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
              {{ song.title }}
            </p>
            <p class="truncate text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
              <span>{{ song.artist }}</span>
              <span v-if="song.suffix" class="ml-1.5 font-mono uppercase opacity-80">
                · {{ song.suffix }}
                <template v-if="song.bit_rate"> {{ song.bit_rate }}k</template>
              </span>
              <span v-if="song.size" class="ml-1 font-mono opacity-75">
                · {{ formatBytes(song.size) }}
              </span>
            </p>
          </div>

          <span
            v-if="song.duration"
            class="shrink-0 font-mono text-[11px] tabular-nums text-[var(--md-sys-color-on-surface-variant)]"
          >
            {{ formatDuration(song.duration) }}
          </span>

          <button
            type="button"
            class="md-btn md-btn-sm md-state shrink-0 !h-7 !px-2.5 !text-[11px]"
            :class="
              subsonicImportedIds[subsonicImportKey(selectedSubsonicId, song.id)]
                ? 'md-btn-outlined'
                : 'md-btn-tonal'
            "
            :disabled="Boolean(subsonicImportingIds[subsonicImportKey(selectedSubsonicId, song.id)])"
            @click="handleImportSong(song)"
          >
            <LuLoader2
              v-if="subsonicImportingIds[subsonicImportKey(selectedSubsonicId, song.id)]"
              class="h-3 w-3 animate-spin"
            />
            <LuCheck
              v-else-if="subsonicImportedIds[subsonicImportKey(selectedSubsonicId, song.id)]"
              class="h-3 w-3"
            />
            <LuDownload v-else class="h-3 w-3" />
            <span>
              {{
                subsonicImportedIds[subsonicImportKey(selectedSubsonicId, song.id)]
                  ? t('remote.importedBtn')
                  : t('remote.importBtn')
              }}
            </span>
          </button>
        </div>
      </div>
    </MdSideSheet>

    <!-- ================= Add / Edit Remote Source Dialog ================= -->
    <RemoteSourceFormDialog
      :open="formDialogOpen"
      :editing-source="editingSource"
      :default-protocol="formDefaultProtocol"
      @close="formDialogOpen = false"
      @saved="handleSourceSaved"
    />
  </div>
</template>
