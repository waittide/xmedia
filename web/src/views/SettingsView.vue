<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import {
  LuAlertCircle,
  LuCheck,
  LuCheckCircle2,
  LuCopy,
  LuKeyRound,
  LuLibrary,
  LuMonitor,
  LuMoon,
  LuPalette,
  LuPlay,
  LuRadio,
  LuRefreshCw,
  LuRotateCw,
  LuSlidersHorizontal,
  LuSun,
  LuTrash2,
  LuUserPlus,
  LuUsers,
} from 'vue-icons-plus/lu'
import {
  CATPPUCCIN_ACCENTS,
  CATPPUCCIN_ACCENT_HEX,
  DARK_PALETTE_PRESETS,
  LIGHT_PALETTE_PRESETS,
  normalizeHex,
  type CatppuccinAccent,
  type CustomPaletteTokens,
} from '@/theme/catppuccin'
import { api, setStoredToken } from '@/api/client'
import { useAuthStore } from '@/stores/auth'
import { useDialogStore } from '@/stores/dialog'
import { useDomainStore } from '@/stores/domain'
import { useEventsStore } from '@/stores/events'
import { useThemeStore } from '@/stores/theme'
import { api as musicApi } from '@/domains/music/api'
import { usePlayerStore } from '@/domains/music/player'
import type { BookmarkDto } from '@/domains/music/types'
import { useBookI18n } from '@/domains/book/i18n'
import { useComicI18n } from '@/domains/comic/i18n'
import type {
  ComicFitMode,
  ComicReaderMode,
  CoreLibraryDto,
  LocaleCode,
  MediaKind,
  ReadingFlow,
  TranscodeProfile,
  UserDto,
  UserRole,
  VideoQualityProfile,
} from '@/types/api'
import AppSelect from '@/components/ui/AppSelect.vue'
import RefreshButton from '@/components/ui/RefreshButton.vue'
import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import AppSwitch from '@/components/ui/AppSwitch.vue'
import AppColorPicker from '@/components/ui/AppColorPicker.vue'
import LayoutCustomizerModal from '@/components/LayoutCustomizerModal.vue'

type SettingsTab = 'appearance' | 'preferences' | 'account' | 'libraries' | 'users'

const { t } = useI18n()
const { t: bookT } = useBookI18n()
const { t: comicT } = useComicI18n()
const route = useRoute()
const router = useRouter()
const authStore = useAuthStore()
const dialogStore = useDialogStore()
const domainStore = useDomainStore()
const eventsStore = useEventsStore()
const playerStore = usePlayerStore()
const themeStore = useThemeStore()

function resolveTabFromQuery(): SettingsTab {
  const q = route.query.tab
  if (q === 'preferences' || q === 'reader' || q === 'playback' || q === 'opds') {
    return 'preferences'
  }
  if (q === 'account' || q === 'profile') return 'account'
  if (authStore.isAdmin && (q === 'libraries' || q === 'users')) return q
  return 'appearance'
}

const activeTab = ref<SettingsTab>(resolveTabFromQuery())

watch(
  () => route.query.tab,
  () => {
    activeTab.value = resolveTabFromQuery()
  },
)

function selectTab(tab: SettingsTab): void {
  activeTab.value = tab
  errorMsg.value = ''
  pwdSuccess.value = ''
  void router.replace({ path: '/settings', query: { tab } })
}

const errorMsg = ref('')
const copiedKey = ref('')
const originUrl = computed(() => window.location.origin)

const bookmarks = ref<BookmarkDto[]>([])

// Account state
const currentPwd = ref('')
const nextPwd = ref('')
const pwdSuccess = ref('')

// Admin Libraries state
const allLibraries = ref<CoreLibraryDto[]>([])
const libraryFilterKind = ref<'all' | MediaKind>('all')
const libKind = ref<MediaKind>(domainStore.activeDomain)
const libName = ref('')
const libPath = ref('')
const scanNow = ref(true)
const mounting = ref(false)
const scanningId = ref<string | null>(null)
const scanningAll = ref(false)

// Admin Users state
const users = ref<UserDto[]>([])
const newUsername = ref('')
const newDisplayName = ref('')
const newPassword = ref('')
const newRole = ref<UserRole>('member')
const creatingUser = ref(false)
const resetPwdMap = ref<Record<string, string>>({})

const localeOptions: { code: LocaleCode; nativeLabel: string; subLabel: string }[] = [
  { code: 'zh-CN', nativeLabel: '简体中文', subLabel: 'Simplified Chinese' },
  { code: 'en-US', nativeLabel: 'English', subLabel: 'English (US)' },
]

const customTokenFields: { key: keyof CustomPaletteTokens; labelKey: string }[] = [
  { key: 'canvas', labelKey: 'theme.tokenCanvas' },
  { key: 'shell', labelKey: 'theme.tokenShell' },
  { key: 'surface', labelKey: 'theme.tokenSurface' },
  { key: 'text', labelKey: 'theme.tokenText' },
  { key: 'subtext', labelKey: 'theme.tokenSubtext' },
]

const bookFontOptions = computed(() => [
  { value: 'system-ui', label: bookT('reader.fontSystem') },
  { value: 'serif', label: bookT('reader.fontSerif') },
  { value: 'mono', label: bookT('reader.fontMono') },
])

const bookFlowOptions = computed(() => [
  { value: 'paginated', label: t('settings.bookFlowPaginated') },
  { value: 'scrolled', label: t('settings.bookFlowScrolled') },
])

const comicModeOptions = computed(() => [
  { value: 'rtl_double', label: t('settings.comicModeRtlDouble') },
  { value: 'ltr_single', label: t('settings.comicModeLtrSingle') },
  { value: 'ltr_double', label: t('settings.comicModeLtrDouble') },
  { value: 'webtoon', label: t('settings.comicModeWebtoon') },
])

const comicFitOptions = computed(() => [
  { value: 'fit_height', label: t('settings.comicFitHeight') },
  { value: 'fit_width', label: t('settings.comicFitWidth') },
  { value: 'original', label: t('settings.comicFitOriginal') },
])

const musicQualityOptions = computed(() => [
  { value: 'raw', label: t('player.qualityRaw') },
  { value: 'opus128', label: t('player.qualityOpus128') },
  { value: 'mp3192', label: t('player.qualityMp3192') },
  { value: 'mp3320', label: t('player.qualityMp3320') },
  { value: 'aac256', label: t('player.qualityAac256') },
])

const videoQualityOptions = computed(() => [
  { value: 'auto', label: t('settings.videoQualityAuto') },
  { value: 'direct', label: t('settings.videoQualityDirect') },
  { value: 'p1080', label: t('settings.videoQuality1080p') },
  { value: 'p720', label: t('settings.videoQuality720p') },
  { value: 'p480', label: t('settings.videoQuality480p') },
])

const domainSelectOptions = computed(() =>
  authStore.enabledModules.map((k) => ({
    value: k,
    label: t(`app.domains.${k}`),
  })),
)

const libraryFilterOptions = computed(() => [
  { value: 'all', label: t('admin.libraryFilterAll') },
  ...domainSelectOptions.value,
])

const roleOptions = computed(() => [
  { value: 'member', label: t('auth.roleMember') },
  { value: 'admin', label: t('auth.roleAdmin') },
])

const filteredLibraries = computed(() => {
  if (libraryFilterKind.value === 'all') return allLibraries.value
  return allLibraries.value.filter((l) => l.media_kind === libraryFilterKind.value)
})

