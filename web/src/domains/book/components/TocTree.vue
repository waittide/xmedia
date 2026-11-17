<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useBookI18n as useI18n } from '@/domains/book/i18n'
import { LuChevronRight } from 'vue-icons-plus/lu'
import type { TocItem } from '@/domains/book/types'

export interface TocTreeNode {
  key: string
  item: TocItem
  children: TocTreeNode[]
}

const props = withDefaults(
  defineProps<{
    items: TocItem[]
    activeHref?: string | null
  }>(),
  {
    activeHref: null,
  },
)

const emit = defineEmits<{
  select: [href: string]
}>()

const { t } = useI18n()

const expandedKeys = ref<Set<string>>(new Set())

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
  walk(treeRoots.value)
  return out
})

const hasHierarchy = computed(() => parentKeys.value.length > 0)

const allExpanded = computed(
  () =>
    parentKeys.value.length > 0 &&
    parentKeys.value.every((k) => expandedKeys.value.has(k)),
)

function isHrefMatch(itemHref: string, active: string | null | undefined): boolean {
  if (!active || !itemHref) return false
  if (itemHref === active) return true
  const cleanItem = itemHref.split('#')[0] ?? itemHref
  const cleanActive = active.split('#')[0] ?? active
  return Boolean(cleanItem && cleanActive && cleanActive.endsWith(cleanItem))
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
  // If nothing expanded yet, expand the first volume (or volume containing active chapter)
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
  if (!expandedKeys.value.has(node.key)) {
    toggleExpand(node.key)
  } else if (node.item.href) {
    emit('select', node.item.href)
  }
}
</script>

<template>
  <div>
    <p v-if="items.length === 0" class="py-6 text-center text-xs text-[var(--ctp-subtext0)]">
      {{ t('reader.noToc') }}
    </p>

    <template v-else>
      <!-- Expand / Collapse All Controls when Volumes exist -->
      <div
        v-if="hasHierarchy"
        class="mb-2 flex items-center justify-between px-2 text-[11px] text-[var(--ctp-subtext0)]"
      >
        <span>{{ t('reader.volumeStructureHint', { volumes: parentKeys.length }) }}</span>
        <button
          type="button"
          class="font-medium text-[var(--ctp-accent)] hover:underline"
          @click="toggleExpandAll"
        >
          {{ allExpanded ? t('reader.collapseAll') : t('reader.expandAll') }}
        </button>
      </div>

      <div class="space-y-0.5">
        <div v-for="root in treeRoots" :key="root.key">
          <!-- Volume / Group Node with Children -->
          <template v-if="root.children.length > 0">
            <div
              class="group flex items-center justify-between gap-1.5 rounded-xl px-2 py-2 text-xs font-semibold transition"
              :class="
                subtreeContainsActive(root, activeHref)
                  ? 'bg-[var(--ctp-accent-soft)]/70 text-[var(--ctp-accent)]'
                  : 'text-[var(--ctp-text)] hover:bg-[var(--ctp-mantle)]'
              "
            >
              <button
                type="button"
                class="flex h-5 w-5 shrink-0 items-center justify-center rounded-md text-[var(--ctp-subtext0)] hover:bg-[var(--ctp-surface0)] hover:text-[var(--ctp-text)]"
                @click.stop="toggleExpand(root.key)"
              >
                <LuChevronRight
                  class="h-3.5 w-3.5 transition-transform duration-150"
                  :class="expandedKeys.has(root.key) ? 'rotate-90' : ''"
                />
              </button>

              <button
                type="button"
                class="min-w-0 flex-1 truncate text-left"
                @click="onVolumeRowClick(root)"
              >
                {{ root.item.title }}
              </button>

              <span class="shrink-0 rounded-md bg-[var(--ctp-surface0)]/60 px-1.5 py-0.5 text-[10px] font-medium text-[var(--ctp-subtext1)]">
                {{ root.children.length }}
              </span>
            </div>

            <!-- Nested Chapters under Volume -->
            <div
              v-if="expandedKeys.has(root.key)"
              class="ml-4 mt-0.5 space-y-0.5 border-l border-[var(--ctp-surface0)] pl-2"
            >
              <div v-for="child in root.children" :key="child.key">
                <template v-if="child.children.length > 0">
                  <div
                    class="flex items-center justify-between gap-1.5 rounded-xl px-2 py-1.5 text-xs font-semibold text-[var(--ctp-text)] hover:bg-[var(--ctp-mantle)]"
                  >
                    <button
                      type="button"
                      class="flex h-5 w-5 shrink-0 items-center justify-center rounded-md text-[var(--ctp-subtext0)]"
                      @click.stop="toggleExpand(child.key)"
                    >
                      <LuChevronRight
                        class="h-3.5 w-3.5 transition-transform duration-150"
                        :class="expandedKeys.has(child.key) ? 'rotate-90' : ''"
                      />
                    </button>
                    <button
                      type="button"
                      class="min-w-0 flex-1 truncate text-left"
                      @click="onVolumeRowClick(child)"
                    >
                      {{ child.item.title }}
                    </button>
                    <span class="shrink-0 rounded-md bg-[var(--ctp-surface0)]/60 px-1.5 py-0.5 text-[10px] text-[var(--ctp-subtext1)]">
                      {{ child.children.length }}
                    </span>
                  </div>

                  <div
                    v-if="expandedKeys.has(child.key)"
                    class="ml-4 mt-0.5 space-y-0.5 border-l border-[var(--ctp-surface0)] pl-2"
                  >
                    <button
                      v-for="leaf in child.children"
                      :key="leaf.key"
                      type="button"
                      class="flex w-full items-center rounded-xl px-2.5 py-1.5 text-left text-xs transition"
                      :class="
                        isHrefMatch(leaf.item.href, activeHref)
                          ? 'bg-[var(--ctp-accent-soft)] font-semibold text-[var(--ctp-accent)]'
                          : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-mantle)] hover:text-[var(--ctp-text)]'
                      "
                      @click="emit('select', leaf.item.href)"
                    >
                      <span class="truncate">{{ leaf.item.title }}</span>
                    </button>
                  </div>
                </template>

                <button
                  v-else
                  type="button"
                  class="flex w-full items-center rounded-xl px-2.5 py-1.5 text-left text-xs transition"
                  :class="
                    isHrefMatch(child.item.href, activeHref)
                      ? 'bg-[var(--ctp-accent-soft)] font-semibold text-[var(--ctp-accent)]'
                      : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-mantle)] hover:text-[var(--ctp-text)]'
                  "
                  @click="emit('select', child.item.href)"
                >
                  <span class="truncate">{{ child.item.title }}</span>
                </button>
              </div>
            </div>
          </template>

          <!-- Flat Top-Level Chapter Node -->
          <button
            v-else
            type="button"
            class="flex w-full items-center rounded-xl px-3 py-2 text-left text-xs transition"
            :class="
              isHrefMatch(root.item.href, activeHref)
                ? 'bg-[var(--ctp-accent-soft)] font-semibold text-[var(--ctp-accent)]'
                : 'text-[var(--ctp-text)] hover:bg-[var(--ctp-mantle)]'
            "
            @click="emit('select', root.item.href)"
          >
            <span class="truncate">{{ root.item.title }}</span>
          </button>
        </div>
      </div>
    </template>
  </div>
</template>
