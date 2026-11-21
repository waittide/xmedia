<script setup lang="ts">
import { LuChevronRight } from 'vue-icons-plus/lu'
import type { TocTreeNode } from './TocTree.vue'

defineOptions({ name: 'TocTreeItem' })

const props = withDefaults(
  defineProps<{
    node: TocTreeNode
    depth?: number
    expandedKeys: Set<string>
    activeHref?: string | null
    isHrefMatch: (itemHref: string, active: string | null | undefined) => boolean
    subtreeContainsActive: (node: TocTreeNode, active: string | null | undefined) => boolean
  }>(),
  {
    depth: 0,
    activeHref: null,
  },
)

const emit = defineEmits<{
  select: [href: string]
  toggleExpand: [key: string]
  volumeClick: [node: TocTreeNode]
}>()
</script>

<template>
  <div>
    <!-- Group / Volume Node with Children -->
    <template v-if="node.children.length > 0">
      <div
        class="group flex items-center justify-between gap-1.5 rounded-xl px-2 text-xs font-semibold transition"
        :class="[
          depth === 0 ? 'py-2' : 'py-1.5',
          subtreeContainsActive(node, activeHref)
            ? 'bg-[var(--md-sys-color-secondary-container)]/75 text-[var(--md-sys-color-on-secondary-container)]'
            : 'text-[var(--md-sys-color-on-surface)] hover:bg-[var(--md-sys-color-surface-container-high)]',
        ]"
      >
        <button
          type="button"
          class="flex h-5 w-5 shrink-0 items-center justify-center rounded-md text-[var(--md-sys-color-on-surface-variant)] hover:bg-[var(--md-sys-color-surface-container-highest)] hover:text-[var(--md-sys-color-on-surface)]"
          @click.stop="emit('toggleExpand', node.key)"
        >
          <LuChevronRight
            class="h-3.5 w-3.5 transition-transform duration-150"
            :class="expandedKeys.has(node.key) ? 'rotate-90' : ''"
          />
        </button>

        <button
          type="button"
          class="min-w-0 flex-1 truncate text-left"
          @click="emit('volumeClick', node)"
        >
          {{ node.item.title }}
        </button>

        <span class="shrink-0 rounded-md bg-[var(--md-sys-color-surface-container-highest)] px-1.5 py-0.5 text-[10px] font-medium text-[var(--md-sys-color-on-surface-variant)]">
          {{ node.children.length }}
        </span>
      </div>

      <div
        v-if="expandedKeys.has(node.key)"
        class="ml-4 mt-0.5 space-y-0.5 border-l border-[var(--md-sys-color-outline-variant)]/70 pl-2"
      >
        <TocTreeItem
          v-for="child in node.children"
          :key="child.key"
          :node="child"
          :depth="depth + 1"
          :expanded-keys="expandedKeys"
          :active-href="activeHref"
          :is-href-match="isHrefMatch"
          :subtree-contains-active="subtreeContainsActive"
          @select="emit('select', $event)"
          @toggle-expand="emit('toggleExpand', $event)"
          @volume-click="emit('volumeClick', $event)"
        />
      </div>
    </template>

    <!-- Leaf Chapter Node -->
    <button
      v-else
      type="button"
      class="flex w-full items-center rounded-xl text-left text-xs transition"
      :class="[
        depth === 0 ? 'px-3 py-2' : 'px-2.5 py-1.5',
        isHrefMatch(node.item.href, activeHref)
          ? 'bg-[var(--md-sys-color-secondary-container)] font-semibold text-[var(--md-sys-color-on-secondary-container)]'
          : depth === 0
            ? 'text-[var(--md-sys-color-on-surface)] hover:bg-[var(--md-sys-color-surface-container-high)]'
            : 'text-[var(--md-sys-color-on-surface-variant)] hover:bg-[var(--md-sys-color-surface-container-high)] hover:text-[var(--md-sys-color-on-surface)]',
      ]"
      @click="emit('select', node.item.href)"
    >
      <span class="truncate">{{ node.item.title }}</span>
    </button>
  </div>
</template>
