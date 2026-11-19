<script setup lang="ts">
import { ref, watch } from 'vue'
import { LuCopy } from 'vue-icons-plus/lu'
import type { MusicInspectorForm } from './types'

const props = defineProps<{
  form: MusicInspectorForm
  sidecarLrcFilename: string
  fileKey: string
}>()

const lyricsTab = ref<'embedded' | 'sidecar'>('embedded')

watch(
  () => props.fileKey,
  () => {
    if (!props.form.embeddedLyrics && props.form.sidecarLrc) {
      lyricsTab.value = 'sidecar'
    } else {
      lyricsTab.value = 'embedded'
    }
  },
  { immediate: true },
)

function syncLyricsBothWays(): void {
  if (lyricsTab.value === 'embedded') {
    props.form.sidecarLrc = props.form.embeddedLyrics
  } else {
    props.form.embeddedLyrics = props.form.sidecarLrc
  }
}
</script>

<template>
  <div class="space-y-3">
  <div class="grid grid-cols-1 gap-2.5 sm:grid-cols-2">
    <label class="block sm:col-span-2">
      <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">曲目标题 (Title)</span>
      <input
        v-model="form.title"
        type="text"
        class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
      />
    </label>
    <label class="block">
      <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">歌手 / 艺术家 (Artist)</span>
      <input
        v-model="form.artist"
        type="text"
        class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
      />
    </label>
    <label class="block">
      <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">专辑艺术家 (Album Artist)</span>
      <input
        v-model="form.albumArtist"
        type="text"
        class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
      />
    </label>
    <label class="block sm:col-span-2">
      <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">专辑名称 (Album)</span>
      <input
        v-model="form.album"
        type="text"
        class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
      />
    </label>
    <div class="grid grid-cols-3 gap-2 sm:col-span-2">
      <label class="block">
        <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">音轨号</span>
        <input
          v-model="form.track"
          type="number"
          min="0"
          class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
        />
      </label>
      <label class="block">
        <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">碟号</span>
        <input
          v-model="form.disc"
          type="number"
          min="0"
          class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
        />
      </label>
      <label class="block">
        <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">发行年份</span>
        <input
          v-model="form.year"
          type="number"
          min="0"
          class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
        />
      </label>
    </div>
    <label class="block">
      <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">音乐流派 (Genre)</span>
      <input
        v-model="form.genre"
        type="text"
        class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
      />
    </label>
    <label class="block">
      <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">备注 (Comment)</span>
      <input
        v-model="form.comment"
        type="text"
        class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
      />
    </label>
  </div>

  <!-- Dual Lyrics Editor (Embedded + Sidecar .lrc) -->
  <div class="rounded-2xl border border-[var(--ctp-surface0)]/80 bg-[var(--ctp-canvas)] p-3">
    <div class="mb-2 flex items-center justify-between gap-2">
      <div class="flex items-center gap-1 rounded-xl bg-[var(--ctp-mantle)] p-1">
        <button
          type="button"
          class="rounded-lg px-2.5 py-1 text-[11px] font-semibold transition"
          :class="
            lyricsTab === 'embedded'
              ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
              : 'text-[var(--ctp-subtext1)] hover:text-[var(--ctp-text)]'
          "
          @click="lyricsTab = 'embedded'"
        >
          内嵌歌词 ({{ form.embeddedLyrics.trim() ? '已有' : '空' }})
        </button>
        <button
          type="button"
          class="rounded-lg px-2.5 py-1 text-[11px] font-semibold transition"
          :class="
            lyricsTab === 'sidecar'
              ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
              : 'text-[var(--ctp-subtext1)] hover:text-[var(--ctp-text)]'
          "
          @click="lyricsTab = 'sidecar'"
        >
          外挂 .lrc ({{ form.sidecarLrc.trim() ? '已有' : '空' }})
        </button>
      </div>

      <button
        type="button"
        class="inline-flex items-center gap-1 rounded-lg px-2 py-1 text-[11px] text-[var(--ctp-accent)] transition hover:bg-[var(--ctp-accent-soft)]"
        title="将当前面板歌词同步复制到另一侧"
        @click="syncLyricsBothWays"
      >
        <LuCopy class="h-3 w-3" />
        <span>同步到{{ lyricsTab === 'embedded' ? ' .lrc' : '内嵌' }}</span>
      </button>
    </div>

    <textarea
      v-if="lyricsTab === 'embedded'"
      v-model="form.embeddedLyrics"
      rows="7"
      placeholder="[00:12.30] 支持直接写入音频文件内嵌 USLT / Vorbis Lyrics 标签…"
      class="w-full resize-y rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)]/45 p-2.5 font-mono text-xs leading-relaxed text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
    />
    <textarea
      v-else
      v-model="form.sidecarLrc"
      rows="7"
      :placeholder="`保存时将原子写入同名歌词文件 ${sidecarLrcFilename}（留空则删除外挂 .lrc）…`"
      class="w-full resize-y rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)]/45 p-2.5 font-mono text-xs leading-relaxed text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
    />
  </div>
  </div>
</template>