const protocolEndpoints = computed(() => {
  const list: Array<{ key: string; label: string; url: string }> = []
  if (authStore.hasModule('book')) {
    list.push(
      { key: 'book12', label: t('settings.bookOpds12Url'), url: `${originUrl.value}/opds/book/v1.2` },
      { key: 'book20', label: t('settings.bookOpds20Url'), url: `${originUrl.value}/opds/book/v2` },
    )
  }
  if (authStore.hasModule('comic')) {
    list.push({
      key: 'comicPse',
      label: t('settings.comicOpdsPseUrl'),
      url: `${originUrl.value}/opds/comic/v1.2`,
    })
  }
  if (authStore.hasModule('music')) {
    list.push(
      { key: 'subServer', label: t('settings.subsonicServerUrl'), url: originUrl.value },
      { key: 'subRest', label: t('settings.subsonicEndpointUrl'), url: `${originUrl.value}/rest` },
    )
  }
  return list
})

async function refreshAdminLibraries(): Promise<void> {
  allLibraries.value = await api.listLibraries()
}

onMounted(async () => {
  const tasks: Promise<unknown>[] = []
  if (authStore.hasModule('music')) {
    tasks.push(
      musicApi.listBookmarks().then((res) => {
        bookmarks.value = res
      }),
    )
  }
  if (authStore.isAdmin) {
    tasks.push(refreshAdminLibraries())
    tasks.push(
      api.listUsers().then((res) => {
        users.value = res
      }),
    )
  }
  await Promise.allSettled(tasks)
})

watch(
  () => eventsStore.domainRefreshTick,
  () => {
    if (authStore.isAdmin) {
      void refreshAdminLibraries().catch(() => {})
    }
  },
  { deep: true },
)

function accentHex(acc: CatppuccinAccent): string {
  return CATPPUCCIN_ACCENT_HEX[themeStore.isDark ? 'dark' : 'light'][acc]
}

function onCustomTokenInput(
  kind: 'light' | 'dark',
  field: keyof CustomPaletteTokens,
  raw: string,
  fallback: string,
): void {
  const normalized = normalizeHex(raw, fallback)
  themeStore.updateCustomPalette(kind, { [field]: normalized })
}

async function copyText(key: string, val: string): Promise<void> {
  if (!val) return
  try {
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(val)
    } else {
      throw new Error('clipboard API unavailable')
    }
  } catch {
    const ta = document.createElement('textarea')
    ta.value = val
    ta.style.position = 'fixed'
    ta.style.opacity = '0'
    document.body.appendChild(ta)
    ta.select()
    document.execCommand('copy')
    document.body.removeChild(ta)
  }
  copiedKey.value = key
  setTimeout(() => {
    if (copiedKey.value === key) copiedKey.value = ''
  }, 1500)
}

function formatBookmarkPos(ms: number): string {
  const totalSecs = Math.floor(ms / 1000)
  const m = Math.floor(totalSecs / 60)
  const s = totalSecs % 60
  return `${m}:${String(s).padStart(2, '0')}`
}

function playBookmark(bm: BookmarkDto): void {
  playerStore.playTrackAt(bm.track, bm.position_ms / 1000)
}

async function removeBookmark(trackId: string): Promise<void> {
  await musicApi.deleteBookmark(trackId)
  bookmarks.value = bookmarks.value.filter((b) => b.track.id !== trackId)
}

async function submitPasswordChange(): Promise<void> {
  errorMsg.value = ''
  pwdSuccess.value = ''
  try {
    const session = await api.changePassword({
      current_password: currentPwd.value,
      new_password: nextPwd.value,
    })
    setStoredToken(session.token)
    authStore.setUser(session.user)
    currentPwd.value = ''
    nextPwd.value = ''
    pwdSuccess.value = t('auth.passwordUpdated')
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  }
}

async function handleRotateApiKey(): Promise<void> {
  const ok = await dialogStore.confirm({
    title: t('settings.rotateApiKey'),
    message: t('settings.rotateApiKeyConfirm'),
    danger: true,
  })
  if (!ok) return
  const updated = await api.rotateApiKey()
  authStore.setUser(updated)
}

async function handleMountLibrary(): Promise<void> {
  errorMsg.value = ''
  mounting.value = true
  try {
    const created = await api.createLibrary({
      name: libName.value.trim(),
      media_kind: libKind.value,
      root_path: libPath.value.trim(),
    })
    if (scanNow.value) {
      await api.scanLibrary(created.id)
    }
    libName.value = ''
    libPath.value = ''
    eventsStore.bumpDomain(created.media_kind)
    await refreshAdminLibraries()
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    mounting.value = false
  }
}

async function handleRescan(lib: CoreLibraryDto): Promise<void> {
  errorMsg.value = ''
  scanningId.value = lib.id
  try {
    await Promise.all([
      (async () => {
        await api.scanLibrary(lib.id)
        eventsStore.bumpDomain(lib.media_kind)
        await refreshAdminLibraries()
      })(),
      new Promise((resolve) => setTimeout(resolve, 450)),
    ])
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    scanningId.value = null
  }
}

async function handleScanAll(): Promise<void> {
  errorMsg.value = ''
  scanningAll.value = true
  try {
    await Promise.all([
      (async () => {
        for (const lib of filteredLibraries.value) {
          if (lib.kind.type === 'local_mount' && lib.scan_status.state !== 'scanning') {
            await api.scanLibrary(lib.id)
            eventsStore.bumpDomain(lib.media_kind)
          }
        }
        await refreshAdminLibraries()
      })(),
      new Promise((resolve) => setTimeout(resolve, 450)),
    ])
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    scanningAll.value = false
  }
}

async function handleDeleteLibrary(lib: CoreLibraryDto): Promise<void> {
  const confirmed = await dialogStore.confirm({
    title: t('admin.deleteLibrary'),
    message: t('admin.confirmDeleteLibrary', { name: lib.name }),
    danger: true,
  })
  if (!confirmed) return
  errorMsg.value = ''
  try {
    await api.deleteLibrary(lib.id)
    eventsStore.bumpDomain(lib.media_kind)
    await refreshAdminLibraries()
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  }
}

function formatScanStatus(lib: CoreLibraryDto): string {
  const s = lib.scan_status
  if (s.state === 'scanning') {
    if (s.total && s.total > 0) {
      const pct = Math.min(100, Math.round((s.scanned / s.total) * 100))
      return t('admin.scanningWithTotal', { scanned: s.scanned, total: s.total, pct })
    }
    return t('admin.scanning', { scanned: s.scanned })
  }
  if (s.state === 'completed') {
    return t('admin.scanCompleted', {
      scanned: s.scanned,
      added: s.added,
      updated: s.updated,
      missing: s.missing,
    })
  }
  if (s.state === 'failed') return t('admin.scanFailed', { message: s.message })
  return t('admin.scanIdle')
}

async function handleCreateUser(): Promise<void> {
  errorMsg.value = ''
  creatingUser.value = true
  try {
    await api.createUser({
      username: newUsername.value.trim(),
      display_name: newDisplayName.value.trim() || undefined,
      password: newPassword.value,
      role: newRole.value,
    })
    newUsername.value = ''
    newDisplayName.value = ''
    newPassword.value = ''
    newRole.value = 'member'
    users.value = await api.listUsers()
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    creatingUser.value = false
  }
}

async function handleRoleChange(u: UserDto, role: string): Promise<void> {
  errorMsg.value = ''
  try {
    await api.updateUser(u.id, { role: role as UserRole })
    users.value = await api.listUsers()
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  }
}

async function handleActiveToggle(u: UserDto, nextActive: boolean): Promise<void> {
  errorMsg.value = ''
  try {
    await api.updateUser(u.id, { is_active: nextActive })
    users.value = await api.listUsers()
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  }
}

