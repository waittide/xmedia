<template>
  <div class="space-y-10 animate-pulse" aria-hidden="true">
    <section class="space-y-4">
      <div class="space-y-2">
        <div class="h-5 w-28 rounded-lg bg-[var(--md-sys-color-surface-container-high)]/70" />
        <div class="h-3.5 w-52 rounded-md bg-[var(--md-sys-color-surface-container-high)]/45" />
      </div>
      <div class="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
        <div
          v-for="n in 3"
          :key="`sk-cont-${n}`"
          class="flex items-center gap-4 rounded-2xl border border-[var(--md-sys-color-surface-container-high)]/60 bg-[var(--md-sys-color-surface-container-low)]/80 p-3.5"
        >
          <div
            class="h-24 w-20 shrink-0 rounded-xl bg-[var(--md-sys-color-surface-container-high)]/65"
          />
          <div class="flex-1 space-y-2.5">
            <div class="h-4 w-3/4 rounded-md bg-[var(--md-sys-color-surface-container-high)]/70" />
            <div class="h-3 w-1/2 rounded-md bg-[var(--md-sys-color-surface-container-high)]/45" />
            <div
              class="mt-3 h-1.5 w-full rounded-full bg-[var(--md-sys-color-surface-container-high)]/50"
            />
          </div>
        </div>
      </div>
    </section>

    <section class="space-y-4">
      <div class="h-5 w-32 rounded-lg bg-[var(--md-sys-color-surface-container-high)]/70" />
      <div class="grid grid-cols-2 gap-5 sm:grid-cols-3 lg:grid-cols-5">
        <div v-for="n in 5" :key="`sk-card-${n}`" class="space-y-2.5">
          <div
            class="aspect-[2/3] w-full rounded-xl bg-[var(--md-sys-color-surface-container-high)]/55"
          />
          <div class="h-3.5 w-4/5 rounded-md bg-[var(--md-sys-color-surface-container-high)]/60" />
          <div class="h-3 w-1/2 rounded-md bg-[var(--md-sys-color-surface-container-high)]/40" />
        </div>
      </div>
    </section>
  </div>
</template>
