<script setup lang="ts">
import { ref, watch } from 'vue'
import {
  LuCheck,
  LuFolderPlus,
  LuKey,
  LuRefreshCw,
  LuSave,
  LuSettings,
  LuTrash2,
  LuX,
} from 'vue-icons-plus/lu'
import AppSelect from '@/components/ui/AppSelect.vue'
import type { MediaKind } from '@/types/api'
import { toolApi } from '../api'
import type { PublicSettingsDto, RenameTemplatesDto } from '../types'

const props = defineProps<{
  open: boolean
  settings: PublicSettingsDto | null
  initialTab?: 'roots' | 'scrapers' | 'templates'
}>()

const emit = defineEmits<{
  close: []
  updated: [settings: PublicSettingsDto]
}>()

const activeTab = ref<'roots' | 'scrapers' | 'templates'>('roots')

const newRootName = ref('')
const newRootPath = ref('')
const newRootKind = ref<string>('all')
const addingRoot = ref(false)

const tmdbKeyInput = ref('')
const bangumiTokenInput = ref('')
const proxyInput = ref('')
const scrapeLocale = ref('zh-CN')

const templates = ref<RenameTemplatesDto>({
  book: '{author} - {title}',
  comic: '{series} - Ch.{number} - {title}',
  music: '{track} - {artist} - {title}',
  video_movie: '{title} ({year})',
  video_episode: '{show} - S{season}E{episode} - {title}',
})

const saving = ref(false)
const savedBanner = ref(false)
const errorMsg = ref<string | null>(null)

const rootKindOptions = [
  { value: 'all', label: '自动识别全部四域文件' },
  { value: 'book', label: '默认图书 (Book)' },
  { value: 'comic', label: '默认漫画 (Comic)' },
  { value: 'music', label: '默认音乐 (Music)' },
  { value: 'video', label: '默认影视 (Video)' },
]

const localeOptions = [
  { value: 'zh-CN', label: '简体中文 (zh-CN)' },
  { value: 'zh-TW', label: '繁體中文 (zh-TW)' },
  { value: 'ja-JP', label: '日本語 (ja-JP)' },
  { value: 'en-US', label: 'English (en-US)' },
]

watch(
  () => [props.open, props.settings, props.initialTab] as const,
  ([isOpen, s, tab]) => {
    if (!isOpen) return
    if (tab) activeTab.value = tab
    errorMsg.value = null
    savedBanner.value = false
    tmdbKeyInput.value = ''
    bangumiTokenInput.value = ''
    if (s) {
      proxyInput.value = s.http_proxy ?? ''
      scrapeLocale.value = s.default_scrape_locale || 'zh-CN'
      templates.value = { ...s.rename_templates }
    }
  },
)

async function handleAddRoot(): Promise<void> {
  if (!newRootPath.value.trim() || addingRoot.value) return
  addingRoot.value = true
  errorMsg.value = null
  try {
    const defaultKind =
      newRootKind.value === 'all' ? null : (newRootKind.value as MediaKind)
    await toolApi.addRoot(newRootName.value.trim(), newRootPath.value.trim(), defaultKind)
    newRootName.value = ''
    newRootPath.value = ''
    const next = await toolApi.getSettings()
    emit('updated', next)
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    addingRoot.value = false
  }
}

async function handleRemoveRoot(id: string): Promise<void> {
  errorMsg.value = null
  try {
    await toolApi.removeRoot(id)
    const next = await toolApi.getSettings()
    emit('updated', next)
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  }
}

async function handleSaveSettings(): Promise<void> {
  saving.value = true
  errorMsg.value = null
  savedBanner.value = false
  try {
    const next = await toolApi.updateSettings({
      tmdb_api_key: tmdbKeyInput.value.trim() ? tmdbKeyInput.value.trim() : undefined,
      bangumi_access_token: bangumiTokenInput.value.trim()
        ? bangumiTokenInput.value.trim()
        : undefined,
      http_proxy: proxyInput.value,
      default_scrape_locale: scrapeLocale.value,
      rename_templates: { ...templates.value },
    })
    tmdbKeyInput.value = ''
    bangumiTokenInput.value = ''
    emit('updated', next)
    savedBanner.value = true
    setTimeout(() => {
      savedBanner.value = false
    }, 2200)
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    saving.value = false
  }
}

