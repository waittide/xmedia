<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { MdButton, MdTextField } from '@/components/ui'
import { useBookI18n as useI18n } from '@/domains/book/i18n'
import type { TocItem } from '@/domains/book/types'
import TocTreeItem from './TocTreeItem.vue'

export interface TocTreeNode {
  key: string
  item: TocItem
  children: TocTreeNode[]
}

const props = withDefaults(
  defineProps<{
    items: TocItem[]
    activeHref?: string | null
    searchable?: boolean
  }>(),
  {
    activeHref: null,
    searchable: false,
  },
)

const emit = defineEmits<{
  select: [href: string]
}>()

const { t } = useI18n()

const expandedKeys = ref<Set<string>>(new Set())
const filterQuery = ref('')

const normalizedFilter = computed(() => filterQuery.value.trim().toLowerCase())

const treeRoots = computed<TocTreeNode[]>(() => {
  const roots: TocTreeNode[] = []
  const stack: TocTreeNode[] = []

  for (const item of props.items) {
    const node: TocTreeNode = {
      key: `${item.index}:${item.href}`,
      item,
      children: [],
    }
    while (stack.length > 0 && stack[stack.length - 1]!.item.level >= item.level) {
      stack.pop()
    }
    if (stack.length === 0) {
      roots.push(node)
    } else {
      stack[stack.length - 1]!.children.push(node)
    }
    stack.push(node)
  }

  return roots
})

function filterTreeNodes(nodes: TocTreeNode[], query: string): TocTreeNode[] {
  if (!query) return nodes
  const out: TocTreeNode[] = []
  for (const node of nodes) {
    const titleMatch = node.item.title.toLowerCase().includes(query)
    const filteredChildren = filterTreeNodes(node.children, query)
    if (titleMatch) {
      out.push(node)
    } else if (filteredChildren.length > 0) {
      out.push({ ...node, children: filteredChildren })
    }
  }
  return out
}

const displayedRoots = computed<TocTreeNode[]>(() =>
  filterTreeNodes(treeRoots.value, normalizedFilter.value),
)

const parentKeys = computed<string[]>(() => {
  const out: string[] = []
  const walk = (nodes: TocTreeNode[]) => {
    for (const n of nodes) {
      if (n.children.length > 0) {
        out.push(n.key)
        walk(n.children)
      }
    }
  }
  walk(displayedRoots.value)
  return out
})

const hasHierarchy = computed(() => parentKeys.value.length > 0)

const allExpanded = computed(
  () =>
    parentKeys.value.length > 0 &&
    parentKeys.value.every((k) => expandedKeys.value.has(k)),
)

const effectiveExpandedKeys = computed<Set<string>>(() => {
  if (normalizedFilter.value) {
    return new Set(parentKeys.value)
  }
  return expandedKeys.value
})

function normalizeHrefPath(raw: string): string {
  const withoutHash = raw.split('#')[0] ?? raw
  return withoutHash.replace(/^(\.\/|\/)+/, '')
}

function isHrefMatch(itemHref: string, active: string | null | undefined): boolean {
  if (!active || !itemHref) return false
  if (itemHref === active) return true
  const cleanItem = normalizeHrefPath(itemHref)
  const cleanActive = normalizeHrefPath(active)
  if (!cleanItem || !cleanActive) return false
  return cleanActive === cleanItem || cleanActive.endsWith(`/${cleanItem}`)
}

function subtreeContainsActive(node: TocTreeNode, active: string | null | undefined): boolean {
  if (!active) return false
  for (const child of node.children) {
    if (isHrefMatch(child.item.href, active) || subtreeContainsActive(child, active)) {
      return true
    }
  }
  return false
}

function autoExpandInitialAndActive(): void {
  const next = new Set(expandedKeys.value)
  let matchedAny = false
  const walk = (nodes: TocTreeNode[]) => {
    for (const n of nodes) {
      if (n.children.length > 0) {
        if (subtreeContainsActive(n, props.activeHref)) {
          next.add(n.key)
          matchedAny = true
        }
        walk(n.children)
      }
    }
  }
  walk(treeRoots.value)

  if (!matchedAny && next.size === 0 && parentKeys.value[0]) {
    next.add(parentKeys.value[0])
  }
  expandedKeys.value = next
}

watch(
  () => [props.items, props.activeHref],
  () => {
    autoExpandInitialAndActive()
  },
  { immediate: true },
)

function toggleExpand(key: string): void {
  const next = new Set(expandedKeys.value)
  if (next.has(key)) {
    next.delete(key)
  } else {
    next.add(key)
  }
  expandedKeys.value = next
}

function toggleExpandAll(): void {
  if (allExpanded.value) {
    expandedKeys.value = new Set()
  } else {
    expandedKeys.value = new Set(parentKeys.value)
  }
}

function onVolumeRowClick(node: TocTreeNode): void {
  if (!effectiveExpandedKeys.value.has(node.key)) {
    toggleExpand(node.key)
  } else if (node.item.href) {
    emit('select', node.item.href)
  }
}
</script>

<template>
  <div class="space-y-2.5">
    <p v-if="items.length === 0" class="text-body-small py-6 text-center text-on-surface-variant">
      {{ t('reader.noToc') }}
    </p>

    <template v-else>
      <MdTextField
        v-if="searchable && items.length > 8"
        v-model="filterQuery"
        type="search"
        size="sm"
        clearable
        :placeholder="t('reader.tocSearchPlaceholder')"
      />

      <!-- Expand / Collapse All Controls when Volumes exist -->
      <div
        v-if="hasHierarchy && !normalizedFilter"
        class="flex items-center justify-between px-2"
      >
        <span class="text-label-small text-on-surface-variant">
          {{ t('reader.volumeStructureHint', { volumes: parentKeys.length }) }}
        </span>
        <MdButton variant="text" @click="toggleExpandAll">
          {{ allExpanded ? t('reader.collapseAll') : t('reader.expandAll') }}
        </MdButton>
      </div>

      <p
        v-if="displayedRoots.length === 0"
        class="text-body-small py-6 text-center text-on-surface-variant"
      >
        {{ t('collections.emptyResult') }}
      </p>

      <div v-else class="space-y-0.5">
        <TocTreeItem
          v-for="root in displayedRoots"
          :key="root.key"
          :node="root"
          :depth="0"
          :expanded-keys="effectiveExpandedKeys"
          :active-href="activeHref"
          :is-href-match="isHrefMatch"
          :subtree-contains-active="subtreeContainsActive"
          @select="emit('select', $event)"
          @toggle-expand="toggleExpand"
          @volume-click="onVolumeRowClick"
        />
      </div>
    </template>
  </div>
</template>
