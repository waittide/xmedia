<script setup lang="ts">
import { ref } from 'vue'
import { useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { LuLayers, LuSparkles } from 'vue-icons-plus/lu'
import { useAuthStore } from '@/stores/auth'
import ThemePopover from '@/components/ThemePopover.vue'

const { t } = useI18n()
const router = useRouter()
const authStore = useAuthStore()

const username = ref('admin')
const displayName = ref('')
const password = ref('')
const submitting = ref(false)
const errorMsg = ref('')

async function onSubmit(): Promise<void> {
  errorMsg.value = ''
  submitting.value = true
  try {
    await authStore.setupAdmin({
      username: username.value.trim(),
      display_name: displayName.value.trim(),
      password: password.value,
    })
    await router.replace('/')
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    submitting.value = false
  }
}
</script>

<template>
  <div class="flex min-h-screen flex-col bg-[var(--ctp-shell)] text-[var(--ctp-text)]">
    <header class="flex items-center justify-between px-6 py-4">
      <div class="flex items-center gap-2.5">
        <div
          class="flex h-10 w-10 items-center justify-center rounded-2xl bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)] shadow-sm"
        >
          <LuLayers class="h-5 w-5" />
        </div>
        <span class="text-lg font-bold tracking-tight">{{ t('app.name') }}</span>
      </div>
      <ThemePopover show-language />
    </header>

    <main class="flex flex-1 items-center justify-center p-4">
      <div
        class="w-full max-w-md rounded-[28px] border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] p-8 shadow-xl"
      >
        <div class="mb-6 flex items-center gap-3">
          <div
            class="flex h-11 w-11 items-center justify-center rounded-2xl bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]"
          >
            <LuSparkles class="h-6 w-6" />
          </div>
          <div>
            <h1 class="text-xl font-bold">{{ t('auth.setupTitle') }}</h1>
            <p class="mt-0.5 text-xs text-[var(--ctp-subtext0)]">{{ t('auth.setupSubtitle') }}</p>
          </div>
        </div>

        <div
          v-if="errorMsg"
          class="mb-4 rounded-2xl bg-[var(--ctp-red)]/15 px-4 py-3 text-xs font-medium text-[var(--ctp-red)]"
        >
          {{ errorMsg }}
        </div>

        <form class="space-y-4" @submit.prevent="onSubmit">
          <div>
            <label class="mb-1.5 block text-xs font-semibold text-[var(--ctp-subtext1)]">
              {{ t('auth.username') }}
            </label>
            <input
              v-model="username"
              type="text"
              required
              minlength="2"
              :placeholder="t('auth.usernamePlaceholder')"
              class="w-full rounded-2xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] px-4 py-2.5 text-sm outline-none transition focus:border-[var(--ctp-accent)]"
            />
          </div>

          <div>
            <label class="mb-1.5 block text-xs font-semibold text-[var(--ctp-subtext1)]">
              {{ t('auth.displayName') }}
            </label>
            <input
              v-model="displayName"
              type="text"
              :placeholder="t('auth.displayNamePlaceholder')"
              class="w-full rounded-2xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] px-4 py-2.5 text-sm outline-none transition focus:border-[var(--ctp-accent)]"
            />
          </div>

          <div>
            <label class="mb-1.5 block text-xs font-semibold text-[var(--ctp-subtext1)]">
              {{ t('auth.password') }}
            </label>
            <input
              v-model="password"
              type="password"
              required
              minlength="6"
              :placeholder="t('auth.passwordPlaceholder')"
              class="w-full rounded-2xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] px-4 py-2.5 text-sm outline-none transition focus:border-[var(--ctp-accent)]"
            />
          </div>

          <button
            type="submit"
            :disabled="submitting"
            class="mt-2 w-full rounded-full bg-[var(--ctp-accent)] py-3 text-sm font-semibold text-[var(--ctp-on-accent)] shadow-md transition hover:opacity-95 disabled:opacity-60"
          >
            {{ submitting ? t('common.loading') : t('auth.createAdminBtn') }}
          </button>
        </form>
      </div>
    </main>
  </div>
</template>
