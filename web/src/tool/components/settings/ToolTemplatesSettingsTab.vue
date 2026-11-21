<script setup lang="ts">
import { ref, watch } from 'vue'
import { LuRotateCcw, LuSave } from 'vue-icons-plus/lu'
import type { MediaKind } from '@/types/api'
import { toolApi } from '../../api'
import type { PublicSettingsDto, RenameTemplatesDto } from '../../types'

const DEFAULT_RENAME_TEMPLATES: RenameTemplatesDto = {
  book: '{author}/{series}/{subseries}/<Vol.{volume:2} >{title} ({year})/{title} - {author}',
  comic: '{series}/{series} <Vol.{volume:2}> <Ch.{number:2}> {title}',
  music: '{album_artist}/{album} ({year})/{track:2} - {title}',
  video_movie: '{title} ({year})/{title} ({year})',
  video_episode: '{show} ({year})/Season {season:2}/{show} - S{season:2}E{episode:2} - {title}',
}

const props = defineProps<{
  settings: PublicSettingsDto | null
  domainSubTab: MediaKind
}>()

const emit = defineEmits<{
  updated: [settings: PublicSettingsDto]
  error: [message: string | null]
  saved: []
}>()

const domainBadgeLabels: Record<MediaKind, string> = {
  book: '图书',
  comic: '漫画',
  music: '音乐',
  video: '影视',
}

const domainTokenExamples: Record<MediaKind, { tokens: string[]; spec: string; sample: string }> = {
  book: {
    tokens: ['{author}', '{series}', '{subseries}', '{volume:2}', '{title}', '{year}', '{publisher}'],
    spec: 'Calibre / Kavita 规范',
    sample: '刘慈欣/地球往事/Vol.01 三体 (2008)/三体 - 刘慈欣.epub',
  },
  comic: {
    tokens: ['{series}', '{series_group}', '{volume:2}', '{number:2}', '{title}', '{year}'],
    spec: 'Komga / Kavita 规范',
    sample: '葬送的芙莉莲/葬送的芙莉莲 Vol.01 Ch.01 冒险的终点.cbz',
  },
  music: {
    tokens: ['{album_artist}', '{artist}', '{album}', '{disc}', '{track:2}', '{title}', '{year}'],
    spec: 'MusicBrainz Picard / Navidrome 规范',
    sample: '米津玄師/LOST CORNER (2024)/01 - RED OUT.flac',
  },
  video: {
    tokens: ['{title}', '{original_title}', '{show}', '{year}', '{season:2}', '{episode:2}'],
    spec: 'Jellyfin / Kodi 官方目录规范',
    sample: '葬送的芙莉莲 (2023)/Season 01/葬送的芙莉莲 - S01E01 - 冒险的终点.mkv',
  },
}

const templates = ref<RenameTemplatesDto>({ ...DEFAULT_RENAME_TEMPLATES })
const saving = ref(false)

watch(
  () => props.settings,
  (s) => {
    if (!s) return
    templates.value = { ...s.rename_templates }
  },
  { immediate: true },
)

function resetDomainTemplates(dom: MediaKind): void {
  if (dom === 'book') templates.value.book = DEFAULT_RENAME_TEMPLATES.book
  else if (dom === 'comic') templates.value.comic = DEFAULT_RENAME_TEMPLATES.comic
  else if (dom === 'music') templates.value.music = DEFAULT_RENAME_TEMPLATES.music
  else {
    templates.value.video_movie = DEFAULT_RENAME_TEMPLATES.video_movie
    templates.value.video_episode = DEFAULT_RENAME_TEMPLATES.video_episode
  }
}

