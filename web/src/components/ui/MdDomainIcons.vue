<script setup lang="ts">
import type { MediaKind } from '@/types/api'

withDefaults(
  defineProps<{
    kind: MediaKind
    active?: boolean
  }>(),
  {
    active: false,
  },
)
</script>

<template>
  <!-- Book: Material 3 auto_stories / open book (24dp filled vs outlined) -->
  <svg
    v-if="kind === 'book'"
    viewBox="0 0 24 24"
    fill="currentColor"
    aria-hidden="true"
  >
    <path
      v-if="active"
      d="M19 2H14C12.95 2 12.02 2.42 11.35 3.1C11.15 3.31 10.85 3.31 10.65 3.1C9.98 2.42 9.05 2 8 2H3C1.9 2 1 2.9 1 4V18C1 19.1 1.9 20 3 20H8.5C9.58 20 10.52 20.54 11 21.35C11.48 20.54 12.42 20 13.5 20H19C20.1 20 21 19.1 21 18V4C21 2.9 20.1 2 19 2ZM10 17.65C9.39 17.24 8.67 17 7.9 17H3V4H8C9.1 4 10 4.9 10 6V17.65Z"
    />
    <path
      v-else
      d="M19 2H14C12.8 2 11.73 2.54 11 3.38C10.27 2.54 9.2 2 8 2H3C1.9 2 1 2.9 1 4V18C1 19.1 1.9 20 3 20H8.5C9.58 20 10.52 20.54 11 21.35C11.48 20.54 12.42 20 13.5 20H19C20.1 20 21 19.1 21 18V4C21 2.9 20.1 2 19 2ZM3 18V4H8C9.1 4 10 4.9 10 6V18.35C9.53 18.13 9.03 18 8.5 18H3ZM19 18H13.5C12.97 18 12.47 18.13 12 18.35V6C12 4.9 12.9 4 14 4H19V18Z"
    />
  </svg>

  <!-- Comic: Material 3 layers / stacked volumes (24dp filled vs outlined) -->
  <svg
    v-else-if="kind === 'comic'"
    viewBox="0 0 24 24"
    fill="currentColor"
    aria-hidden="true"
  >
    <path
      v-if="active"
      d="M11.99 2L3 9L11.99 16L21 9L11.99 2ZM11.99 18.54L4.62 12.81L3 14.07L12 21.07L21 14.07L19.37 12.8L11.99 18.54Z"
    />
    <path
      v-else
      d="M11.99 18.54L4.62 12.81L3 14.07L12 21.07L21 14.07L19.37 12.8L11.99 18.54ZM12 16L19.36 10.27L21 9L12 2L3 9L4.63 10.27L12 16ZM12 4.53L17.74 9L12 13.47L6.26 9L12 4.53Z"
    />
  </svg>

  <!-- Music: Material 3 album / vinyl disc (24dp filled vs outlined) -->
  <svg
    v-else-if="kind === 'music'"
    viewBox="0 0 24 24"
    fill="currentColor"
    aria-hidden="true"
  >
    <path
      v-if="active"
      fill-rule="evenodd"
      clip-rule="evenodd"
      d="M12 2C6.48 2 2 6.48 2 12C2 17.52 6.48 22 12 22C17.52 22 22 17.52 22 12C22 6.48 17.52 2 12 2ZM12 16.5C9.51 16.5 7.5 14.49 7.5 12C7.5 9.51 9.51 7.5 12 7.5C14.49 7.5 16.5 9.51 16.5 12C16.5 14.49 14.49 16.5 12 16.5ZM12 13.25C12.69 13.25 13.25 12.69 13.25 12C13.25 11.31 12.69 10.75 12 10.75C11.31 10.75 10.75 11.31 10.75 12C10.75 12.69 11.31 13.25 12 13.25Z"
    />
    <path
      v-else
      d="M12 2C6.48 2 2 6.48 2 12C2 17.52 6.48 22 12 22C17.52 22 22 17.52 22 12C22 6.48 17.52 2 12 2ZM12 20C7.59 20 4 16.41 4 12C4 7.59 7.59 4 12 4C16.41 4 20 7.59 20 12C20 16.41 16.41 20 12 20ZM12 7.5C9.51 7.5 7.5 9.51 7.5 12C7.5 14.49 9.51 16.5 12 16.5C14.49 16.5 16.5 14.49 16.5 12C16.5 9.51 14.49 7.5 12 7.5ZM12 13.25C11.31 13.25 10.75 12.69 10.75 12C10.75 11.31 11.31 10.75 12 10.75C12.69 10.75 13.25 11.31 13.25 12C13.25 12.69 12.69 13.25 12 13.25Z"
    />
  </svg>

  <!-- Video: Material 3 movie / clapperboard (24dp filled vs outlined) -->
  <svg
    v-else
    viewBox="0 0 24 24"
    fill="currentColor"
    aria-hidden="true"
  >
    <path
      v-if="active"
      d="M18 4L20 8H17L15 4H13L15 8H12L10 4H8L10 8H7L5 4H4C2.9 4 2.01 4.9 2.01 6L2 18C2 19.1 2.9 20 4 20H20C21.1 20 22 19.1 22 18V4H18Z"
    />
    <path
      v-else
      d="M18 4L20 8H17L15 4H13L15 8H12L10 4H8L10 8H7L5 4H4C2.9 4 2.01 4.9 2.01 6L2 18C2 19.1 2.9 20 4 20H20C21.1 20 22 19.1 22 18V4H18ZM20 18H4V10H20V18Z"
    />
  </svg>
</template>
