<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import {
  LuBookOpen,
  LuClapperboard,
  LuCompass,
  LuDisc3,
  LuGlobe,
  LuLayers,
  LuLogOut,
  LuMonitor,
  LuMoon,
  LuPanelLeftClose,
  LuPanelLeftOpen,
  LuRefreshCw,
  LuSettings,
  LuSun,
  LuUpload,
} from 'vue-icons-plus/lu'
import { api } from '@/api/client'
import { useAuthStore } from '@/stores/auth'
import { useEventsStore } from '@/stores/events'
import { useMusicPlayerStore } from '@/stores/musicPlayer'
import { useThemeStore } from '@/stores/theme'
import MusicPlayerBar from '@/components/MusicPlayerBar.vue'

const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const auth = useAuthStore()
const theme = useThemeStore()
const events = useEventsStore()
const musicPlayer = useMusicPlayerStore()

const collapsed = ref(localStorage.getItem('xmedia.sidebar_collapsed') === '1')
const uploading = ref(false)
const uploadFeedback = ref('')
const fileInputRef = ref<HTMLInputElement | null>(null)

function toggleSidebar(): void {
  collapsed.value = !collapsed.value
  localStorage.setItem('xmedia.sidebar_collapsed', collapsed.value ? '1' : '0')
}

const navItems = computed(() => {
  const items = [
    { path: '/', label: t('nav.overview'), icon: LuCompass, show: true },
    {
      path: '/books',
      label: t('nav.books'),
      icon: LuBookOpen,
      show: auth.hasModule('book'),
    },
    {
      path: '/comics',
      label: t('nav.comics'),
      icon: LuLayers,
      show: auth.hasModule('comic'),
    },
    {
      path: '/music',
      label: t('nav.music'),
      icon: LuDisc3,
      show: auth.hasModule('music'),
    },
    {
      path: '/videos',
      label: t('nav.videos'),
      icon: LuClapperboard,
      show: auth.hasModule('video'),
    },
    { path: '/settings', label: t('nav.settings'), icon: LuSettings, show: true },
  ]
  return items.filter((i) => i.show)
})

const activeScanCount = computed(() => Object.keys(events.activeScans).length)

function isNavActive(path: string): boolean {
  if (path === '/') return route.path === '/'
  return route.path.startsWith(path)
}

function triggerUpload(): void {
  fileInputRef.value?.click()
}

async function handleSelectedFiles(ev: Event): Promise<void> {
  const input = ev.target as HTMLInputElement
  const files = input.files ? Array.from(input.files) : []
  input.value = ''
  if (files.length === 0) return

  uploading.value = true
  uploadFeedback.value = ''
  try {
    const books: File[] = []
    const comics: File[] = []
    const music: File[] = []
    const videos: File[] = []

    for (const f of files) {
      const ext = f.name.split('.').pop()?.toLowerCase() ?? ''
      if (['epub', 'pdf', 'txt'].includes(ext)) books.push(f)
      else if (['cbz', 'zip'].includes(ext)) comics.push(f)
      else if (['flac', 'mp3', 'm4a', 'alac', 'ogg', 'opus', 'wav', 'lrc'].includes(ext)) {
        music.push(f)
      } else if (
        ['mp4', 'mkv', 'webm', 'mov', 'avi', 'ts', 'm4v', 'flv', 'srt', 'vtt', 'ass', 'ssa'].includes(
          ext,
        )
      ) {
        videos.push(f)
      }
    }

    if (books.length > 0) await api.uploadBooks(books)
    for (const c of comics) await api.uploadComicArchive(c)
    if (music.length > 0) await api.uploadMusicFiles(music)
    if (videos.length > 0) await api.uploadVideoFiles(videos)

    uploadFeedback.value = `Uploaded ${files.length} file(s)`
    setTimeout(() => {
      uploadFeedback.value = ''
    }, 3000)
  } catch (err) {
    uploadFeedback.value = err instanceof Error ? err.message : String(err)
  } finally {
    uploading.value = false
  }
}

async function handleLogout(): Promise<void> {
  musicPlayer.pause()
  events.disconnect()
  await auth.logout()
  await router.push('/login')
}

onMounted(() => {
  events.connect()
  if (auth.hasModule('music')) {
    void musicPlayer.restoreRemoteQueue()
  }
})

onBeforeUnmount(() => {
  events.disconnect()
})
</script>

