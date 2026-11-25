<script setup lang="ts">
import { ref, type Component } from 'vue'
import { LuUpload } from 'vue-icons-plus/lu'
import { MdButton, MediaCoverImage } from '@/components/ui'

withDefaults(
  defineProps<{
    previewSrc: string | null
    alt: string
    aspectClass: string
    widthClass?: string
    fallbackIcon: Component
    buttonLabel: string
    pendingFileName?: string | null
  }>(),
  {
    widthClass: 'w-20',
    pendingFileName: null,
  },
)

const emit = defineEmits<{
  'select-file': [file: File]
}>()

const inputRef = ref<HTMLInputElement | null>(null)

function handleChange(ev: Event): void {
  const input = ev.target as HTMLInputElement
  const file = input.files?.[0]
  input.value = ''
  if (file) {
    emit('select-file', file)
  }
}
</script>

<template>
  <div class="flex items-center gap-4">
    <div :class="[widthClass, 'shrink-0 overflow-hidden rounded-corner-sm bg-surface-container-high']">
      <MediaCoverImage
        :src="previewSrc"
        :alt="alt"
        :aspect-class="aspectClass"
        rounded="rounded-corner-sm"
        :fallback-icon="fallbackIcon"
        fallback-icon-class="h-6 w-6 text-on-surface-variant/50"
      />
    </div>
    <div class="min-w-0 space-y-1.5">
      <input
        ref="inputRef"
        type="file"
        accept="image/*"
        class="hidden"
        @change="handleChange"
      />
      <MdButton
        variant="tonal"
        :icon="LuUpload"
        @click="inputRef?.click()"
      >
        {{ buttonLabel }}
      </MdButton>
      <p v-if="pendingFileName" class="text-body-small truncate text-on-surface-variant">
        {{ pendingFileName }}
      </p>
    </div>
  </div>
</template>
