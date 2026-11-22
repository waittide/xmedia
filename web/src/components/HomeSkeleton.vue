<script setup lang="ts">
import { MdSkeleton } from '@/components/ui'
</script>

<template>
  <div class="space-y-10" aria-hidden="true">
    <section class="space-y-4">
      <div class="space-y-2">
        <MdSkeleton variant="text" width="7rem" height="1.25rem" />
        <MdSkeleton variant="text" width="13rem" height="0.875rem" />
      </div>
      <div class="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
        <div
          v-for="n in 3"
          :key="`sk-cont-${n}`"
          class="flex items-center gap-4 rounded-corner-md bg-surface-container-low p-3"
        >
          <MdSkeleton variant="rect" width="5rem" height="6rem" class="shrink-0" />
          <div class="flex-1 space-y-2.5">
            <MdSkeleton variant="text" width="75%" />
            <MdSkeleton variant="text" width="50%" height="0.75rem" />
            <MdSkeleton variant="rect" width="100%" height="0.375rem" class="rounded-corner-full" />
          </div>
        </div>
      </div>
    </section>

    <section class="space-y-4">
      <MdSkeleton variant="text" width="8rem" height="1.25rem" />
      <div class="grid grid-cols-2 gap-5 sm:grid-cols-3 lg:grid-cols-5">
        <MdSkeleton
          v-for="n in 5"
          :key="`sk-card-${n}`"
          variant="card"
          aspect="poster"
        />
      </div>
    </section>
  </div>
</template>