async function handleSaveSettings(): Promise<void> {
  saving.value = true
  emit('error', null)
  try {
    const next = await toolApi.updateSettings({
      rename_templates: { ...templates.value },
    })
    emit('updated', next)
    emit('saved')
  } catch (err) {
    emit('error', err instanceof Error ? err.message : String(err))
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <section class="md-card-filled space-y-4 p-6">
    <div class="flex flex-wrap items-start justify-between gap-3">
      <div>
        <h2 class="text-sm font-bold text-[var(--md-sys-color-on-surface)]">
          「{{ domainBadgeLabels[domainSubTab] }}」默认重命名归档模板
        </h2>
        <p class="mt-1 text-xs text-[var(--md-sys-color-on-surface-variant)]">
          对齐 {{ domainTokenExamples[domainSubTab].spec }}；支持
          <code class="font-mono text-[var(--md-sys-color-primary)]">:2</code> 零填充与
          <code class="font-mono text-[var(--md-sys-color-primary)]">&lt;...&gt;</code> 条件段（变量为空自动省略）
        </p>
      </div>

      <button
        type="button"
        class="md-btn md-btn-sm md-btn-outlined md-state"
        @click="resetDomainTemplates(domainSubTab)"
      >
        <LuRotateCcw class="h-3.5 w-3.5" />
        <span>恢复当前类型默认</span>
      </button>
    </div>

    <template v-if="domainSubTab === 'book'">
      <div>
        <label class="mb-1.5 block text-xs font-semibold text-[var(--md-sys-color-on-surface-variant)]">
          图书归档路径模板
        </label>
        <input v-model="templates.book" type="text" class="md-input h-10 w-full font-mono text-xs" />
      </div>
    </template>

    <template v-else-if="domainSubTab === 'comic'">
      <div>
        <label class="mb-1.5 block text-xs font-semibold text-[var(--md-sys-color-on-surface-variant)]">
          漫画归档路径模板
        </label>
        <input v-model="templates.comic" type="text" class="md-input h-10 w-full font-mono text-xs" />
      </div>
    </template>

    <template v-else-if="domainSubTab === 'music'">
      <div>
        <label class="mb-1.5 block text-xs font-semibold text-[var(--md-sys-color-on-surface-variant)]">
          音乐归档路径模板
        </label>
        <input v-model="templates.music" type="text" class="md-input h-10 w-full font-mono text-xs" />
      </div>
    </template>

    <template v-else>
      <div class="space-y-4">
        <div>
          <label class="mb-1.5 block text-xs font-semibold text-[var(--md-sys-color-on-surface-variant)]">
            电影 (Movie) 归档路径模板
          </label>
          <input v-model="templates.video_movie" type="text" class="md-input h-10 w-full font-mono text-xs" />
        </div>
        <div>
          <label class="mb-1.5 block text-xs font-semibold text-[var(--md-sys-color-on-surface-variant)]">
            剧集 / 番剧 (TV Show Episode) 归档路径模板
          </label>
          <input v-model="templates.video_episode" type="text" class="md-input h-10 w-full font-mono text-xs" />
        </div>
      </div>
    </template>

    <div class="space-y-2 rounded-2xl border border-[var(--md-sys-color-outline-variant)]/55 bg-[var(--md-sys-color-surface)] p-3.5">
      <div class="flex flex-wrap items-center gap-1.5">
        <span class="text-[11px] text-[var(--md-sys-color-on-surface-variant)]">可用占位变量：</span>
        <code
          v-for="tok in domainTokenExamples[domainSubTab].tokens"
          :key="tok"
          class="rounded-md bg-[var(--md-sys-color-surface-container-high)] px-2 py-0.5 font-mono text-[11px] text-[var(--md-sys-color-primary)]"
        >
          {{ tok }}
        </code>
      </div>
      <p class="font-mono text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
        示例输出：{{ domainTokenExamples[domainSubTab].sample }}
      </p>
    </div>

    <div class="flex justify-end pt-1">
      <button
        type="button"
        :disabled="saving"
        class="md-btn md-btn-filled md-state"
        @click="handleSaveSettings"
      >
        <LuSave class="h-4 w-4" />
        <span>{{ saving ? '保存中…' : '保存模板设置' }}</span>
      </button>
    </div>
  </section>
</template>