async function clearSecret(which: 'tmdb' | 'bangumi'): Promise<void> {
  saving.value = true
  try {
    const next = await toolApi.updateSettings(
      which === 'tmdb' ? { tmdb_api_key: '' } : { bangumi_access_token: '' },
    )
    emit('updated', next)
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <Teleport to="body">
    <div
      v-if="open"
      class="fixed inset-0 z-50 flex items-center justify-center bg-black/55 p-3 backdrop-blur-xs sm:p-6"
      @click.self="emit('close')"
    >
      <div
        class="flex max-h-[88vh] w-full max-w-3xl flex-col overflow-hidden rounded-3xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] shadow-2xl"
      >
        <!-- Header -->
        <div
          class="flex shrink-0 items-center justify-between gap-3 border-b border-[var(--ctp-surface0)]/80 px-5 py-4"
        >
          <div class="flex items-center gap-2">
            <LuSettings class="h-4 w-4 text-[var(--ctp-accent)]" />
            <h2 class="text-sm font-bold text-[var(--ctp-text)]">xmedia-tool 工作台设置</h2>
          </div>
          <button
            type="button"
            class="flex h-8 w-8 items-center justify-center rounded-xl text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)]/70 hover:text-[var(--ctp-text)]"
            @click="emit('close')"
          >
            <LuX class="h-4 w-4" />
          </button>
        </div>

        <!-- Tabs -->
        <div class="flex shrink-0 gap-1 border-b border-[var(--ctp-surface0)]/75 bg-[var(--ctp-mantle)]/40 px-5 py-2.5">
          <button
            type="button"
            class="rounded-xl px-3.5 py-1.5 text-xs font-semibold transition"
            :class="
              activeTab === 'roots'
                ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
                : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60'
            "
            @click="activeTab = 'roots'"
          >
            白名单目录管理
          </button>
          <button
            type="button"
            class="rounded-xl px-3.5 py-1.5 text-xs font-semibold transition"
            :class="
              activeTab === 'scrapers'
                ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
                : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60'
            "
            @click="activeTab = 'scrapers'"
          >
            刮削源密钥与代理
          </button>
          <button
            type="button"
            class="rounded-xl px-3.5 py-1.5 text-xs font-semibold transition"
            :class="
              activeTab === 'templates'
                ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
                : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60'
            "
            @click="activeTab = 'templates'"
          >
            默认重命名模板
          </button>
        </div>

        <!-- Body -->
        <div class="flex-1 space-y-4 overflow-y-auto p-5">
          <div
            v-if="errorMsg"
            class="rounded-xl border border-red-500/35 bg-red-500/10 px-3 py-2 text-xs text-red-400"
          >
            {{ errorMsg }}
          </div>

          <!-- TAB 1: ROOTS -->
          <div v-if="activeTab === 'roots'" class="space-y-4">
            <form
              class="space-y-3 rounded-2xl border border-[var(--ctp-surface0)]/80 bg-[var(--ctp-mantle)]/40 p-4"
              @submit.prevent="handleAddRoot"
            >
              <div class="text-xs font-semibold text-[var(--ctp-text)]">新增受控媒体根目录</div>
              <div class="grid grid-cols-1 gap-2.5 sm:grid-cols-12">
                <input
                  v-model="newRootName"
                  type="text"
                  placeholder="显示别名（可选，默认取目录名）"
                  class="rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)] sm:col-span-4"
                />
                <input
                  v-model="newRootPath"
                  type="text"
                  placeholder="服务器本地绝对路径，如 /data/media/music"
                  class="rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 font-mono text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)] sm:col-span-8"
                />
              </div>
              <div class="flex flex-wrap items-center justify-between gap-2">
                <div class="w-60">
                  <AppSelect v-model="newRootKind" :options="rootKindOptions" />
                </div>
                <button
                  type="submit"
                  :disabled="!newRootPath.trim() || addingRoot"
                  class="inline-flex items-center gap-1.5 rounded-xl bg-[var(--ctp-accent)] px-4 py-2 text-xs font-semibold text-[var(--ctp-on-accent)] transition hover:opacity-95 disabled:opacity-50"
                >
                  <LuFolderPlus class="h-3.5 w-3.5" />
                  <span>添加根目录</span>
                </button>
              </div>
            </form>

            <div class="space-y-2">
              <div class="text-xs font-semibold text-[var(--ctp-subtext0)]">
                已登记根目录 ({{ settings?.roots.length ?? 0 }})
              </div>
              <div
                v-if="!settings || settings.roots.length === 0"
                class="rounded-2xl border border-dashed border-[var(--ctp-surface0)] py-8 text-center text-xs text-[var(--ctp-subtext0)]"
              >
                暂无根目录，请在上方添加本地文件夹路径
              </div>
              <div
                v-for="r in settings?.roots ?? []"
                :key="r.id"
                class="flex items-center justify-between gap-3 rounded-2xl border border-[var(--ctp-surface0)]/80 bg-[var(--ctp-mantle)]/35 px-4 py-3"
              >
                <div class="min-w-0">
                  <div class="flex items-center gap-2">
                    <span class="text-xs font-bold text-[var(--ctp-text)]">{{ r.name }}</span>
                    <span
                      v-if="r.default_kind"
                      class="rounded-md bg-[var(--ctp-accent-soft)] px-1.5 py-0.5 text-[10px] font-semibold uppercase text-[var(--ctp-accent)]"
                    >
                      {{ r.default_kind }}
                    </span>
                  </div>
                  <div class="mt-0.5 truncate font-mono text-[11px] text-[var(--ctp-subtext0)]">
                    {{ r.path }}
                  </div>
                </div>
                <button
                  type="button"
                  class="flex h-8 w-8 shrink-0 items-center justify-center rounded-xl text-[var(--ctp-subtext0)] transition hover:bg-red-500/15 hover:text-red-400"
                  title="移除白名单（不会删除磁盘文件）"
                  @click="handleRemoveRoot(r.id)"
                >
                  <LuTrash2 class="h-4 w-4" />
                </button>
              </div>
            </div>
          </div>

          <!-- TAB 2: SCRAPERS & PROXY -->
          <div v-else-if="activeTab === 'scrapers'" class="space-y-4">
            <div class="rounded-2xl border border-[var(--ctp-surface0)]/80 bg-[var(--ctp-mantle)]/35 p-4 space-y-3">
              <div class="flex items-center justify-between">
                <div class="flex items-center gap-2 text-xs font-bold text-[var(--ctp-text)]">
                  <LuKey class="h-3.5 w-3.5 text-[var(--ctp-accent)]" />
                  <span>TMDB API Key / v4 Read Access Token</span>
                </div>
                <div class="flex items-center gap-2">
                  <span
                    v-if="settings?.has_tmdb_api_key"
                    class="rounded-full bg-emerald-500/15 px-2.5 py-0.5 font-mono text-[10px] font-semibold text-emerald-500"
                  >
                    已配置 {{ settings.tmdb_api_key_masked }}
                  </span>
                  <button
                    v-if="settings?.has_tmdb_api_key"
                    type="button"
                    class="text-[11px] text-red-400 hover:underline"
                    @click="clearSecret('tmdb')"
                  >
                    清空
                  </button>
                </div>
              </div>
              <input
                v-model="tmdbKeyInput"
                type="password"
                placeholder="输入新的 TMDB v3 API Key 或 v4 Bearer Token（留空保持原值）"
                class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 font-mono text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
              />
            </div>

            <div class="rounded-2xl border border-[var(--ctp-surface0)]/80 bg-[var(--ctp-mantle)]/35 p-4 space-y-3">
              <div class="flex items-center justify-between">
                <div class="flex items-center gap-2 text-xs font-bold text-[var(--ctp-text)]">
                  <LuKey class="h-3.5 w-3.5 text-[var(--ctp-accent)]" />
                  <span>Bangumi Access Token（可选，免密钥亦可检索公开条目）</span>
                </div>
                <div class="flex items-center gap-2">
                  <span
                    v-if="settings?.has_bangumi_token"
                    class="rounded-full bg-emerald-500/15 px-2.5 py-0.5 font-mono text-[10px] font-semibold text-emerald-500"
                  >
                    已配置 {{ settings.bangumi_token_masked }}
                  </span>
                  <button
                    v-if="settings?.has_bangumi_token"
                    type="button"
                    class="text-[11px] text-red-400 hover:underline"
                    @click="clearSecret('bangumi')"
                  >
                    清空
                  </button>
                </div>
              </div>
              <input
                v-model="bangumiTokenInput"
                type="password"
                placeholder="输入 Bangumi Personal Access Token（留空保持原值）"
                class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 font-mono text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
              />
            </div>

            <div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
              <label class="block">
                <span class="mb-1 block text-xs font-medium text-[var(--ctp-subtext0)]">
                  HTTP / SOCKS5 代理地址（用于访问 TMDB / MusicBrainz 等外网源）
                </span>
                <input
                  v-model="proxyInput"
                  type="text"
                  placeholder="如 http://127.0.0.1:7890 或 socks5://127.0.0.1:1080"
                  class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 font-mono text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
                />
              </label>
              <div class="block">
                <span class="mb-1 block text-xs font-medium text-[var(--ctp-subtext0)]">
                  默认刮削语言偏好
                </span>
                <AppSelect v-model="scrapeLocale" :options="localeOptions" />
              </div>
            </div>
          </div>

          <!-- TAB 3: RENAME TEMPLATES -->
          <div v-else class="space-y-3">
            <label class="block">
              <span class="mb-1 block text-xs font-medium text-[var(--ctp-subtext0)]">图书模板 ({author}, {title}, {year})</span>
              <input
                v-model="templates.book"
                type="text"
                class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 font-mono text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
              />
            </label>
            <label class="block">
              <span class="mb-1 block text-xs font-medium text-[var(--ctp-subtext0)]">漫画模板 ({series}, {number}, {volume}, {title}, {year})</span>
              <input
                v-model="templates.comic"
                type="text"
                class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 font-mono text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
              />
            </label>
            <label class="block">
              <span class="mb-1 block text-xs font-medium text-[var(--ctp-subtext0)]">音乐模板 ({track}, {disc}, {artist}, {album}, {title}, {year})</span>
              <input
                v-model="templates.music"
                type="text"
                class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 font-mono text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
              />
            </label>
            <label class="block">
              <span class="mb-1 block text-xs font-medium text-[var(--ctp-subtext0)]">电影模板 ({title}, {year})</span>
              <input
                v-model="templates.video_movie"
                type="text"
                class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 font-mono text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
              />
            </label>
            <label class="block">
              <span class="mb-1 block text-xs font-medium text-[var(--ctp-subtext0)]">剧集单集模板 ({show}, {season}, {episode}, {title}, {year})</span>
              <input
                v-model="templates.video_episode"
                type="text"
                class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 font-mono text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
              />
            </label>
          </div>
        </div>

        <!-- Footer -->
        <div
          class="flex shrink-0 items-center justify-between gap-3 border-t border-[var(--ctp-surface0)]/80 bg-[var(--ctp-mantle)]/35 px-5 py-3.5"
        >
          <span v-if="savedBanner" class="inline-flex items-center gap-1.5 text-xs font-semibold text-emerald-500">
            <LuCheck class="h-4 w-4" />
            设置已保存
          </span>
          <span v-else class="text-[11px] text-[var(--ctp-subtext0)]">
            MusicBrainz、LRCLIB、OpenLibrary 与 Bangumi 默认免密钥即开即用
          </span>

          <div class="flex items-center gap-2">
            <button
              type="button"
              class="rounded-xl px-4 py-2 text-xs font-medium text-[var(--ctp-subtext1)] transition hover:bg-[var(--ctp-surface0)]/65"
              @click="emit('close')"
            >
              关闭
            </button>
            <button
              v-if="activeTab !== 'roots'"
              type="button"
              :disabled="saving"
              class="inline-flex items-center gap-1.5 rounded-xl bg-[var(--ctp-accent)] px-4 py-2 text-xs font-semibold text-[var(--ctp-on-accent)] shadow-sm transition hover:opacity-95 disabled:opacity-50"
              @click="handleSaveSettings"
            >
              <LuRefreshCw v-if="saving" class="h-3.5 w-3.5 animate-spin" />
              <LuSave v-else class="h-3.5 w-3.5" />
              <span>保存配置</span>
            </button>
          </div>
        </div>
      </div>
    </div>
  </Teleport>
</template>