async function handleResetUserPassword(u: UserDto): Promise<void> {
  const pwd = resetPwdMap.value[u.id]?.trim()
  if (!pwd) return
  errorMsg.value = ''
  try {
    await api.updateUser(u.id, { password: pwd })
    resetPwdMap.value[u.id] = ''
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  }
}

async function handleDeleteUser(u: UserDto): Promise<void> {
  const confirmed = await dialogStore.confirm({
    title: t('admin.deleteUser'),
    message: t('admin.confirmDeleteUser', { username: u.username }),
    danger: true,
  })
  if (!confirmed) return
  errorMsg.value = ''
  try {
    await api.deleteUser(u.id)
    users.value = await api.listUsers()
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  }
}
</script>

<template>
  <div class="mx-auto max-w-5xl px-6 py-8 sm:px-8">
    <div class="mb-8">
      <h1 class="text-2xl font-bold tracking-tight">{{ t('settings.title') }}</h1>
      <p class="mt-1 text-xs text-[var(--ctp-subtext0)]">{{ t('settings.subtitle') }}</p>
    </div>

    <div class="flex flex-col gap-8 md:flex-row">
      <!-- Settings Secondary Sidebar (xbook / xmusic style) -->
      <nav class="flex shrink-0 gap-1 overflow-x-auto md:w-52 md:flex-col">
        <button
          type="button"
          class="flex items-center gap-2.5 rounded-xl px-3.5 py-2.5 text-left text-xs font-semibold whitespace-nowrap transition"
          :class="
            activeTab === 'appearance'
              ? 'bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
              : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-mantle)] hover:text-[var(--ctp-text)]'
          "
          @click="selectTab('appearance')"
        >
          <LuPalette class="h-4 w-4 shrink-0" />
          <span>{{ t('settings.navAppearance') }}</span>
        </button>

        <button
          type="button"
          class="flex items-center gap-2.5 rounded-xl px-3.5 py-2.5 text-left text-xs font-semibold whitespace-nowrap transition"
          :class="
            activeTab === 'preferences'
              ? 'bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
              : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-mantle)] hover:text-[var(--ctp-text)]'
          "
          @click="selectTab('preferences')"
        >
          <LuRadio class="h-4 w-4 shrink-0" />
          <span>{{ t('settings.navPreferences') }}</span>
        </button>

        <button
          type="button"
          class="flex items-center gap-2.5 rounded-xl px-3.5 py-2.5 text-left text-xs font-semibold whitespace-nowrap transition"
          :class="
            activeTab === 'account'
              ? 'bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
              : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-mantle)] hover:text-[var(--ctp-text)]'
          "
          @click="selectTab('account')"
        >
          <LuKeyRound class="h-4 w-4 shrink-0" />
          <span>{{ t('settings.navAccount') }}</span>
        </button>

        <template v-if="authStore.isAdmin">
          <div class="my-1 hidden h-px bg-[var(--ctp-surface0)] md:block" />

          <button
            type="button"
            class="flex items-center gap-2.5 rounded-xl px-3.5 py-2.5 text-left text-xs font-semibold whitespace-nowrap transition"
            :class="
              activeTab === 'libraries'
                ? 'bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
                : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-mantle)] hover:text-[var(--ctp-text)]'
            "
            @click="selectTab('libraries')"
          >
            <LuLibrary class="h-4 w-4 shrink-0" />
            <span>{{ t('settings.navLibraries') }}</span>
          </button>

          <button
            type="button"
            class="flex items-center gap-2.5 rounded-xl px-3.5 py-2.5 text-left text-xs font-semibold whitespace-nowrap transition"
            :class="
              activeTab === 'users'
                ? 'bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
                : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-mantle)] hover:text-[var(--ctp-text)]'
            "
            @click="selectTab('users')"
          >
            <LuUsers class="h-4 w-4 shrink-0" />
            <span>{{ t('settings.navUsers') }}</span>
          </button>
        </template>
      </nav>

      <!-- Settings Main Content Area -->
      <div class="min-w-0 flex-1 space-y-6">
        <div
          v-if="errorMsg"
          class="rounded-2xl bg-[var(--ctp-red)]/15 px-4 py-3 text-xs font-medium text-[var(--ctp-red)]"
        >
          {{ errorMsg }}
        </div>

        <!-- ==================== 1. APPEARANCE & LANGUAGE ==================== -->
        <template v-if="activeTab === 'appearance'">
          <section class="card-soft rounded-2xl bg-[var(--ctp-shell)] p-6">
            <h2 class="text-sm font-bold">{{ t('layoutCustomizer.title') }}</h2>
            <p class="mt-1 text-xs text-[var(--ctp-subtext0)]">
              {{ t('layoutCustomizer.subtitle') }}
            </p>
            <div class="mt-4">
              <LayoutCustomizerModal inline :domain="domainStore.activeDomain" />
            </div>
          </section>

          <section class="card-soft rounded-2xl bg-[var(--ctp-shell)] p-6">
            <h2 class="text-sm font-bold">{{ t('theme.languageTitle') }}</h2>
            <p class="mt-1 text-xs text-[var(--ctp-subtext0)]">{{ t('theme.languageDesc') }}</p>

            <div class="mt-4 grid grid-cols-1 gap-3 sm:grid-cols-2">
              <button
                v-for="loc in localeOptions"
                :key="loc.code"
                type="button"
                class="flex items-center justify-between rounded-xl border px-4 py-3 text-left transition"
                :class="
                  themeStore.locale === loc.code
                    ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent-soft)]'
                    : 'border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] hover:border-[var(--ctp-surface1)]'
                "
                @click="themeStore.setLocale(loc.code)"
              >
                <div>
                  <p class="text-sm font-semibold text-[var(--ctp-text)]">{{ loc.nativeLabel }}</p>
                  <p class="text-xs text-[var(--ctp-subtext0)]">{{ loc.subLabel }}</p>
                </div>
                <LuCheck
                  v-if="themeStore.locale === loc.code"
                  class="h-4 w-4 text-[var(--ctp-accent)]"
                />
              </button>
            </div>
          </section>

          <section class="card-soft rounded-2xl bg-[var(--ctp-shell)] p-6">
            <div class="flex flex-wrap items-start justify-between gap-2">
              <div>
                <h2 class="text-sm font-bold">{{ t('theme.modeTitle') }}</h2>
                <p class="mt-1 text-xs text-[var(--ctp-subtext0)]">{{ t('theme.modeDesc') }}</p>
              </div>
              <button
                type="button"
                class="inline-flex items-center gap-1.5 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-1.5 text-xs font-medium text-[var(--ctp-subtext1)] transition hover:text-[var(--ctp-text)]"
                @click="themeStore.resetPalettesToDefault()"
              >
                <LuRotateCw class="h-3.5 w-3.5" />
                <span>{{ t('theme.resetDefaults') }}</span>
              </button>
            </div>

            <div class="mt-4 grid grid-cols-3 gap-3">
              <button
                type="button"
                class="flex items-center justify-center gap-2 rounded-xl border py-2.5 text-xs font-semibold transition"
                :class="
                  themeStore.themeMode === 'system'
                    ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
                    : 'border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] text-[var(--ctp-text)]'
                "
                @click="themeStore.setThemeMode('system')"
              >
                <LuMonitor class="h-4 w-4" />
                <span>{{ t('theme.system') }}</span>
              </button>

              <button
                type="button"
                class="flex items-center justify-center gap-2 rounded-xl border py-2.5 text-xs font-semibold transition"
                :class="
                  themeStore.themeMode === 'light'
                    ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
                    : 'border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] text-[var(--ctp-text)]'
                "
                @click="themeStore.setThemeMode('light')"
              >
                <LuSun class="h-4 w-4" />
                <span>{{ t('theme.light') }}</span>
              </button>

              <button
                type="button"
                class="flex items-center justify-center gap-2 rounded-xl border py-2.5 text-xs font-semibold transition"
                :class="
                  themeStore.themeMode === 'dark'
                    ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
                    : 'border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] text-[var(--ctp-text)]'
                "
                @click="themeStore.setThemeMode('dark')"
              >
                <LuMoon class="h-4 w-4" />
                <span>{{ t('theme.dark') }}</span>
              </button>
            </div>
          </section>

          <!-- Light Palette Pool -->
          <section class="card-soft rounded-2xl bg-[var(--ctp-shell)] p-6">
            <h2 class="text-sm font-bold">{{ t('theme.lightPaletteTitle') }}</h2>
            <p class="mt-1 text-xs text-[var(--ctp-subtext0)]">{{ t('theme.lightPaletteDesc') }}</p>

            <div class="mt-4 grid grid-cols-1 gap-3 sm:grid-cols-3">
              <button
                v-for="preset in LIGHT_PALETTE_PRESETS"
                :key="preset.id"
                type="button"
                class="flex items-center justify-between rounded-xl border px-3.5 py-3 text-left text-xs font-semibold transition"
                :class="
                  themeStore.lightPreset === preset.id
                    ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
                    : 'border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] text-[var(--ctp-text)] hover:border-[var(--ctp-surface1)]'
                "
                @click="themeStore.setLightPreset(preset.id)"
              >
                <div class="flex min-w-0 items-center gap-2">
                  <span class="flex -space-x-1">
                    <span
                      class="h-3.5 w-3.5 rounded-full border border-black/15"
                      :style="{ backgroundColor: preset.tokens.canvas }"
                    />
                    <span
                      class="h-3.5 w-3.5 rounded-full border border-black/15"
                      :style="{ backgroundColor: preset.tokens.shell }"
                    />
                    <span
                      class="h-3.5 w-3.5 rounded-full border border-black/15"
                      :style="{ backgroundColor: preset.defaultAccent }"
                    />
                  </span>
                  <span class="truncate">{{ t(`theme.presets.${preset.id}`) }}</span>
                </div>
                <span
                  v-if="preset.id === 'catppuccin-latte' && themeStore.lightPreset !== preset.id"
                  class="rounded bg-[var(--ctp-surface0)]/60 px-1.5 py-0.5 text-[10px] font-medium text-[var(--ctp-subtext0)]"
                >
                  {{ t('theme.defaultBadge') }}
                </span>
                <LuCheck v-else-if="themeStore.lightPreset === preset.id" class="h-4 w-4 shrink-0" />
              </button>

              <button
                type="button"
                class="flex items-center justify-between rounded-xl border px-3.5 py-3 text-left text-xs font-semibold transition"
                :class="
                  themeStore.lightPreset === 'custom'
                    ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
                    : 'border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] text-[var(--ctp-text)] hover:border-[var(--ctp-surface1)]'
                "
                @click="
                  themeStore.lightPreset === 'custom'
                    ? undefined
                    : themeStore.seedCustomFromActivePreset('light')
                "
              >
                <span class="flex items-center gap-2">
                  <LuSlidersHorizontal class="h-3.5 w-3.5 shrink-0" />
                  <span>{{ t('theme.customPalette') }}</span>
                </span>
                <LuCheck v-if="themeStore.lightPreset === 'custom'" class="h-4 w-4 shrink-0" />
              </button>
            </div>

            <div
              v-if="themeStore.lightPreset === 'custom'"
              class="mt-4 grid grid-cols-2 gap-3 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] p-4 sm:grid-cols-5"
            >
              <div v-for="f in customTokenFields" :key="`light-${f.key}`">
                <label class="mb-1.5 block truncate text-[11px] font-medium text-[var(--ctp-subtext0)]">
                  {{ t(f.labelKey) }}
                </label>
                <div class="flex items-center gap-1.5">
                  <AppColorPicker
                    :model-value="themeStore.customLight[f.key]"
                    @update:model-value="themeStore.updateCustomPalette('light', { [f.key]: $event })"
                  />
                  <input
                    type="text"
                    :value="themeStore.customLight[f.key]"
                    maxlength="7"
                    class="h-8 w-full rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] px-2 font-mono text-xs outline-none focus:border-[var(--ctp-accent)]"
                    @change="onCustomTokenInput('light', f.key, ($event.target as HTMLInputElement).value, themeStore.customLight[f.key])"
                  />
                </div>
              </div>
            </div>
          </section>

          <!-- Dark Palette Pool -->
          <section class="card-soft rounded-2xl bg-[var(--ctp-shell)] p-6">
            <h2 class="text-sm font-bold">{{ t('theme.darkPaletteTitle') }}</h2>
            <p class="mt-1 text-xs text-[var(--ctp-subtext0)]">{{ t('theme.darkPaletteDesc') }}</p>

            <div class="mt-4 grid grid-cols-1 gap-3 sm:grid-cols-3">
              <button
                v-for="preset in DARK_PALETTE_PRESETS"
                :key="preset.id"
                type="button"
                class="flex items-center justify-between rounded-xl border px-3.5 py-3 text-left text-xs font-semibold transition"
                :class="
                  themeStore.darkPreset === preset.id
                    ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
                    : 'border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] text-[var(--ctp-text)] hover:border-[var(--ctp-surface1)]'
                "
                @click="themeStore.setDarkPreset(preset.id)"
              >
                <div class="flex min-w-0 items-center gap-2">
                  <span class="flex -space-x-1">
                    <span
                      class="h-3.5 w-3.5 rounded-full border border-white/20"
                      :style="{ backgroundColor: preset.tokens.canvas }"
                    />
                    <span
                      class="h-3.5 w-3.5 rounded-full border border-white/20"
                      :style="{ backgroundColor: preset.tokens.shell }"
                    />
                    <span
                      class="h-3.5 w-3.5 rounded-full border border-white/20"
                      :style="{ backgroundColor: preset.defaultAccent }"
                    />
                  </span>
                  <span class="truncate">{{ t(`theme.presets.${preset.id}`) }}</span>
                </div>
                <span
                  v-if="preset.id === 'catppuccin-mocha' && themeStore.darkPreset !== preset.id"
                  class="rounded bg-[var(--ctp-surface0)]/60 px-1.5 py-0.5 text-[10px] font-medium text-[var(--ctp-subtext0)]"
                >
                  {{ t('theme.defaultBadge') }}
                </span>
                <LuCheck v-else-if="themeStore.darkPreset === preset.id" class="h-4 w-4 shrink-0" />
              </button>

              <button
                type="button"
                class="flex items-center justify-between rounded-xl border px-3.5 py-3 text-left text-xs font-semibold transition"
                :class="
                  themeStore.darkPreset === 'custom'
                    ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
                    : 'border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] text-[var(--ctp-text)] hover:border-[var(--ctp-surface1)]'
                "
                @click="
                  themeStore.darkPreset === 'custom'
                    ? undefined
                    : themeStore.seedCustomFromActivePreset('dark')
                "
              >
                <span class="flex items-center gap-2">
                  <LuSlidersHorizontal class="h-3.5 w-3.5 shrink-0" />
                  <span>{{ t('theme.customPalette') }}</span>
                </span>
                <LuCheck v-if="themeStore.darkPreset === 'custom'" class="h-4 w-4 shrink-0" />
              </button>
            </div>

            <div
              v-if="themeStore.darkPreset === 'custom'"
              class="mt-4 grid grid-cols-2 gap-3 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] p-4 sm:grid-cols-5"
            >
              <div v-for="f in customTokenFields" :key="`dark-${f.key}`">
                <label class="mb-1.5 block truncate text-[11px] font-medium text-[var(--ctp-subtext0)]">
                  {{ t(f.labelKey) }}
                </label>
                <div class="flex items-center gap-1.5">
                  <AppColorPicker
                    :model-value="themeStore.customDark[f.key]"
                    @update:model-value="themeStore.updateCustomPalette('dark', { [f.key]: $event })"
                  />
                  <input
                    type="text"
                    :value="themeStore.customDark[f.key]"
                    maxlength="7"
                    class="h-8 w-full rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] px-2 font-mono text-xs outline-none focus:border-[var(--ctp-accent)]"
                    @change="onCustomTokenInput('dark', f.key, ($event.target as HTMLInputElement).value, themeStore.customDark[f.key])"
                  />
                </div>
              </div>
            </div>
          </section>

          <!-- 3-Mode Accent Color -->
          <section class="card-soft rounded-2xl bg-[var(--ctp-shell)] p-6">
            <h2 class="text-sm font-bold">{{ t('theme.accentTitle') }}</h2>
            <p class="mt-1 text-xs text-[var(--ctp-subtext0)]">{{ t('theme.accentDesc') }}</p>

            <div class="mt-4 grid grid-cols-1 gap-2.5 sm:grid-cols-3">
              <button
                type="button"
                class="rounded-xl border px-3.5 py-2.5 text-xs font-semibold transition"
                :class="
                  themeStore.accentMode === 'preset'
                    ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
                    : 'border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] text-[var(--ctp-text)]'
                "
                @click="themeStore.setAccentMode('preset')"
              >
                {{ t('theme.accentModePreset') }}
              </button>

              <button
                type="button"
                class="rounded-xl border px-3.5 py-2.5 text-xs font-semibold transition"
                :class="
                  themeStore.accentMode === 'catppuccin'
                    ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
                    : 'border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] text-[var(--ctp-text)]'
                "
                @click="themeStore.setAccentMode('catppuccin')"
              >
                {{ t('theme.accentModeCatppuccin') }}
              </button>

              <button
                type="button"
                class="rounded-xl border px-3.5 py-2.5 text-xs font-semibold transition"
                :class="
                  themeStore.accentMode === 'custom'
                    ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
                    : 'border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] text-[var(--ctp-text)]'
                "
                @click="themeStore.setAccentMode('custom')"
              >
                {{ t('theme.accentModeCustom') }}
              </button>
            </div>

            <div
              v-if="themeStore.accentMode === 'catppuccin'"
              class="mt-4 flex flex-wrap gap-2.5"
            >
              <button
                v-for="acc in CATPPUCCIN_ACCENTS"
                :key="acc"
                type="button"
                class="flex h-8 w-8 items-center justify-center rounded-full transition hover:scale-110"
                :style="{ backgroundColor: accentHex(acc) }"
                :title="t(`theme.accents.${acc}`)"
                @click="themeStore.setCatppuccinAccent(acc)"
              >
                <LuCheck
                  v-if="themeStore.accent === acc"
                  class="h-4 w-4"
                  :style="{ color: themeStore.isDark ? '#11111b' : '#ffffff' }"
                />
              </button>
            </div>

            <div
              v-else-if="themeStore.accentMode === 'custom'"
              class="mt-4 flex items-center gap-2.5"
            >
              <AppColorPicker
                :model-value="themeStore.customAccent"
                @update:model-value="themeStore.setCustomAccentHex($event)"
              />
              <input
                type="text"
                :value="themeStore.customAccent"
                maxlength="7"
                class="h-9 w-32 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 font-mono text-xs outline-none focus:border-[var(--ctp-accent)]"
                @change="themeStore.setCustomAccentHex(normalizeHex(($event.target as HTMLInputElement).value, themeStore.customAccent))"
              />
            </div>
          </section>
        </template>

        <!-- ==================== 2. READERS, PLAYBACK & PROTOCOLS ==================== -->
        <template v-else-if="activeTab === 'preferences'">
          <!-- FFmpeg Engine Status -->
          <section class="card-soft rounded-2xl bg-[var(--ctp-shell)] p-6">
            <div class="flex flex-wrap items-center justify-between gap-3">
              <div>
                <h2 class="text-sm font-bold">{{ t('settings.ffmpegStatusTitle') }}</h2>
                <p
                  v-if="authStore.capabilities?.ffmpeg.ffmpeg_version"
                  class="mt-1 font-mono text-xs text-[var(--ctp-subtext0)]"
                >
                  {{ authStore.capabilities.ffmpeg.ffmpeg_version }}
                </p>
              </div>
              <span
                v-if="authStore.capabilities?.ffmpeg.ffmpeg_available"
                class="inline-flex items-center gap-1.5 rounded-full bg-[var(--ctp-green)]/15 px-3 py-1 text-xs font-semibold text-[var(--ctp-green)]"
              >
                <LuCheckCircle2 class="h-4 w-4" />
                <span>{{ t('settings.ffmpegAvailable') }}</span>
              </span>
              <span
                v-else
                class="inline-flex items-center gap-1.5 rounded-full bg-[var(--ctp-yellow)]/15 px-3 py-1 text-xs font-semibold text-[var(--ctp-yellow)]"
              >
                <LuAlertCircle class="h-4 w-4" />
                <span>{{ t('settings.ffmpegMissing') }}</span>
              </span>
            </div>
          </section>

          <!-- Book Reader Preferences -->
          <section
            v-if="authStore.hasModule('book')"
            class="card-soft space-y-4 rounded-2xl bg-[var(--ctp-shell)] p-6"
          >
            <div>
              <h2 class="text-sm font-bold">{{ t('settings.bookPrefsTitle') }}</h2>
              <p class="mt-1 text-xs text-[var(--ctp-subtext0)]">{{ t('settings.bookPrefsDesc') }}</p>
            </div>

            <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
              <div>
                <label class="mb-1.5 block text-xs font-medium text-[var(--ctp-subtext0)]">
                  {{ bookT('reader.fontFamily') }}
                </label>
                <AppSelect
                  :model-value="themeStore.reader.font_family"
                  :options="bookFontOptions"
                  @update:model-value="themeStore.updateReaderPreferences({ font_family: $event })"
                />
              </div>

              <div>
                <label class="mb-1.5 block text-xs font-medium text-[var(--ctp-subtext0)]">
                  {{ t('settings.bookFlowLabel') }}
                </label>
                <AppSelect
                  :model-value="themeStore.reader.flow"
                  :options="bookFlowOptions"
                  @update:model-value="themeStore.updateReaderPreferences({ flow: $event as ReadingFlow })"
                />
              </div>

              <div>
                <div class="mb-1 flex justify-between text-xs">
                  <span class="text-[var(--ctp-subtext0)]">{{ t('settings.bookFontSize') }}</span>
                  <span class="font-mono">{{ themeStore.reader.font_size }} px</span>
                </div>
                <input
                  type="range"
                  min="13"
                  max="28"
                  step="1"
                  :value="themeStore.reader.font_size"
                  class="w-full accent-[var(--ctp-accent)]"
                  @change="themeStore.updateReaderPreferences({ font_size: Number(($event.target as HTMLInputElement).value) })"
                />
              </div>

              <div>
                <div class="mb-1 flex justify-between text-xs">
                  <span class="text-[var(--ctp-subtext0)]">{{ bookT('reader.lineHeight') }}</span>
                  <span class="font-mono">{{ themeStore.reader.line_height }}</span>
                </div>
                <input
                  type="range"
                  min="1.3"
                  max="2.4"
                  step="0.1"
                  :value="themeStore.reader.line_height"
                  class="w-full accent-[var(--ctp-accent)]"
                  @change="themeStore.updateReaderPreferences({ line_height: Number(($event.target as HTMLInputElement).value) })"
                />
              </div>
            </div>
          </section>

          <!-- Comic Reader Preferences -->
          <section
            v-if="authStore.hasModule('comic')"
            class="card-soft space-y-4 rounded-2xl bg-[var(--ctp-shell)] p-6"
          >
            <div>
              <h2 class="text-sm font-bold">{{ t('settings.comicPrefsTitle') }}</h2>
              <p class="mt-1 text-xs text-[var(--ctp-subtext0)]">{{ t('settings.comicPrefsDesc') }}</p>
            </div>

            <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
              <div>
                <label class="mb-1.5 block text-xs font-medium text-[var(--ctp-subtext0)]">
                  {{ comicT('settings.defaultReaderMode') }}
                </label>
                <AppSelect
                  :model-value="themeStore.comic.default_reader_mode"
                  :options="comicModeOptions"
                  @update:model-value="themeStore.updateComicPreferences({ default_reader_mode: $event as ComicReaderMode })"
                />
              </div>

              <div>
                <label class="mb-1.5 block text-xs font-medium text-[var(--ctp-subtext0)]">
                  {{ comicT('settings.defaultFitMode') }}
                </label>
                <AppSelect
                  :model-value="themeStore.comic.default_fit_mode"
                  :options="comicFitOptions"
                  @update:model-value="themeStore.updateComicPreferences({ default_fit_mode: $event as ComicFitMode })"
                />
              </div>
            </div>

            <div class="flex flex-wrap gap-6 pt-1">
              <AppCheckbox
                :model-value="themeStore.comic.offset_cover_page"
                :label="t('settings.comicOffsetCover')"
                @update:model-value="themeStore.updateComicPreferences({ offset_cover_page: $event })"
              />
              <AppCheckbox
                :model-value="themeStore.comic.dim_images_in_dark"
                :label="comicT('reader.eyeCareDim')"
                @update:model-value="themeStore.updateComicPreferences({ dim_images_in_dark: $event })"
              />
            </div>
          </section>

          <!-- Music Playback Preferences & Bookmarks -->
          <section
            v-if="authStore.hasModule('music')"
            class="card-soft space-y-4 rounded-2xl bg-[var(--ctp-shell)] p-6"
          >
            <div>
              <h2 class="text-sm font-bold">{{ t('settings.playbackDefaultQuality') }}</h2>
              <p class="mt-1 text-xs text-[var(--ctp-subtext0)]">
                {{ t('settings.playbackDefaultQualityDesc') }}
              </p>
            </div>

            <div class="max-w-sm">
              <AppSelect
                :model-value="themeStore.playback.transcode_profile"
                :options="musicQualityOptions"
                @update:model-value="themeStore.updatePlaybackPreferences({ transcode_profile: $event as TranscodeProfile })"
              />
            </div>

            <div v-if="bookmarks.length > 0" class="border-t border-[var(--ctp-surface0)] pt-4">
              <h3 class="mb-2.5 text-xs font-bold text-[var(--ctp-subtext1)]">
                {{ t('settings.bookmarksTitle') }}
              </h3>
              <div class="space-y-2">
                <div
                  v-for="bm in bookmarks"
                  :key="bm.track.id"
                  class="flex items-center justify-between rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3.5 py-2.5 text-xs"
                >
                  <div class="min-w-0 flex-1">
                    <p class="truncate font-semibold text-[var(--ctp-text)]">{{ bm.track.title }}</p>
                    <p class="truncate text-[11px] text-[var(--ctp-subtext0)]">
                      {{ bm.track.artist_name }} · {{ formatBookmarkPos(bm.position_ms) }}
                    </p>
                  </div>
                  <div class="ml-3 flex items-center gap-1.5">
                    <button
                      type="button"
                      class="rounded-lg bg-[var(--ctp-accent-soft)] p-1.5 text-[var(--ctp-accent)] hover:opacity-85"
                      @click="playBookmark(bm)"
                    >
                      <LuPlay class="h-3.5 w-3.5 fill-current" />
                    </button>
                    <button
                      type="button"
                      class="rounded-lg p-1.5 text-[var(--ctp-subtext0)] hover:bg-[var(--ctp-red)]/10 hover:text-[var(--ctp-red)]"
                      @click="removeBookmark(bm.track.id)"
                    >
                      <LuTrash2 class="h-3.5 w-3.5" />
                    </button>
                  </div>
                </div>
              </div>
            </div>
          </section>

          <!-- Video Playback Preferences -->
          <section
            v-if="authStore.hasModule('video')"
            class="card-soft space-y-4 rounded-2xl bg-[var(--ctp-shell)] p-6"
          >
            <div>
              <h2 class="text-sm font-bold">{{ t('settings.videoPrefsTitle') }}</h2>
              <p class="mt-1 text-xs text-[var(--ctp-subtext0)]">{{ t('settings.videoPrefsDesc') }}</p>
            </div>

            <div class="max-w-md">
              <AppSelect
                :model-value="themeStore.video.default_quality"
                :options="videoQualityOptions"
                @update:model-value="themeStore.updateVideoPreferences({ default_quality: $event as VideoQualityProfile })"
              />
            </div>

            <AppCheckbox
              :model-value="themeStore.video.autoplay_next"
              :label="t('settings.videoAutoplayNext')"
              @update:model-value="themeStore.updateVideoPreferences({ autoplay_next: $event })"
            />
          </section>

          <!-- Open Protocols Client Guide (OPDS / PSE / OpenSubsonic) -->
          <section
            v-if="protocolEndpoints.length > 0"
            class="card-soft space-y-4 rounded-2xl bg-[var(--ctp-shell)] p-6"
          >
            <div>
              <h2 class="text-sm font-bold">{{ t('settings.protocolsGuideTitle') }}</h2>
              <p class="mt-1 text-xs leading-relaxed text-[var(--ctp-subtext0)]">
                {{ t('settings.protocolsGuideDesc') }}
              </p>
            </div>

            <div class="space-y-3">
              <div v-for="ep in protocolEndpoints" :key="ep.key">
                <label class="mb-1 block text-xs font-medium text-[var(--ctp-subtext0)]">
                  {{ ep.label }}
                </label>
                <div class="flex items-center gap-2">
                  <input
                    readonly
                    :value="ep.url"
                    class="h-9 flex-1 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 font-mono text-xs text-[var(--ctp-text)] outline-none"
                  />
                  <button
                    type="button"
                    class="inline-flex h-9 shrink-0 items-center gap-1.5 rounded-xl bg-[var(--ctp-accent)] px-3.5 text-xs font-semibold text-[var(--ctp-on-accent)] shadow-sm transition hover:opacity-90"
                    @click="copyText(ep.key, ep.url)"
                  >
                    <LuCheck v-if="copiedKey === ep.key" class="h-3.5 w-3.5" />
                    <LuCopy v-else class="h-3.5 w-3.5" />
                    <span>{{ copiedKey === ep.key ? t('common.copied') : t('common.copy') }}</span>
                  </button>
                </div>
              </div>
            </div>

            <div class="rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3.5 py-2.5 text-xs">
              <span class="font-semibold text-[var(--ctp-subtext1)]">{{ t('settings.subsonicAuthModes') }}：</span>
              <span class="text-[var(--ctp-subtext0)]">{{ t('settings.subsonicAuthModesVal') }}</span>
            </div>
          </section>
        </template>

        <!-- ==================== 3. ACCOUNT & API KEY ==================== -->
        <template v-else-if="activeTab === 'account'">
          <section class="card-soft space-y-4 rounded-2xl bg-[var(--ctp-shell)] p-6">
            <div>
              <h2 class="text-sm font-bold">{{ t('settings.apiKeyTitle') }}</h2>
              <p class="mt-1 text-xs text-[var(--ctp-subtext0)]">{{ t('settings.apiKeyDesc') }}</p>
            </div>

            <div class="flex flex-wrap items-center gap-2">
              <input
                readonly
                :value="authStore.user?.api_key || ''"
                class="h-10 min-w-0 flex-1 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3.5 font-mono text-xs text-[var(--ctp-text)] outline-none"
              />
              <button
                type="button"
                class="inline-flex h-10 items-center gap-1.5 rounded-xl bg-[var(--ctp-accent)] px-4 text-xs font-semibold text-[var(--ctp-on-accent)] transition hover:opacity-90"
                @click="copyText('apiKey', authStore.user?.api_key || '')"
              >
                <LuCheck v-if="copiedKey === 'apiKey'" class="h-3.5 w-3.5" />
                <LuCopy v-else class="h-3.5 w-3.5" />
                <span>{{ copiedKey === 'apiKey' ? t('common.copied') : t('common.copy') }}</span>
              </button>
              <button
                type="button"
                class="inline-flex h-10 items-center gap-1.5 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3.5 text-xs font-semibold text-[var(--ctp-red)] transition hover:bg-[var(--ctp-red)]/10"
                @click="handleRotateApiKey"
              >
                <LuRotateCw class="h-3.5 w-3.5" />
                <span>{{ t('settings.rotateApiKey') }}</span>
              </button>
            </div>
          </section>

          <section class="card-soft rounded-2xl bg-[var(--ctp-shell)] p-6">
            <h2 class="text-sm font-bold">{{ t('auth.password') }}</h2>

            <div
              v-if="pwdSuccess"
              class="mt-3 rounded-xl bg-[var(--ctp-green)]/15 px-3.5 py-2.5 text-xs font-medium text-[var(--ctp-green)]"
            >
              {{ pwdSuccess }}
            </div>

            <form class="mt-4 max-w-md space-y-4" @submit.prevent="submitPasswordChange">
              <div>
                <label class="mb-1.5 block text-xs font-semibold text-[var(--ctp-subtext1)]">
                  {{ t('auth.currentPassword') }}
                </label>
                <input
                  v-model="currentPwd"
                  type="password"
                  required
                  class="h-10 w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3.5 text-xs outline-none focus:border-[var(--ctp-accent)]"
                />
              </div>

              <div>
                <label class="mb-1.5 block text-xs font-semibold text-[var(--ctp-subtext1)]">
                  {{ t('auth.newPassword') }}
                </label>
                <input
                  v-model="nextPwd"
                  type="password"
                  required
                  minlength="6"
                  :placeholder="t('auth.passwordPlaceholder')"
                  class="h-10 w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3.5 text-xs outline-none focus:border-[var(--ctp-accent)]"
                />
              </div>

              <button
                type="submit"
                class="inline-flex h-10 items-center rounded-xl bg-[var(--ctp-accent)] px-5 text-xs font-semibold text-[var(--ctp-on-accent)] shadow-sm transition hover:opacity-90"
              >
                {{ t('common.save') }}
              </button>
            </form>
          </section>
        </template>

        <!-- ==================== 4. MEDIA LIBRARIES (ADMIN) ==================== -->
        <template v-else-if="activeTab === 'libraries' && authStore.isAdmin">
          <section class="card-soft rounded-2xl bg-[var(--ctp-shell)] p-6">
            <h2 class="text-sm font-bold">{{ t('admin.mountLibraryTitle') }}</h2>
            <p class="mt-1 text-xs text-[var(--ctp-subtext0)]">{{ t('admin.mountLibraryDesc') }}</p>

            <form class="mt-4 space-y-4" @submit.prevent="handleMountLibrary">
              <div class="grid grid-cols-1 gap-4 sm:grid-cols-3">
                <div>
                  <label class="mb-1.5 block text-xs font-semibold text-[var(--ctp-subtext1)]">
                    {{ t('admin.libraryKind') }}
                  </label>
                  <AppSelect
                    :model-value="libKind"
                    :options="domainSelectOptions"
                    @update:model-value="libKind = $event as MediaKind"
                  />
                </div>

                <div>
                  <label class="mb-1.5 block text-xs font-semibold text-[var(--ctp-subtext1)]">
                    {{ t('admin.libraryName') }}
                  </label>
                  <input
                    v-model="libName"
                    type="text"
                    required
                    :placeholder="t('admin.libraryNamePlaceholder')"
                    class="h-10 w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3.5 text-xs outline-none focus:border-[var(--ctp-accent)]"
                  />
                </div>

                <div>
                  <label class="mb-1.5 block text-xs font-semibold text-[var(--ctp-subtext1)]">
                    {{ t('admin.libraryPath') }}
                  </label>
                  <input
                    v-model="libPath"
                    type="text"
                    required
                    :placeholder="t('admin.libraryPathPlaceholder')"
                    class="h-10 w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3.5 font-mono text-xs outline-none focus:border-[var(--ctp-accent)]"
                  />
                </div>
              </div>

              <div class="flex flex-wrap items-center justify-between gap-3">
                <AppCheckbox v-model="scanNow" :label="t('admin.scanAfterMountLabel')" />
                <button
                  type="submit"
                  :disabled="mounting"
                  class="inline-flex h-10 items-center gap-2 rounded-xl bg-[var(--ctp-accent)] px-5 text-xs font-semibold text-[var(--ctp-on-accent)] shadow-sm transition hover:opacity-90 disabled:opacity-55"
                >
                  <LuLibrary class="h-4 w-4" />
                  <span>{{ scanNow ? t('admin.mountAndScanBtn') : t('admin.mountOnlyBtn') }}</span>
                </button>
              </div>
            </form>
          </section>

          <section class="card-soft space-y-4 rounded-2xl bg-[var(--ctp-shell)] p-6">
            <div class="flex flex-wrap items-center justify-between gap-3">
              <div class="w-48">
                <AppSelect
                  :model-value="libraryFilterKind"
                  :options="libraryFilterOptions"
                  @update:model-value="libraryFilterKind = $event as 'all' | MediaKind"
                />
              </div>

              <div class="flex items-center gap-2">
                <RefreshButton size="md" :action="refreshAdminLibraries" />
                <button
                  type="button"
                  :disabled="scanningAll"
                  class="inline-flex h-9 items-center gap-1.5 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3.5 text-xs font-semibold text-[var(--ctp-text)] transition hover:border-[var(--ctp-accent)] disabled:opacity-50"
                  @click="handleScanAll"
                >
                  <LuRefreshCw class="h-3.5 w-3.5" :class="scanningAll ? 'animate-spin' : ''" />
                  <span>{{ t('admin.scanAllBtn') }}</span>
                </button>
              </div>
            </div>

            <div class="space-y-3">
              <div
                v-for="lib in filteredLibraries"
                :key="lib.id"
                class="flex flex-col justify-between gap-3 rounded-2xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] p-4 sm:flex-row sm:items-center"
              >
                <div class="min-w-0 flex-1 space-y-1">
                  <div class="flex flex-wrap items-center gap-2">
                    <span
                      class="rounded-md bg-[var(--ctp-accent-soft)] px-2 py-0.5 text-[11px] font-bold text-[var(--ctp-accent)]"
                    >
                      {{ t(`app.domains.${lib.media_kind}`) }}
                    </span>
                    <p class="truncate text-sm font-bold text-[var(--ctp-text)]">{{ lib.name }}</p>
                    <span
                      class="rounded-full bg-[var(--ctp-mantle)] px-2.5 py-0.5 text-[11px] font-medium text-[var(--ctp-subtext0)]"
                    >
                      {{
                        lib.kind.type === 'managed'
                          ? t('admin.managedBadge')
                          : t('admin.localMountBadge')
                      }}
                    </span>
                    <span class="text-xs font-semibold text-[var(--ctp-subtext1)]">
                      · {{ t('admin.itemsCountBadge', { count: lib.item_count }) }}
                    </span>
                  </div>

                  <p
                    v-if="lib.kind.type === 'local_mount'"
                    class="truncate font-mono text-xs text-[var(--ctp-subtext0)]"
                  >
                    {{ lib.kind.root_path }}
                  </p>
                  <p class="text-xs text-[var(--ctp-subtext0)]">{{ formatScanStatus(lib) }}</p>
                </div>

                <div class="flex shrink-0 items-center gap-2">
                  <button
                    v-if="lib.kind.type === 'local_mount'"
                    type="button"
                    :disabled="scanningId === lib.id || lib.scan_status.state === 'scanning'"
                    class="inline-flex h-9 items-center gap-1.5 rounded-xl bg-[var(--ctp-accent-soft)] px-3 text-xs font-semibold text-[var(--ctp-accent)] transition hover:opacity-85 disabled:opacity-50"
                    @click="handleRescan(lib)"
                  >
                    <LuRefreshCw
                      class="h-3.5 w-3.5"
                      :class="
                        scanningId === lib.id || lib.scan_status.state === 'scanning'
                          ? 'animate-spin'
                          : ''
                      "
                    />
                    <span>{{ t('admin.rescanBtn') }}</span>
                  </button>

                  <button
                    v-if="lib.kind.type === 'local_mount'"
                    type="button"
                    class="rounded-xl p-2 text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-red)]/10 hover:text-[var(--ctp-red)]"
                    :title="t('admin.deleteLibrary')"
                    @click="handleDeleteLibrary(lib)"
                  >
                    <LuTrash2 class="h-4 w-4" />
                  </button>
                </div>
              </div>
            </div>
          </section>
        </template>

        <!-- ==================== 5. USERS (ADMIN) ==================== -->
        <template v-else-if="activeTab === 'users' && authStore.isAdmin">
          <section class="card-soft rounded-2xl bg-[var(--ctp-shell)] p-6">
            <h2 class="text-sm font-bold">{{ t('admin.createUserTitle') }}</h2>
            <p class="mt-1 text-xs text-[var(--ctp-subtext0)]">{{ t('admin.createUserDesc') }}</p>

            <form class="mt-4 space-y-4" @submit.prevent="handleCreateUser">
              <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
                <div>
                  <label class="mb-1.5 block text-xs font-semibold text-[var(--ctp-subtext1)]">
                    {{ t('auth.username') }}
                  </label>
                  <input
                    v-model="newUsername"
                    type="text"
                    required
                    minlength="2"
                    :placeholder="t('auth.usernamePlaceholder')"
                    class="h-10 w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3.5 text-xs outline-none focus:border-[var(--ctp-accent)]"
                  />
                </div>

                <div>
                  <label class="mb-1.5 block text-xs font-semibold text-[var(--ctp-subtext1)]">
                    {{ t('auth.displayName') }}
                  </label>
                  <input
                    v-model="newDisplayName"
                    type="text"
                    :placeholder="t('auth.displayNamePlaceholder')"
                    class="h-10 w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3.5 text-xs outline-none focus:border-[var(--ctp-accent)]"
                  />
                </div>

                <div>
                  <label class="mb-1.5 block text-xs font-semibold text-[var(--ctp-subtext1)]">
                    {{ t('auth.password') }}
                  </label>
                  <input
                    v-model="newPassword"
                    type="password"
                    required
                    minlength="6"
                    :placeholder="t('auth.passwordPlaceholder')"
                    class="h-10 w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3.5 text-xs outline-none focus:border-[var(--ctp-accent)]"
                  />
                </div>

                <div>
                  <label class="mb-1.5 block text-xs font-semibold text-[var(--ctp-subtext1)]">
                    {{ t('admin.userRoleLabel') }}
                  </label>
                  <AppSelect
                    :model-value="newRole"
                    :options="roleOptions"
                    @update:model-value="newRole = $event as UserRole"
                  />
                </div>
              </div>

              <button
                type="submit"
                :disabled="creatingUser"
                class="inline-flex h-10 items-center gap-2 rounded-xl bg-[var(--ctp-accent)] px-5 text-xs font-semibold text-[var(--ctp-on-accent)] shadow-sm transition hover:opacity-90 disabled:opacity-55"
              >
                <LuUserPlus class="h-4 w-4" />
                <span>{{ t('admin.createUserBtn') }}</span>
              </button>
            </form>
          </section>

          <section class="card-soft space-y-3 rounded-2xl bg-[var(--ctp-shell)] p-6">
            <div class="flex items-center justify-between gap-3">
              <h2 class="text-sm font-bold">{{ t('admin.usersListTitle') }}</h2>
              <RefreshButton
                :action="async () => { users = await api.listUsers() }"
              />
            </div>

            <div
              v-for="u in users"
              :key="u.id"
              class="flex flex-col justify-between gap-3 rounded-2xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] p-4 lg:flex-row lg:items-center"
            >
              <div class="min-w-0">
                <div class="flex items-center gap-2">
                  <span class="truncate text-sm font-bold text-[var(--ctp-text)]">
                    {{ u.display_name || u.username }}
                  </span>
                  <span class="font-mono text-xs text-[var(--ctp-subtext0)]">@{{ u.username }}</span>
                </div>
              </div>

              <div class="flex flex-wrap items-center gap-2.5">
                <div class="w-36">
                  <AppSelect
                    :model-value="u.role"
                    :options="roleOptions"
                    :disabled="u.id === authStore.user?.id"
                    variant="compact"
                    @update:model-value="handleRoleChange(u, $event)"
                  />
                </div>

                <AppSwitch
                  :model-value="u.is_active"
                  :disabled="u.id === authStore.user?.id"
                  :label="u.is_active ? t('auth.activeAccount') : t('auth.disabledAccount')"
                  @update:model-value="handleActiveToggle(u, $event)"
                />

                <form
                  class="flex items-center gap-1.5"
                  @submit.prevent="handleResetUserPassword(u)"
                >
                  <input
                    v-model="resetPwdMap[u.id]"
                    type="password"
                    minlength="6"
                    :placeholder="t('admin.resetPasswordPlaceholder')"
                    class="h-8 w-36 rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] px-2.5 text-xs outline-none focus:border-[var(--ctp-accent)]"
                  />
                  <button
                    type="submit"
                    class="h-8 rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] px-2.5 text-xs font-medium text-[var(--ctp-text)] hover:border-[var(--ctp-accent)]"
                  >
                    {{ t('admin.resetPasswordBtn') }}
                  </button>
                </form>

                <button
                  v-if="u.id !== authStore.user?.id"
                  type="button"
                  class="rounded-xl p-1.5 text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-red)]/10 hover:text-[var(--ctp-red)]"
                  :title="t('admin.deleteUser')"
                  @click="handleDeleteUser(u)"
                >
                  <LuTrash2 class="h-4 w-4" />
                </button>
              </div>
            </div>
          </section>
        </template>
      </div>
    </div>
  </div>
</template>
