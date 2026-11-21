<script setup lang="ts">
import {
  LuArrowDownToLine,
  LuArrowUpToLine,
  LuDownload,
  LuFlaskConical,
  LuKeyRound,
  LuSearch,
  LuTrash2,
} from 'vue-icons-plus/lu'
import { onlineBookApi, type BookSourceSummaryDto } from '../../onlineApi'

defineProps<{
  source: BookSourceSummaryDto | null
  pos: { top: number; left: number }
}>()

const emit = defineEmits<{
  close: []
  pin: [source: BookSourceSummaryDto, position: 'top' | 'bottom']
  login: [source: BookSourceSummaryDto]
  search: [source: BookSourceSummaryDto]
  debug: [source: BookSourceSummaryDto]
  delete: [source: BookSourceSummaryDto]
}>()
</script>

<template>
  <Teleport to="body">
    <div
      v-if="source"
      class="fixed inset-0 z-[var(--z-popover,60)]"
      @mousedown="emit('close')"
    >
      <div
        role="menu"
        class="fixed w-44 space-y-0.5 rounded-2xl bg-[var(--md-sys-color-surface-container-high)] p-1.5 text-[var(--md-sys-color-on-surface)] shadow-[var(--md-sys-elevation-3)] ring-1 ring-[var(--md-sys-color-outline-variant)]/40"
        :style="{ top: `${pos.top}px`, left: `${pos.left}px` }"
        @mousedown.stop
      >
        <button
          type="button"
          role="menuitem"
          class="md-state flex w-full items-center gap-2.5 rounded-xl px-3 py-2 text-left text-xs font-medium"
          @click="emit('pin', source, 'top')"
        >
          <LuArrowUpToLine class="h-4 w-4 text-[var(--md-sys-color-on-surface-variant)]" />
          <span>置顶</span>
        </button>

        <button
          type="button"
          role="menuitem"
          class="md-state flex w-full items-center gap-2.5 rounded-xl px-3 py-2 text-left text-xs font-medium"
          @click="emit('pin', source, 'bottom')"
        >
          <LuArrowDownToLine class="h-4 w-4 text-[var(--md-sys-color-on-surface-variant)]" />
          <span>置底</span>
        </button>

        <button
          v-if="source.has_login_url || source.login_ui.length > 0"
          type="button"
          role="menuitem"
          class="md-state flex w-full items-center gap-2.5 rounded-xl px-3 py-2 text-left text-xs font-medium"
          @click="emit('login', source)"
        >
          <LuKeyRound class="h-4 w-4 text-[var(--md-sys-color-on-surface-variant)]" />
          <span>登录</span>
        </button>

        <button
          type="button"
          role="menuitem"
          class="md-state flex w-full items-center gap-2.5 rounded-xl px-3 py-2 text-left text-xs font-medium"
          @click="emit('search', source)"
        >
          <LuSearch class="h-4 w-4 text-[var(--md-sys-color-on-surface-variant)]" />
          <span>搜索</span>
        </button>

        <button
          type="button"
          role="menuitem"
          class="md-state flex w-full items-center gap-2.5 rounded-xl px-3 py-2 text-left text-xs font-medium"
          @click="emit('debug', source)"
        >
          <LuFlaskConical class="h-4 w-4 text-[var(--md-sys-color-on-surface-variant)]" />
          <span>调试</span>
        </button>

        <a
          role="menuitem"
          :href="onlineBookApi.exportSourceUrl(source.id)"
          :download="`${source.name}.json`"
          class="md-state flex w-full items-center gap-2.5 rounded-xl px-3 py-2 text-left text-xs font-medium"
          @click="emit('close')"
        >
          <LuDownload class="h-4 w-4 text-[var(--md-sys-color-on-surface-variant)]" />
          <span>导出</span>
        </a>

        <div class="my-1 h-px bg-[var(--md-sys-color-outline-variant)]/50" />

        <button
          type="button"
          role="menuitem"
          class="md-state flex w-full items-center gap-2.5 rounded-xl px-3 py-2 text-left text-xs font-medium text-[var(--md-sys-color-error)]"
          @click="emit('delete', source)"
        >
          <LuTrash2 class="h-4 w-4" />
          <span>删除</span>
        </button>
      </div>
    </div>
  </Teleport>
</template>
