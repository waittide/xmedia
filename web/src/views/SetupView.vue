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
const confirmPassword = ref('')
const submitting = ref(false)
const errorMsg = ref('')

async function onSubmit(): Promise<void> {
  errorMsg.value = ''
  const trimmedUsername = username.value.trim()
  if (trimmedUsername.length < 2) {
    errorMsg.value = t('auth.usernamePlaceholder')
    return
  }
  if (password.value !== confirmPassword.value) {
    errorMsg.value = t('auth.passwordMismatch')
    return
  }
  submitting.value = true
  try {
    await authStore.setupAdmin({
      username: trimmedUsername,
      display_name: displayName.value.trim() || undefined,
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
  <div
    class="safe-pb flex min-h-screen flex-col bg-[var(--md-sys-color-surface)] text-[var(--md-sys-color-on-surface)]"
  >
    <header class="safe-pt safe-px">
      <div class="flex items-center justify-between px-6 py-4">
        <div class="flex items-center gap-2.5">
          <div
            class="flex h-10 w-10 items-center justify-center rounded-2xl bg-[var(--md-sys-color-primary)] text-[var(--md-sys-color-on-primary)] shadow-[var(--md-sys-elevation-1)]"
          >
            <LuLayers class="h-5 w-5" />
          </div>
          <span class="md-title-medium tracking-tight">{{ t('app.name') }}</span>
        </div>
        <ThemePopover show-language />
      </div>
    </header>

    <main class="safe-px flex flex-1 items-center justify-center">
      <div class="flex w-full justify-center p-4">
        <div
          class="w-full max-w-md rounded-[28px] border border-[var(--md-sys-color-outline-variant)]/55 bg-[var(--md-sys-color-surface-container-low)] p-8 shadow-[var(--md-sys-elevation-2)]"
        >
        <div class="mb-6 flex items-center gap-3">
          <div
            class="flex h-11 w-11 shrink-0 items-center justify-center rounded-2xl bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)]"
          >
            <LuSparkles class="h-6 w-6" />
          </div>
          <div>
            <h1 class="md-title-large text-[var(--md-sys-color-on-surface)]">
              {{ t('auth.setupTitle') }}
            </h1>
            <p class="md-body-small mt-0.5 text-[var(--md-sys-color-on-surface-variant)]">
              {{ t('auth.setupSubtitle') }}
            </p>
          </div>
        </div>

        <div
          v-if="errorMsg"
          class="mb-4 rounded-[var(--md-sys-shape-corner-md)] bg-[var(--md-sys-color-error-container)] px-4 py-3 text-xs font-medium text-[var(--md-sys-color-on-error-container)]"
        >
          {{ errorMsg }}
        </div>

        <form class="space-y-4" @submit.prevent="onSubmit">
          <div>
            <label
              class="md-label-medium mb-1.5 block text-[var(--md-sys-color-on-surface-variant)]"
            >
              {{ t('auth.username') }}
            </label>
            <input
              v-model="username"
              type="text"
              required
              minlength="2"
              :placeholder="t('auth.usernamePlaceholder')"
              class="md-input h-11 w-full text-sm"
            />
          </div>

          <div>
            <label
              class="md-label-medium mb-1.5 block text-[var(--md-sys-color-on-surface-variant)]"
            >
              {{ t('auth.displayName') }}
            </label>
            <input
              v-model="displayName"
              type="text"
              :placeholder="t('auth.displayNamePlaceholder')"
              class="md-input h-11 w-full text-sm"
            />
          </div>

          <div>
            <label
              class="md-label-medium mb-1.5 block text-[var(--md-sys-color-on-surface-variant)]"
            >
              {{ t('auth.password') }}
            </label>
            <input
              v-model="password"
              type="password"
              required
              minlength="6"
              :placeholder="t('auth.passwordPlaceholder')"
              class="md-input h-11 w-full text-sm"
            />
          </div>

          <div>
            <label
              class="md-label-medium mb-1.5 block text-[var(--md-sys-color-on-surface-variant)]"
            >
              {{ t('auth.confirmPassword') }}
            </label>
            <input
              v-model="confirmPassword"
              type="password"
              required
              minlength="6"
              :placeholder="t('auth.confirmPasswordPlaceholder')"
              class="md-input h-11 w-full text-sm"
            />
          </div>

          <button
            type="submit"
            :disabled="submitting"
            class="md-btn md-btn-filled mt-2 h-11 w-full"
          >
            {{ submitting ? t('common.loading') : t('auth.createAdminBtn') }}
          </button>
        </form>
        </div>
      </div>
    </main>
  </div>
</template>
