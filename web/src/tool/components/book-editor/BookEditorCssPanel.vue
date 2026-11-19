<script lang="ts">
export const cssPresets = [
  {
    id: 'balanced',
    label: '默认均衡衬线',
    desc: '正文宋体/思源宋体 · 1.85 行高 · 首行缩进 2 字',
    css: `body {\n  font-family: "Noto Serif CJK SC", "Songti SC", "PingFang SC", Georgia, serif;\n  line-height: 1.85;\n  margin: 0;\n  padding: 1.2em 1.4em;\n  word-wrap: break-word;\n}\nh1, h2, h3 {\n  line-height: 1.35;\n  margin: 1.2em 0 0.8em;\n  font-weight: 700;\n}\np {\n  margin: 0 0 0.85em;\n  text-indent: 2em;\n  text-align: justify;\n}\nfigure, .cover-wrap {\n  text-align: center;\n  margin: 1em 0;\n}\nimg {\n  max-width: 100%;\n  height: auto;\n}`,
  },
  {
    id: 'classic',
    label: '古典宣纸宋刻',
    desc: '章题底纹装饰 · 宽松舒朗段距 · 典籍与古风长篇优选',
    css: `body {\n  font-family: "Songti SC", "STSong", "Noto Serif CJK SC", "SimSun", serif;\n  line-height: 2.0;\n  letter-spacing: 0.03em;\n  margin: 0;\n  padding: 1.4em 1.6em;\n}\nh1, h2 {\n  font-weight: 700;\n  text-align: center;\n  line-height: 1.4;\n  margin: 1.5em 0 1.1em;\n  padding-bottom: 0.45em;\n  border-bottom: 1px solid rgba(128, 128, 128, 0.28);\n}\np {\n  margin: 0 0 0.95em;\n  text-indent: 2em;\n  text-align: justify;\n}\nblockquote {\n  font-family: "Kaiti SC", "STKaiti", "FangSong", serif;\n  margin: 1em 1.2em;\n  padding-left: 0.8em;\n  border-left: 2px solid rgba(128, 128, 128, 0.4);\n}`,
  },
  {
    id: 'modern',
    label: '现代屏显黑体',
    desc: '苹方/鸿蒙黑体 · 无衬线高清晰屏显 · 社科技术图书优选',
    css: `body {\n  font-family: -apple-system, BlinkMacSystemFont, "PingFang SC", "HarmonyOS Sans SC", "Microsoft YaHei", sans-serif;\n  line-height: 1.8;\n  margin: 0;\n  padding: 1.2em 1.5em;\n}\nh1, h2, h3 {\n  font-weight: 700;\n  line-height: 1.35;\n  margin: 1.3em 0 0.75em;\n}\np {\n  margin: 0 0 0.9em;\n  text-indent: 2em;\n  text-align: justify;\n}\ncode, pre {\n  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;\n  font-size: 0.9em;\n}`,
  },
  {
    id: 'lightnovel',
    label: '轻小说插图全屏',
    desc: '插图自适应整屏防截断 · 紧凑对话节奏 · 日轻/网文优选',
    css: `body {\n  font-family: "PingFang SC", "Noto Serif CJK SC", "Hiragino Sans GB", serif;\n  line-height: 1.82;\n  margin: 0;\n  padding: 1em 1.25em;\n}\nh1, h2 {\n  font-weight: 700;\n  line-height: 1.35;\n  margin: 1.1em 0 0.8em;\n}\np {\n  margin: 0 0 0.7em;\n  text-indent: 2em;\n}\nfigure {\n  margin: 0.8em auto;\n  text-align: center;\n  page-break-inside: avoid;\n  break-inside: avoid;\n}\nfigure img, img {\n  max-width: 100%;\n  max-height: 95vh;\n  object-fit: contain;\n  display: block;\n  margin: 0 auto;\n}`,
  },
]

</script>

<script setup lang="ts">
import { LuX } from 'vue-icons-plus/lu'

defineProps<{
  modelValue: string
}>()

const emit = defineEmits<{
  'update:modelValue': [value: string]
  applyPreset: [css: string, label: string]
  close: []
}>()
</script>

<template>
  <div
    class="space-y-2 border-b border-[var(--ctp-surface0)]/80 bg-[var(--ctp-shell)] px-4 py-2.5 text-xs"
  >
    <div class="flex flex-wrap items-center justify-between gap-2">
      <div class="flex flex-wrap items-center gap-1.5">
        <span class="font-semibold text-[var(--ctp-subtext1)]">中文排版预设：</span>
        <button
          v-for="preset in cssPresets"
          :key="preset.id"
          type="button"
          class="rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-2.5 py-1 text-[11px] font-medium text-[var(--ctp-text)] transition hover:border-[var(--ctp-accent)] hover:text-[var(--ctp-accent)]"
          :title="preset.desc"
          @click="emit('applyPreset', preset.css, preset.label)"
        >
          {{ preset.label }}
        </button>
      </div>
      <button
        type="button"
        class="rounded p-1 text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]"
        @click="emit('close')"
      >
        <LuX class="h-3.5 w-3.5" />
      </button>
    </div>
    <textarea
      :value="modelValue"
      rows="5"
      spellcheck="false"
      placeholder="编辑全局 EPUB CSS 样式表（保存 EPUB 或 TXT 转 EPUB 时自动写入样式文件，右侧预览分屏实时生效）…"
      class="w-full rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] p-2 font-mono text-[11px] leading-relaxed text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
      @input="emit('update:modelValue', ($event.target as HTMLTextAreaElement).value)"
    />
  </div>
</template>
