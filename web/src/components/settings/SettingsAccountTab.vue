<script setup lang="ts">
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { LuCheck, LuCopy, LuRotateCw } from 'vue-icons-plus/lu'
import { api, setStoredToken } from '@/api/client'
import { useAuthStore } from '@/stores/auth'
import { useDialogStore } from '@/stores/dialog'

const emit = defineEmits<{
  error: [message: string]
}>()

const { t } = useI18n()
const authStore = useAuthStore()
const dialogStore = useDialogStore()

const copiedKey = ref('')
const currentPwd = ref('')
const nextPwd = ref('')
const pwdSuccess = ref('')

async function copyText(key: string, val: string): Promise<void> {
  if (!val) return
  try {
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(val)
    } else {
      throw new Error('clipboard API unavailable')
    }
  } catch {
    const ta = document.createElement('textarea')
    ta.value = val
    ta.style.position = 'fixed'
    ta.style.opacity = '0'
    document.body.appendChild(ta)
    ta.select()
    document.execCommand('copy')
    document.body.removeChild(ta)
  }
  copiedKey.value = key
  setTimeout(() => {
    if (copiedKey.value === key) copiedKey.value = ''
  }, 1500)
}

async function submitPasswordChange(): Promise<void> {
  emit('error', '')
  pwdSuccess.value = ''
  try {
    const session = await api.changePassword({
      current_password: currentPwd.value,
      new_password: nextPwd.value,
    })
    setStoredToken(session.token)
    authStore.setUser(session.user)
    currentPwd.value = ''
    nextPwd.value = ''
    pwdSuccess.value = t('auth.passwordUpdated')
  } catch (err) {
    emit('error', err instanceof Error ? err.message : String(err))
  }
}

async function handleRotateApiKey(): Promise<void> {
  const ok = await dialogStore.confirm({
    title: t('settings.rotateApiKey'),
    message: t('settings.rotateApiKeyConfirm'),
    danger: true,
  })
  if (!ok) return
  const updated = await api.rotateApiKey()
  authStore.setUser(updated)
}
</script>

<template>
  <section class="card-soft space-y-4 rounded-2xl bg-[var(--ctp-shell)] p-6">
    <div>
      <h2 class="text-sm font-bold">{{ t('settings.apiKeyTitle') }}</h2>
      <p class="mt-1 text-xs text-[var(--ctp-subtext0)]">{{ t('settings.apiKeyDesc') }}</p>
    </div>

    <div class="flex flex-wrap items-center gap-2">
      <input
        readonly
        :value="authStore.user?.api_key || ''"
        class="h-10 min-w-0 flex-1 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3.5 font-mono text-xs text-[var(--ctp-text)] outline-none"
      />
      <button
        type="button"
        class="inline-flex h-10 items-center gap-1.5 rounded-xl bg-[var(--ctp-accent)] px-4 text-xs font-semibold text-[var(--ctp-on-accent)] transition hover:opacity-90"
        @click="copyText('apiKey', authStore.user?.api_key || '')"
      >
        <LuCheck v-if="copiedKey === 'apiKey'" class="h-3.5 w-3.5" />
        <LuCopy v-else class="h-3.5 w-3.5" />
        <span>{{ copiedKey === 'apiKey' ? t('common.copied') : t('common.copy') }}</span>
      </button>
      <button
        type="button"
        class="inline-flex h-10 items-center gap-1.5 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3.5 text-xs font-semibold text-[var(--ctp-red)] transition hover:bg-[var(--ctp-red)]/10"
        @click="handleRotateApiKey"
      >
        <LuRotateCw class="h-3.5 w-3.5" />
        <span>{{ t('settings.rotateApiKey') }}</span>
      </button>
    </div>
  </section>

  <section class="card-soft rounded-2xl bg-[var(--ctp-shell)] p-6">
    <h2 class="text-sm font-bold">{{ t('auth.password') }}</h2>

    <div
      v-if="pwdSuccess"
      class="mt-3 rounded-xl bg-[var(--ctp-green)]/15 px-3.5 py-2.5 text-xs font-medium text-[var(--ctp-green)]"
    >
      {{ pwdSuccess }}
    </div>

    <form class="mt-4 max-w-md space-y-4" @submit.prevent="submitPasswordChange">
      <div>
        <label class="mb-1.5 block text-xs font-semibold text-[var(--ctp-subtext1)]">
          {{ t('auth.currentPassword') }}
        </label>
        <input
          v-model="currentPwd"
          type="password"
          required
          class="h-10 w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3.5 text-xs outline-none focus:border-[var(--ctp-accent)]"
        />
      </div>

      <div>
        <label class="mb-1.5 block text-xs font-semibold text-[var(--ctp-subtext1)]">
          {{ t('auth.newPassword') }}
        </label>
        <input
          v-model="nextPwd"
          type="password"
          required
          minlength="6"
          :placeholder="t('auth.passwordPlaceholder')"
          class="h-10 w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3.5 text-xs outline-none focus:border-[var(--ctp-accent)]"
        />
      </div>

      <button
        type="submit"
        class="inline-flex h-10 items-center rounded-xl bg-[var(--ctp-accent)] px-5 text-xs font-semibold text-[var(--ctp-on-accent)] shadow-sm transition hover:opacity-90"
      >
        {{ t('common.save') }}
      </button>
    </form>
  </section>
</template>
