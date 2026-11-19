<script setup lang="ts">
import { LuFileText, LuLock, LuUnlock } from 'vue-icons-plus/lu'
import type { BookInspectorForm } from './types'

const props = defineProps<{
  form: BookInspectorForm
  bookFormat: string
  writable: boolean
  isEpub: boolean
  hasCover: boolean
  polishingBook: boolean
}>()

const emit = defineEmits<{
  polish: []
}>()

function isBookFieldLocked(field: string): boolean {
  return props.form.lockedFields.includes(field)
}

function toggleBookFieldLock(field: string): void {
  if (props.form.lockedFields.includes(field)) {
    props.form.lockedFields = props.form.lockedFields.filter((f) => f !== field)
  } else {
    props.form.lockedFields = [...props.form.lockedFields, field]
  }
}

function lockFilledBookFields(): void {
  const next = new Set(props.form.lockedFields)
  if (props.form.title.trim()) next.add('title')
  if (props.form.authors.trim()) next.add('authors')
  if (props.form.series.trim()) next.add('series')
  if (props.form.subseries.trim()) next.add('subseries')
  if (props.form.seriesIndex.trim()) next.add('series_index')
  if (props.form.isbn.trim()) next.add('isbn')
  if (props.form.publisher.trim()) next.add('publisher')
  if (props.form.publishedAt.trim()) next.add('published_at')
  if (props.form.language.trim()) next.add('language')
  if (props.form.tags.trim()) next.add('tags')
  if (props.form.description.trim()) next.add('description')
  if (props.hasCover) next.add('cover')
  props.form.lockedFields = Array.from(next)
}
</script>

