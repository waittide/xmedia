<script setup lang="ts">
import { LuChevronRight } from 'vue-icons-plus/lu'
import type { TocTreeNode } from './TocTree.vue'

defineOptions({ name: 'TocTreeItem' })

withDefaults(
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
        class="group flex items-center justify-between gap-2 rounded-corner-md px-2 transition-colors duration-150 ease-standard"
        :class="[
          depth === 0 ? 'py-2' : 'py-1.5',
          subtreeContainsActive(node, activeHref)
            ? 'bg-secondary-container/75 text-on-secondary-container'
            : 'text-on-surface hover:bg-surface-container-high',
        ]"
      >
        <button
          type="button"
          :aria-expanded="expandedKeys.has(node.key)"
          :aria-label="node.item.title"
          class="md-state md-touch-target flex h-9 w-9 shrink-0 cursor-pointer items-center justify-center rounded-corner-sm text-on-surface-variant hover:text-on-surface focus-visible:outline-3 focus-visible:outline-offset-2 focus-visible:outline-secondary"
          @click.stop="emit('toggleExpand', node.key)"
        >
          <LuChevronRight
            class="h-4 w-4 transition-transform duration-150 ease-standard"
            :class="expandedKeys.has(node.key) ? 'rotate-90' : ''"
            aria-hidden="true"
          />
        </button>

        <button
          type="button"
          class="text-title-small min-h-9 min-w-0 flex-1 cursor-pointer truncate rounded-corner-xs px-1 text-left focus-visible:outline-3 focus-visible:outline-offset-2 focus-visible:outline-secondary"
          @click="emit('volumeClick', node)"
        >
          {{ node.item.title }}
        </button>

        <span
          class="text-label-small shrink-0 rounded-corner-xs bg-surface-container-highest px-1.5 py-0.5 tabular-nums text-on-surface-variant"
        >
          {{ node.children.length }}
        </span>
      </div>

      <div
        v-if="expandedKeys.has(node.key)"
        class="mt-0.5 ml-4 space-y-0.5 border-l border-outline-variant/70 pl-2"
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
      class="md-state flex w-full cursor-pointer items-center rounded-corner-md text-left transition-colors duration-150 ease-standard"
      :class="[
        depth === 0 ? 'px-3 py-2.5' : 'px-2.5 py-2',
        isHrefMatch(node.item.href, activeHref)
          ? 'text-title-small bg-secondary-container text-on-secondary-container'
          : depth === 0
            ? 'text-body-medium text-on-surface hover:bg-surface-container-high'
            : 'text-body-small text-on-surface-variant hover:bg-surface-container-high hover:text-on-surface',
      ]"
      @click="emit('select', node.item.href)"
    >
      <span class="truncate">{{ node.item.title }}</span>
    </button>
  </div>
</template>
