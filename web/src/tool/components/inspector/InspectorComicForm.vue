<script setup lang="ts">
import MdSelect from '@/components/ui/MdSelect.vue'
import type { ComicInspectorForm } from './types'

defineProps<{
  form: ComicInspectorForm
}>()

const directionOptions = [
  { value: 'rtl', label: '日漫右翻 (RTL)' },
  { value: 'ltr', label: '美漫左翻 (LTR)' },
  { value: 'webtoon', label: '条漫竖滑 (Webtoon)' },
]

const inputCls =
  'w-full rounded-xl border border-[var(--md-sys-color-outline-variant)]/70 bg-[var(--md-sys-color-surface)] px-3 py-2 text-xs text-[var(--md-sys-color-on-surface)] outline-none transition focus:border-[var(--md-sys-color-primary)]'
</script>

<template>
  <div class="space-y-2.5">
    <div class="grid grid-cols-1 gap-2.5 sm:grid-cols-2">
      <label class="block sm:col-span-2">
        <span class="mb-1 block text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">系列 / 部名 (Series，对应一级连载目录)</span>
        <input v-model="form.series" type="text" :class="inputCls" />
      </label>
      <label class="block sm:col-span-2">
        <span class="mb-1 block text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">总系列 / 系列组 (SeriesGroup，分多部作品选填)</span>
        <input
          v-model="form.seriesGroup"
          type="text"
          placeholder="如：JOJO的奇妙冒险（单部作品留空）"
          :class="inputCls"
        />
      </label>
      <label class="block sm:col-span-2">
        <span class="mb-1 block text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">话/卷标题 (Title)</span>
        <input v-model="form.title" type="text" :class="inputCls" />
      </label>
      <div class="grid grid-cols-3 gap-2 sm:col-span-2">
        <label class="block">
          <span class="mb-1 block text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">话号 (Number)</span>
          <input v-model="form.number" type="text" :class="inputCls" />
        </label>
        <label class="block">
          <span class="mb-1 block text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">卷号 (Volume)</span>
          <input v-model="form.volume" type="number" :class="inputCls" />
        </label>
        <label class="block">
          <span class="mb-1 block text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">年份</span>
          <input v-model="form.year" type="number" :class="inputCls" />
        </label>
      </div>
      <label class="block">
        <span class="mb-1 block text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">原作者 / 编剧 (Writer)</span>
        <input v-model="form.writers" type="text" :class="inputCls" />
      </label>
      <label class="block">
        <span class="mb-1 block text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">作画 (Penciller)</span>
        <input v-model="form.pencillers" type="text" :class="inputCls" />
      </label>
      <label class="block">
        <span class="mb-1 block text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">出版社</span>
        <input v-model="form.publisher" type="text" :class="inputCls" />
      </label>
      <label class="block">
        <span class="mb-1 block text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">语言 (LanguageISO)</span>
        <input v-model="form.language" type="text" placeholder="如：zh-CN / ja / en" :class="inputCls" />
      </label>
      <div class="block sm:col-span-2">
        <span class="mb-1 block text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">阅读方向</span>
        <MdSelect v-model="form.direction" :options="directionOptions" />
      </div>
      <label class="block">
        <span class="mb-1 block text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">分类 (Genre)</span>
        <input v-model="form.genres" type="text" :class="inputCls" />
      </label>
      <label class="block">
        <span class="mb-1 block text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">标签 (Tags)</span>
        <input v-model="form.tags" type="text" :class="inputCls" />
      </label>
      <label class="block sm:col-span-2">
        <span class="mb-1 block text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">简介 (Summary)</span>
        <textarea
          v-model="form.summary"
          rows="5"
          class="w-full resize-y rounded-xl border border-[var(--md-sys-color-outline-variant)]/70 bg-[var(--md-sys-color-surface)] p-2.5 text-xs leading-relaxed text-[var(--md-sys-color-on-surface)] outline-none transition focus:border-[var(--md-sys-color-primary)]"
        />
      </label>
    </div>
  </div>
</template>
