<script setup lang="ts">
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRouter } from 'vue-router'
import { LuAlertCircle, LuEye, LuEyeOff } from 'vue-icons-plus/lu'
import ThemePopover from '@/components/ThemePopover.vue'
import { MdButton, MdCard, MdIconButton, MdTextField } from '@/components/ui'
import { useAuthStore } from '@/stores/auth'
import { useThemeStore } from '@/stores/theme'

const { t } = useI18n()
const router = useRouter()
const authStore = useAuthStore()
const themeStore = useThemeStore()

const username = ref('')
const password = ref('')
const showPassword = ref(false)
const submitting = ref(false)
const errorMsg = ref('')

async function onSubmit(): Promise<void> {
  errorMsg.value = ''
  submitting.value = true
  try {
    await authStore.login(username.value, password.value)
    await router.replace('/')
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    submitting.value = false
  }
}
</script>

<template>
  <div class="safe-pb flex min-h-screen flex-col bg-surface text-on-surface">
    <header class="safe-pt safe-px">
      <div class="flex items-center justify-between px-6 py-4">
        <div class="flex items-center gap-2.5">
          <img
            :src="themeStore.appIconSrc"
            alt=""
            aria-hidden="true"
            class="h-10 w-10 shrink-0 select-none"
          />
          <span class="text-title-medium tracking-tight">{{ t('app.name') }}</span>
        </div>
        <ThemePopover show-language />
      </div>
    </header>

    <main class="safe-px flex flex-1 items-center justify-center p-4">
      <MdCard
        variant="elevated"
        padding="lg"
        class="w-full max-w-md rounded-corner-xl"
      >
        <div class="mb-6">
          <h1 class="text-headline-small text-on-surface">
            {{ t('auth.loginTitle') }}
          </h1>
          <p class="text-body-small mt-1 text-on-surface-variant">
            {{ t('auth.loginSubtitle') }}
          </p>
        </div>

        <div
          v-if="errorMsg"
          role="alert"
          class="text-body-small mb-4 flex items-start gap-2 rounded-corner-md bg-error-container px-4 py-3 text-on-error-container"
        >
          <LuAlertCircle class="mt-0.5 h-4 w-4 shrink-0" />
          <span>{{ errorMsg }}</span>
        </div>

        <form class="space-y-4" @submit.prevent="onSubmit">
          <MdTextField
            v-model="username"
            type="text"
            required
            autocomplete="username"
            :label="t('auth.username')"
            :placeholder="t('auth.usernamePlaceholder')"
            :disabled="submitting"
          />

          <MdTextField
            v-model="password"
            :type="showPassword ? 'text' : 'password'"
            required
            autocomplete="current-password"
            :label="t('auth.password')"
            :placeholder="t('auth.passwordPlaceholder')"
            :disabled="submitting"
          >
            <template #trailing>
              <MdIconButton
                size="sm"
                :icon="showPassword ? LuEyeOff : LuEye"
                :aria-label="showPassword ? t('auth.hidePassword') : t('auth.showPassword')"
                @click="showPassword = !showPassword"
              />
            </template>
          </MdTextField>

          <MdButton
            type="submit"
            variant="filled"
            block
            :loading="submitting"
            class="mt-2"
          >
            {{ submitting ? t('common.loading') : t('auth.signInBtn') }}
          </MdButton>
        </form>
      </MdCard>
    </main>
  </div>
</template>