<template>
  <div class="space-y-2.5">
  <div
    v-if="bookFormat !== 'epub'"
    class="flex items-center gap-2 rounded-xl border border-[var(--ctp-accent)]/35 bg-[var(--ctp-accent-soft)]/50 px-3 py-2 text-xs text-[var(--ctp-accent)]"
  >
    <LuFileText class="h-4 w-4 shrink-0" />
    <span>当前为 {{ bookFormat.toUpperCase() }} 文件，单书目录按 Calibre 规范写入 metadata.opf / cover，多书混放目录写入同名旁路文件，归档移动自动同步。</span>
  </div>

  <div
    v-if="writable"
    class="flex flex-wrap items-center justify-between gap-2 rounded-xl border border-[var(--ctp-surface0)]/80 bg-[var(--ctp-mantle)]/55 px-3 py-1.5 text-[11px]"
  >
    <span class="flex items-center gap-1.5 text-[var(--ctp-subtext1)]">
      <LuLock class="h-3.5 w-3.5 text-amber-500" />
      <span>
        防刮削字段锁：已锁
        <strong class="text-[var(--ctp-text)]">{{ form.lockedFields.length }}</strong>
        项（保存后单本/批量刮削均跳过被锁字段）
      </span>
    </span>
    <div class="flex items-center gap-1.5">
      <button
        type="button"
        class="rounded-md px-2 py-0.5 font-medium text-[var(--ctp-accent)] transition hover:bg-[var(--ctp-accent-soft)]"
        @click="lockFilledBookFields"
      >
        锁非空项
      </button>
      <button
        v-if="form.lockedFields.length > 0"
        type="button"
        class="rounded-md px-2 py-0.5 font-medium text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)]/70 hover:text-[var(--ctp-text)]"
        @click="form.lockedFields = []"
      >
        全部解锁
      </button>
    </div>
  </div>

  <div class="grid grid-cols-1 gap-2.5 sm:grid-cols-2">
    <div class="block sm:col-span-2">
      <div class="mb-1 flex items-center justify-between">
        <span class="text-[11px] font-medium text-[var(--ctp-subtext0)]">书名 (dc:title)</span>
        <button
          v-if="writable"
          type="button"
          class="inline-flex items-center gap-0.5 rounded px-1 py-0.5 text-[10px] transition"
          :class="isBookFieldLocked('title') ? 'bg-amber-500/15 font-semibold text-amber-500' : 'text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]'"
          title="锁定/解锁书名字段"
          @click="toggleBookFieldLock('title')"
        >
          <LuLock v-if="isBookFieldLocked('title')" class="h-3 w-3" />
          <LuUnlock v-else class="h-3 w-3" />
        </button>
      </div>
      <input
        v-model="form.title"
        :disabled="!writable"
        type="text"
        class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)] disabled:opacity-55"
      />
    </div>
    <div class="block sm:col-span-2">
      <div class="mb-1 flex items-center justify-between">
        <span class="text-[11px] font-medium text-[var(--ctp-subtext0)]">作者 (dc:creator，`/` 分隔)</span>
        <button
          v-if="writable"
          type="button"
          class="inline-flex items-center gap-0.5 rounded px-1 py-0.5 text-[10px] transition"
          :class="isBookFieldLocked('authors') ? 'bg-amber-500/15 font-semibold text-amber-500' : 'text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]'"
          title="锁定/解锁作者字段"
          @click="toggleBookFieldLock('authors')"
        >
          <LuLock v-if="isBookFieldLocked('authors')" class="h-3 w-3" />
          <LuUnlock v-else class="h-3 w-3" />
        </button>
      </div>
      <input
        v-model="form.authors"
        :disabled="!writable"
        type="text"
        class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)] disabled:opacity-55"
      />
    </div>
    <div class="grid grid-cols-1 gap-2 sm:col-span-2 sm:grid-cols-12">
      <div class="block sm:col-span-5">
        <div class="mb-1 flex items-center justify-between">
          <span class="text-[11px] font-medium text-[var(--ctp-subtext0)]">系列 / 丛书 (Series)</span>
          <button
            v-if="writable"
            type="button"
            class="inline-flex items-center gap-0.5 rounded px-1 py-0.5 text-[10px] transition"
            :class="isBookFieldLocked('series') ? 'bg-amber-500/15 font-semibold text-amber-500' : 'text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]'"
            title="锁定/解锁系列字段"
            @click="toggleBookFieldLock('series')"
          >
            <LuLock v-if="isBookFieldLocked('series')" class="h-3 w-3" />
            <LuUnlock v-else class="h-3 w-3" />
          </button>
        </div>
        <input
          v-model="form.series"
          :disabled="!writable"
          type="text"
          placeholder="如：三体 / 魔法禁书目录"
          class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)] disabled:opacity-55"
        />
      </div>
      <div class="block sm:col-span-4">
        <div class="mb-1 flex items-center justify-between">
          <span class="text-[11px] font-medium text-[var(--ctp-subtext0)]">部 / 子系列 (Subseries)</span>
          <button
            v-if="writable"
            type="button"
            class="inline-flex items-center gap-0.5 rounded px-1 py-0.5 text-[10px] transition"
            :class="isBookFieldLocked('subseries') ? 'bg-amber-500/15 font-semibold text-amber-500' : 'text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]'"
            title="锁定/解锁子系列字段"
            @click="toggleBookFieldLock('subseries')"
          >
            <LuLock v-if="isBookFieldLocked('subseries')" class="h-3 w-3" />
            <LuUnlock v-else class="h-3 w-3" />
          </button>
        </div>
        <input
          v-model="form.subseries"
          :disabled="!writable"
          type="text"
          placeholder="选填，如：新约"
          class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)] disabled:opacity-55"
        />
      </div>
      <div class="block sm:col-span-3">
        <div class="mb-1 flex items-center justify-between">
          <span class="text-[11px] font-medium text-[var(--ctp-subtext0)]">卷 / 册号 (Volume)</span>
          <button
            v-if="writable"
            type="button"
            class="inline-flex items-center gap-0.5 rounded px-1 py-0.5 text-[10px] transition"
            :class="isBookFieldLocked('series_index') ? 'bg-amber-500/15 font-semibold text-amber-500' : 'text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]'"
            title="锁定/解锁卷册号字段"
            @click="toggleBookFieldLock('series_index')"
          >
            <LuLock v-if="isBookFieldLocked('series_index')" class="h-3 w-3" />
            <LuUnlock v-else class="h-3 w-3" />
          </button>
        </div>
        <input
          v-model="form.seriesIndex"
          :disabled="!writable"
          type="text"
          placeholder="如：1 / 2.5"
          class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)] disabled:opacity-55"
        />
      </div>
    </div>
    <div class="block">
      <div class="mb-1 flex items-center justify-between">
        <span class="text-[11px] font-medium text-[var(--ctp-subtext0)]">ISBN</span>
        <button
          v-if="writable"
          type="button"
          class="inline-flex items-center gap-0.5 rounded px-1 py-0.5 text-[10px] transition"
          :class="isBookFieldLocked('isbn') ? 'bg-amber-500/15 font-semibold text-amber-500' : 'text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]'"
          title="锁定/解锁 ISBN"
          @click="toggleBookFieldLock('isbn')"
        >
          <LuLock v-if="isBookFieldLocked('isbn')" class="h-3 w-3" />
          <LuUnlock v-else class="h-3 w-3" />
        </button>
      </div>
      <input
        v-model="form.isbn"
        :disabled="!writable"
        type="text"
        class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)] disabled:opacity-55"
      />
    </div>
    <div class="block">
      <div class="mb-1 flex items-center justify-between">
        <span class="text-[11px] font-medium text-[var(--ctp-subtext0)]">出版社 (dc:publisher)</span>
        <button
          v-if="writable"
          type="button"
          class="inline-flex items-center gap-0.5 rounded px-1 py-0.5 text-[10px] transition"
          :class="isBookFieldLocked('publisher') ? 'bg-amber-500/15 font-semibold text-amber-500' : 'text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]'"
          title="锁定/解锁出版社"
          @click="toggleBookFieldLock('publisher')"
        >
          <LuLock v-if="isBookFieldLocked('publisher')" class="h-3 w-3" />
          <LuUnlock v-else class="h-3 w-3" />
        </button>
      </div>
      <input
        v-model="form.publisher"
        :disabled="!writable"
        type="text"
        class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)] disabled:opacity-55"
      />
    </div>
    <div class="block">
      <div class="mb-1 flex items-center justify-between">
        <span class="text-[11px] font-medium text-[var(--ctp-subtext0)]">出版日期 (dc:date)</span>
        <button
          v-if="writable"
          type="button"
          class="inline-flex items-center gap-0.5 rounded px-1 py-0.5 text-[10px] transition"
          :class="isBookFieldLocked('published_at') ? 'bg-amber-500/15 font-semibold text-amber-500' : 'text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]'"
          title="锁定/解锁出版日期"
          @click="toggleBookFieldLock('published_at')"
        >
          <LuLock v-if="isBookFieldLocked('published_at')" class="h-3 w-3" />
          <LuUnlock v-else class="h-3 w-3" />
        </button>
      </div>
      <input
        v-model="form.publishedAt"
        :disabled="!writable"
        type="text"
        placeholder="YYYY-MM-DD"
        class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)] disabled:opacity-55"
      />
    </div>
    <div class="block">
      <div class="mb-1 flex items-center justify-between">
        <span class="text-[11px] font-medium text-[var(--ctp-subtext0)]">语言 (dc:language)</span>
        <button
          v-if="writable"
          type="button"
          class="inline-flex items-center gap-0.5 rounded px-1 py-0.5 text-[10px] transition"
          :class="isBookFieldLocked('language') ? 'bg-amber-500/15 font-semibold text-amber-500' : 'text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]'"
          title="锁定/解锁语言"
          @click="toggleBookFieldLock('language')"
        >
          <LuLock v-if="isBookFieldLocked('language')" class="h-3 w-3" />
          <LuUnlock v-else class="h-3 w-3" />
        </button>
      </div>
      <input
        v-model="form.language"
        :disabled="!writable"
        type="text"
        placeholder="zh / en / ja"
        class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)] disabled:opacity-55"
      />
    </div>
    <div class="block sm:col-span-2">
      <div class="mb-1 flex items-center justify-between">
        <span class="text-[11px] font-medium text-[var(--ctp-subtext0)]">标签 / 主题 (dc:subject，`/` 分隔)</span>
        <button
          v-if="writable"
          type="button"
          class="inline-flex items-center gap-0.5 rounded px-1 py-0.5 text-[10px] transition"
          :class="isBookFieldLocked('tags') ? 'bg-amber-500/15 font-semibold text-amber-500' : 'text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]'"
          title="锁定/解锁标签"
          @click="toggleBookFieldLock('tags')"
        >
          <LuLock v-if="isBookFieldLocked('tags')" class="h-3 w-3" />
          <LuUnlock v-else class="h-3 w-3" />
        </button>
      </div>
      <input
        v-model="form.tags"
        :disabled="!writable"
        type="text"
        class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)] disabled:opacity-55"
      />
    </div>
    <div class="block sm:col-span-2">
      <div class="mb-1 flex items-center justify-between">
        <span class="text-[11px] font-medium text-[var(--ctp-subtext0)]">内容简介 (dc:description)</span>
        <button
          v-if="writable"
          type="button"
          class="inline-flex items-center gap-0.5 rounded px-1 py-0.5 text-[10px] transition"
          :class="isBookFieldLocked('description') ? 'bg-amber-500/15 font-semibold text-amber-500' : 'text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]'"
          title="锁定/解锁内容简介"
          @click="toggleBookFieldLock('description')"
        >
          <LuLock v-if="isBookFieldLocked('description')" class="h-3 w-3" />
          <LuUnlock v-else class="h-3 w-3" />
        </button>
      </div>
      <textarea
        v-model="form.description"
        :disabled="!writable"
        rows="5"
        class="w-full resize-y rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] p-2.5 text-xs leading-relaxed text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)] disabled:opacity-55"
      />
    </div>
  </div>
  </div>
</template>
