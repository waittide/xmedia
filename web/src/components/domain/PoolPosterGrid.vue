<script setup lang="ts">
import type { Component } from 'vue'
import { useI18n } from 'vue-i18n'
import { LuCheck, LuExternalLink, LuPlus, LuX } from 'vue-icons-plus/lu'
import { MdButton, MdIconButton, MediaCoverImage } from '@/components/ui'
import type { PoolPosterCardVM } from '@/sources'

defineProps<{
  items: readonly PoolPosterCardVM[]
  gridClass: string
  fallbackIcon: Component
  isTargetingSelf: boolean
  busyIds: ReadonlySet<string>
}>()

const emit = defineEmits<{
  bind: [id: string]
  unbind: [id: string]
  open: [id: string, kind?: 'movie' | 'show']
}>()

const { t } = useI18n()
</script>

<template>
  <div :class="gridClass">
    <article v-for="item in items" :key="item.id" class="group flex flex-col gap-2">
      <div class="relative overflow-hidden rounded-corner-md bg-surface-container-low">
        <MediaCoverImage
          :src="item.coverUrl"
          :alt="item.title"
          aspect-class="aspect-[2/3]"
          rounded="rounded-corner-md"
          :fallback-icon="fallbackIcon"
        />
        <span
          v-if="isTargetingSelf && item.bound"
          class="text-label-small absolute top-2 left-2 inline-flex items-center gap-1 rounded-corner-full bg-primary px-2 py-0.5 text-on-primary shadow-elevation-1"
        >
          <LuCheck class="h-3 w-3" aria-hidden="true" />
          {{ t('pool.boundLabel') }}
        </span>
        <span
          v-if="item.badgeText"
          class="text-label-small absolute right-2 bottom-2 rounded-corner-xs bg-surface-container-highest/90 px-1.5 py-0.5 uppercase text-on-surface backdrop-blur-xs"
        >
          {{ item.badgeText }}
        </span>
      </div>

      <div class="min-w-0 space-y-0.5 px-0.5">
        <p class="text-title-small truncate text-on-surface" :title="item.title">
          {{ item.title }}
        </p>
        <p class="text-body-small truncate text-on-surface-variant">
          {{ item.subtitle }}
        </p>
      </div>

      <div class="mt-auto flex items-center gap-1.5 pt-0.5">
        <template v-if="isTargetingSelf && item.bound">
          <MdButton
            variant="tonal"
            class="flex-1"
            :icon="LuExternalLink"
            @click="emit('open', item.id, item.openKind)"
          >
            {{ t('pool.openLocalBtn') }}
          </MdButton>
          <MdIconButton
            variant="standard"
            :icon="LuX"
            :loading="busyIds.has(item.id)"
            :tooltip="t('pool.unbindBtn')"
            :aria-label="t('pool.unbindBtn')"
            @click="emit('unbind', item.id)"
          />
        </template>
        <MdButton
          v-else
          variant="filled"
          class="w-full"
          :icon="LuPlus"
          :loading="busyIds.has(item.id)"
          @click="emit('bind', item.id)"
        >
          {{ t('pool.bindBtn') }}
        </MdButton>
      </div>
    </article>
  </div>
</template>
