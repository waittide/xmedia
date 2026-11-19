<script setup lang="ts">
import AppSelect from '@/components/ui/AppSelect.vue'
import type { ComicInspectorForm } from './types'

defineProps<{
  form: ComicInspectorForm
}>()

const directionOptions = [
  { value: 'rtl', label: '日漫右翻 (RTL)' },
  { value: 'ltr', label: '美漫左翻 (LTR)' },
  { value: 'webtoon', label: '条漫竖滑 (Webtoon)' },
]
</script>

<template>
  <div class="space-y-2.5">
  <div class="grid grid-cols-1 gap-2.5 sm:grid-cols-2">
    <label class="block sm:col-span-2">
      <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">系列 / 部名 (Series，对应一级连载目录)</span>
      <input
        v-model="form.series"
        type="text"
        class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
      />
    </label>
    <label class="block sm:col-span-2">
      <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">总系列 / 系列组 (SeriesGroup，分多部作品选填)</span>
      <input
        v-model="form.seriesGroup"
        type="text"
        placeholder="如：JOJO的奇妙冒险（单部作品留空）"
        class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
      />
    </label>
    <label class="block sm:col-span-2">
      <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">话/卷标题 (Title)</span>
      <input
        v-model="form.title"
        type="text"
        class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
      />
    </label>
    <div class="grid grid-cols-3 gap-2 sm:col-span-2">
      <label class="block">
        <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">话号 (Number)</span>
        <input
          v-model="form.number"
          type="text"
          class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
        />
      </label>
      <label class="block">
        <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">卷号 (Volume)</span>
        <input
          v-model="form.volume"
          type="number"
          class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
        />
      </label>
      <label class="block">
        <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">年份</span>
        <input
          v-model="form.year"
          type="number"
          class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
        />
      </label>
    </div>
    <label class="block">
      <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">原作者 / 编剧 (Writer)</span>
      <input
        v-model="form.writers"
        type="text"
        class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
      />
    </label>
    <label class="block">
      <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">作画 (Penciller)</span>
      <input
        v-model="form.pencillers"
        type="text"
        class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
      />
    </label>
    <label class="block">
      <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">出版社</span>
      <input
        v-model="form.publisher"
        type="text"
        class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
      />
    </label>
    <div class="block">
      <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">阅读方向</span>
      <AppSelect v-model="form.direction" :options="directionOptions" />
    </div>
    <label class="block">
      <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">分类 (Genre)</span>
      <input
        v-model="form.genres"
        type="text"
        class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
      />
    </label>
    <label class="block">
      <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">标签 (Tags)</span>
      <input
        v-model="form.tags"
        type="text"
        class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
      />
    </label>
    <label class="block sm:col-span-2">
      <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">简介 (Summary)</span>
      <textarea
        v-model="form.summary"
        rows="5"
        class="w-full resize-y rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] p-2.5 text-xs leading-relaxed text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
      />
    </label>
  </div>
  </div>
</template>