<template>
  <div class="flex h-screen w-screen flex-col overflow-hidden bg-[var(--ctp-shell)] text-[var(--ctp-text)]">
    <div class="flex flex-1 min-h-0 overflow-hidden">
      <!-- Left Activity Rail -->
      <aside
        class="flex shrink-0 flex-col border-r border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] transition-all duration-200"
        :class="collapsed ? 'w-16' : 'w-56'"
      >
        <div class="flex h-14 items-center justify-between px-4 border-b border-[var(--ctp-surface0)]">
          <div v-if="!collapsed" class="flex items-center gap-2 font-bold tracking-tight">
            <span class="inline-flex h-7 w-7 items-center justify-center rounded-lg bg-[var(--ctp-accent)] text-xs font-extrabold text-[var(--ctp-on-accent)]">
              XM
            </span>
            <span>xmedia</span>
          </div>
          <button
            type="button"
            class="rounded-lg p-1.5 text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)] hover:text-[var(--ctp-text)]"
            @click="toggleSidebar"
          >
            <LuPanelLeftOpen v-if="collapsed" class="h-4 w-4" />
            <LuPanelLeftClose v-else class="h-4 w-4" />
          </button>
        </div>

        <nav class="flex-1 space-y-1 p-2 overflow-y-auto">
          <RouterLink
            v-for="item in navItems"
            :key="item.path"
            :to="item.path"
            class="flex items-center gap-3 rounded-xl px-3 py-2.5 text-sm font-medium transition"
            :class="
              isNavActive(item.path)
                ? 'bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
                : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]'
            "
          >
            <component :is="item.icon" class="h-4 w-4 shrink-0" />
            <span v-if="!collapsed" class="truncate">{{ item.label }}</span>
          </RouterLink>
        </nav>

        <div class="border-t border-[var(--ctp-surface0)] p-3">
          <div v-if="!collapsed && auth.user" class="mb-2 px-1">
            <div class="truncate text-xs font-semibold">{{ auth.user.display_name || auth.user.username }}</div>
            <div class="text-[11px] uppercase text-[var(--ctp-subtext0)]">{{ auth.user.role }}</div>
          </div>
          <button
            type="button"
            class="flex w-full items-center gap-2 rounded-lg px-2.5 py-2 text-xs text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)] hover:text-[var(--ctp-red)]"
            @click="handleLogout"
          >
            <LuLogOut class="h-4 w-4 shrink-0" />
            <span v-if="!collapsed">{{ t('nav.logout') }}</span>
          </button>
        </div>
      </aside>

      <!-- Main Workspace Column -->
      <div class="flex flex-1 flex-col min-w-0 overflow-hidden bg-[var(--ctp-canvas)]">
        <!-- Top Bar -->
        <header class="flex h-14 shrink-0 items-center justify-between border-b border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)]/60 px-6">
          <div class="flex items-center gap-3 text-xs text-[var(--ctp-subtext0)]">
            <span
              v-if="activeScanCount > 0"
              class="inline-flex items-center gap-1.5 rounded-full bg-[var(--ctp-accent-soft)] px-3 py-1 font-medium text-[var(--ctp-accent)]"
            >
              <LuRefreshCw class="h-3.5 w-3.5 animate-spin" />
              {{ t('common.scanning') }} ({{ activeScanCount }})
            </span>
            <span v-else-if="uploadFeedback" class="font-medium text-[var(--ctp-green)]">
              {{ uploadFeedback }}
            </span>
          </div>

          <div class="flex items-center gap-2">
            <input
              ref="fileInputRef"
              type="file"
              multiple
              class="hidden"
              accept=".epub,.pdf,.txt,.cbz,.zip,.flac,.mp3,.m4a,.ogg,.opus,.wav,.lrc,.mp4,.mkv,.webm,.mov,.avi,.ts,.m4v,.srt,.vtt,.ass"
              @change="handleSelectedFiles"
            />
            <button
              v-if="auth.isAdmin"
              type="button"
              class="inline-flex items-center gap-1.5 rounded-lg bg-[var(--ctp-accent)] px-3 py-1.5 text-xs font-semibold text-[var(--ctp-on-accent)] shadow-sm hover:opacity-90 disabled:opacity-50"
              :disabled="uploading"
              @click="triggerUpload"
            >
              <LuUpload class="h-3.5 w-3.5" />
              {{ uploading ? t('common.loading') : t('nav.upload') }}
            </button>

            <button
              type="button"
              class="rounded-lg border border-[var(--ctp-surface0)] p-1.5 text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)] hover:text-[var(--ctp-text)]"
              @click="theme.toggleLocale()"
            >
              <LuGlobe class="h-4 w-4" />
            </button>

            <button
              type="button"
              class="rounded-lg border border-[var(--ctp-surface0)] p-1.5 text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)] hover:text-[var(--ctp-text)]"
              @click="theme.cycleThemeMode()"
            >
              <LuSun v-if="theme.themeMode === 'light'" class="h-4 w-4" />
              <LuMoon v-else-if="theme.themeMode === 'dark'" class="h-4 w-4" />
              <LuMonitor v-else class="h-4 w-4" />
            </button>
          </div>
        </header>

        <!-- Active View -->
        <main class="flex-1 overflow-y-auto">
          <RouterView />
        </main>
      </div>
    </div>

    <!-- Persistent Cross-Domain Music Player Bar -->
    <MusicPlayerBar />
  </div>
</template>
