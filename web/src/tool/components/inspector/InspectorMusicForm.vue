<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { LuCopy } from 'vue-icons-plus/lu'
import MdSegmentedButton from '@/components/ui/MdSegmentedButton.vue'
import type { MusicInspectorForm } from './types'

const props = defineProps<{
  form: MusicInspectorForm
  sidecarLrcFilename: string
  fileKey: string
}>()

const lyricsTab = ref<'embedded' | 'sidecar'>('embedded')

const lyricsOptions = computed(() => [
  {
    value: 'embedded' as const,
    label: `内嵌歌词 (${props.form.embeddedLyrics.trim() ? '已有' : '空'})`,
  },
  {
    value: 'sidecar' as const,
    label: `外挂 .lrc (${props.form.sidecarLrc.trim() ? '已有' : '空'})`,
  },
])

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

const inputCls =
  'w-full rounded-xl border border-[var(--md-sys-color-outline-variant)]/70 bg-[var(--md-sys-color-surface)] px-3 py-2 text-xs text-[var(--md-sys-color-on-surface)] outline-none transition focus:border-[var(--md-sys-color-primary)]'
</script>

<template>
  <div class="space-y-3">
    <div class="grid grid-cols-1 gap-2.5 sm:grid-cols-2">
      <label class="block sm:col-span-2">
        <span class="mb-1 block text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">曲目标题 (Title)</span>
        <input v-model="form.title" type="text" :class="inputCls" />
      </label>
      <label class="block">
        <span class="mb-1 block text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">歌手 / 艺术家 (Artist)</span>
        <input v-model="form.artist" type="text" :class="inputCls" />
      </label>
      <label class="block">
        <span class="mb-1 block text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">专辑艺术家 (Album Artist)</span>
        <input v-model="form.albumArtist" type="text" :class="inputCls" />
      </label>
      <label class="block sm:col-span-2">
        <span class="mb-1 block text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">专辑名称 (Album)</span>
        <input v-model="form.album" type="text" :class="inputCls" />
      </label>
      <div class="grid grid-cols-3 gap-2 sm:col-span-2">
        <label class="block">
          <span class="mb-1 block text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">音轨号</span>
          <input v-model="form.track" type="number" min="0" :class="inputCls" />
        </label>
        <label class="block">
          <span class="mb-1 block text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">碟号</span>
          <input v-model="form.disc" type="number" min="0" :class="inputCls" />
        </label>
        <label class="block">
          <span class="mb-1 block text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">发行年份</span>
          <input v-model="form.year" type="number" min="0" :class="inputCls" />
        </label>
      </div>
      <label class="block">
        <span class="mb-1 block text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">音乐流派 (Genre)</span>
        <input v-model="form.genre" type="text" :class="inputCls" />
      </label>
      <label class="block">
        <span class="mb-1 block text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">备注 (Comment)</span>
        <input v-model="form.comment" type="text" :class="inputCls" />
      </label>
    </div>

    <!-- Dual Lyrics Editor (Embedded + Sidecar .lrc) -->
    <div class="rounded-2xl border border-[var(--md-sys-color-outline-variant)]/60 bg-[var(--md-sys-color-surface)] p-3">
      <div class="mb-2 flex flex-wrap items-center justify-between gap-2">
        <MdSegmentedButton v-model="lyricsTab" :options="lyricsOptions" size="sm" />

        <button
          type="button"
          class="md-btn md-btn-sm md-btn-text md-state !h-7 !px-2 !text-[11px]"
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
        class="w-full resize-y rounded-xl border border-[var(--md-sys-color-outline-variant)]/60 bg-[var(--md-sys-color-surface-container-low)] p-2.5 font-mono text-xs leading-relaxed text-[var(--md-sys-color-on-surface)] outline-none transition focus:border-[var(--md-sys-color-primary)]"
      />
      <textarea
        v-else
        v-model="form.sidecarLrc"
        rows="7"
        :placeholder="`保存时将原子写入同名歌词文件 ${sidecarLrcFilename}（留空则删除外挂 .lrc）…`"
        class="w-full resize-y rounded-xl border border-[var(--md-sys-color-outline-variant)]/60 bg-[var(--md-sys-color-surface-container-low)] p-2.5 font-mono text-xs leading-relaxed text-[var(--md-sys-color-on-surface)] outline-none transition focus:border-[var(--md-sys-color-primary)]"
      />
    </div>
  </div>
</template>
