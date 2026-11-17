<script setup lang="ts">
import { ref } from 'vue'
import { useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { useAuthStore } from '@/stores/auth'

const { t } = useI18n()
const router = useRouter()
const auth = useAuthStore()

const username = ref('')
const password = ref('')
const loading = ref(false)
const error = ref('')

async function onSubmit(): Promise<void> {
  loading.value = true
  error.value = ''
  try {
    await auth.login(username.value.trim(), password.value)
    await router.push('/')
  } catch (err) {
    error.value = err instanceof Error ? err.message : String(err)
  } finally {
    loading.value = false
  }
}
</script>

<template>
  <div class="flex min-h-screen items-center justify-center bg-[var(--ctp-shell)] p-6">
    <form
      class="w-full max-w-md space-y-5 rounded-2xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] p-8 shadow-xl"
      @submit.prevent="onSubmit"
    >
      <div class="space-y-1.5">
        <div class="inline-flex h-10 w-10 items-center justify-center rounded-xl bg-[var(--ctp-accent)] text-sm font-extrabold text-[var(--ctp-on-accent)]">
          XM
        </div>
        <h1 class="text-xl font-bold text-[var(--ctp-text)]">{{ t('auth.loginTitle') }}</h1>
        <p class="text-xs text-[var(--ctp-subtext0)]">{{ t('auth.loginSubtitle') }}</p>
      </div>

      <div v-if="error" class="rounded-lg bg-[var(--ctp-red)]/15 px-3 py-2 text-xs text-[var(--ctp-red)]">
        {{ error }}
      </div>

      <div class="space-y-3">
        <div>
          <label class="mb-1 block text-xs font-medium text-[var(--ctp-subtext1)]">{{ t('auth.username') }}</label>
          <input
            v-model="username"
            required
            class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3.5 py-2.5 text-sm outline-none focus:border-[var(--ctp-accent)]"
          />
        </div>
        <div>
          <label class="mb-1 block text-xs font-medium text-[var(--ctp-subtext1)]">{{ t('auth.password') }}</label>
          <input
            v-model="password"
            type="password"
            required
            class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3.5 py-2.5 text-sm outline-none focus:border-[var(--ctp-accent)]"
          />
        </div>
      </div>

      <button
        type="submit"
        :disabled="loading"
        class="w-full rounded-xl bg-[var(--ctp-accent)] py-2.5 text-sm font-semibold text-[var(--ctp-on-accent)] shadow hover:opacity-90 disabled:opacity-50"
      >
        {{ loading ? t('common.loading') : t('auth.submitLogin') }}
      </button>
    </form>
  </div>
</template>
