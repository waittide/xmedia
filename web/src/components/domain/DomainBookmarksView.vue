<script setup lang="ts" generic="T">
import { useI18n } from 'vue-i18n'
import { LuBookmark, LuPlay, LuSearch, LuTrash2, LuX } from 'vue-icons-plus/lu'
import RefreshButton from '@/components/ui/RefreshButton.vue'

defineProps<{
  title: string
  subtitle: string
  emptyText: string
  totalCount: number
  items: T[]
  itemKey: (item: T) => string
  loading: boolean
  onRefresh: () => Promise<void>
}>()

const emit = defineEmits<{
  open: [item: T]
  delete: [item: T]
}>()

const keyword = defineModel<string>('keyword', { default: '' })
const { t } = useI18n()
</script>

<template>
  <div class="mx-auto max-w-7xl space-y-6 px-6 py-8 sm:px-8">
    <div class="flex flex-wrap items-end justify-between gap-4">
      <div>
        <div class="flex items-center gap-2.5">
          <h1 class="text-2xl font-bold tracking-tight text-[var(--md-sys-color-on-surface)]">
            {{ title }}
          </h1>
          <span
            class="rounded-full bg-[var(--md-sys-color-primary-container)] px-2.5 py-0.5 text-xs font-bold text-[var(--md-sys-color-on-primary-container)]"
          >
            {{ totalCount }}
          </span>
        </div>
        <p class="mt-1 text-xs text-[var(--md-sys-color-on-surface-variant)]">
          {{ subtitle }}
        </p>
      </div>

      <RefreshButton size="md" :action="onRefresh" :busy="loading" />
    </div>

    <div class="md-card-filled flex items-center justify-between gap-3 p-3">
      <div class="relative max-w-md flex-1">
        <LuSearch
          class="pointer-events-none absolute top-1/2 left-3.5 h-4 w-4 -translate-y-1/2 text-[var(--md-sys-color-on-surface-variant)]"
        />
        <input
          v-model="keyword"
          type="text"
          :placeholder="t('common.filterPlaceholder')"
          class="md-input h-9 w-full !pr-8 !pl-9 text-xs placeholder:text-[var(--md-sys-color-on-surface-variant)]"
        />
        <button
          v-if="keyword"
          type="button"
          class="md-icon-btn md-icon-btn-sm md-state absolute top-1/2 right-1 !h-7 !w-7 -translate-y-1/2"
          @click="keyword = ''"
        >
          <LuX class="h-3.5 w-3.5" />
        </button>
      </div>
    </div>

    <div
      v-if="!loading && items.length === 0"
      class="md-card-filled flex flex-col items-center justify-center rounded-3xl px-6 py-16 text-center"
    >
      <div
        class="mb-4 flex h-14 w-14 items-center justify-center rounded-2xl bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)]"
      >
        <LuBookmark class="h-7 w-7" />
      </div>
      <p class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
        {{ emptyText }}
      </p>
    </div>

    <div v-else class="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
      <div
        v-for="item in items"
        :key="itemKey(item)"
        class="group md-card-filled flex items-center gap-4 border border-transparent p-3.5 transition hover:-translate-y-0.5 hover:border-[var(--md-sys-color-primary)]/45 hover:shadow-[var(--md-sys-elevation-1)]"
      >
        <button
          type="button"
          class="relative h-24 w-17 shrink-0 overflow-hidden rounded-xl bg-[var(--md-sys-color-surface-container-high)] shadow-[var(--md-sys-elevation-1)]"
          @click="emit('open', item)"
        >
          <slot name="cover" :item="item" />
        </button>

        <div class="min-w-0 flex-1">
          <slot name="body" :item="item" :open="() => emit('open', item)" />

          <div class="mt-2.5 flex items-center justify-between gap-2">
            <slot name="badge" :item="item" />

            <div class="flex items-center gap-1">
              <button
                type="button"
                class="md-btn md-btn-sm md-btn-filled md-state !h-7 !px-2.5 !text-[11px]"
                @click="emit('open', item)"
              >
                <LuPlay class="h-3 w-3 fill-current" />
                <span>{{ t('bookmarksPage.jumpBtn') }}</span>
              </button>
              <button
                type="button"
                class="md-icon-btn md-icon-btn-sm md-state !h-7 !w-7 hover:bg-[var(--md-sys-color-error-container)]/60 hover:!text-[var(--md-sys-color-error)]"
                :title="t('common.delete')"
                @click="emit('delete', item)"
              >
                <LuTrash2 class="h-3.5 w-3.5" />
              </button>
            </div>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
