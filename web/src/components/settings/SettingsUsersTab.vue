<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { LuTrash2, LuUserPlus } from 'vue-icons-plus/lu'
import { api } from '@/api/client'
import { useAuthStore } from '@/stores/auth'
import { useDialogStore } from '@/stores/dialog'
import type { UserDto, UserRole } from '@/types/api'
import AppSelect from '@/components/ui/AppSelect.vue'

const emit = defineEmits<{
  error: [message: string]
}>()

const { t } = useI18n()
const authStore = useAuthStore()
const dialogStore = useDialogStore()

const users = ref<UserDto[]>([])
const newUsername = ref('')
const newDisplayName = ref('')
const newPassword = ref('')
const newRole = ref<UserRole>('member')
const creatingUser = ref(false)
const resetPwdMap = ref<Record<string, string>>({})

const roleOptions = computed(() => [
  { value: 'member', label: t('auth.roleMember') },
  { value: 'admin', label: t('auth.roleAdmin') },
])

onMounted(async () => {
  try {
    users.value = await api.listUsers()
  } catch {
    // ignore
  }
})

async function handleCreateUser(): Promise<void> {
  emit('error', '')
  creatingUser.value = true
  try {
    await api.createUser({
      username: newUsername.value.trim(),
      display_name: newDisplayName.value.trim() || undefined,
      password: newPassword.value,
      role: newRole.value,
    })
    newUsername.value = ''
    newDisplayName.value = ''
    newPassword.value = ''
    newRole.value = 'member'
    users.value = await api.listUsers()
  } catch (err) {
    emit('error', err instanceof Error ? err.message : String(err))
  } finally {
    creatingUser.value = false
  }
}

async function handleRoleChange(u: UserDto, role: string): Promise<void> {
  emit('error', '')
  try {
    await api.updateUser(u.id, { role: role as UserRole })
    users.value = await api.listUsers()
  } catch (err) {
    emit('error', err instanceof Error ? err.message : String(err))
  }
}

async function handleActiveToggle(u: UserDto, nextActive: boolean): Promise<void> {
  emit('error', '')
  try {
    await api.updateUser(u.id, { is_active: nextActive })
    users.value = await api.listUsers()
  } catch (err) {
    emit('error', err instanceof Error ? err.message : String(err))
  }
}

async function handleResetUserPassword(u: UserDto): Promise<void> {
  const pwd = resetPwdMap.value[u.id]?.trim()
  if (!pwd) return
  emit('error', '')
  try {
    await api.updateUser(u.id, { password: pwd })
    resetPwdMap.value[u.id] = ''
  } catch (err) {
    emit('error', err instanceof Error ? err.message : String(err))
  }
}

async function handleDeleteUser(u: UserDto): Promise<void> {
  const confirmed = await dialogStore.confirm({
    title: t('admin.deleteUser'),
    message: t('admin.confirmDeleteUser', { username: u.username }),
    danger: true,
  })
  if (!confirmed) return
  emit('error', '')
  try {
    await api.deleteUser(u.id)
    users.value = await api.listUsers()
  } catch (err) {
    emit('error', err instanceof Error ? err.message : String(err))
  }
}
</script>

<template>
  <section class="card-soft rounded-2xl bg-[var(--ctp-shell)] p-6">
    <h2 class="text-sm font-bold">{{ t('admin.createUserTitle') }}</h2>
    <p class="mt-1 text-xs text-[var(--ctp-subtext0)]">{{ t('admin.createUserDesc') }}</p>

    <form class="mt-4 space-y-4" @submit.prevent="handleCreateUser">
      <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
        <div>
          <label class="mb-1.5 block text-xs font-semibold text-[var(--ctp-subtext1)]">
            {{ t('auth.username') }}
          </label>
          <input
            v-model="newUsername"
            type="text"
            required
            minlength="2"
            :placeholder="t('auth.usernamePlaceholder')"
            class="h-10 w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3.5 text-xs outline-none focus:border-[var(--ctp-accent)]"
          />
        </div>

        <div>
          <label class="mb-1.5 block text-xs font-semibold text-[var(--ctp-subtext1)]">
            {{ t('auth.displayName') }}
          </label>
          <input
            v-model="newDisplayName"
            type="text"
            :placeholder="t('auth.displayNamePlaceholder')"
            class="h-10 w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3.5 text-xs outline-none focus:border-[var(--ctp-accent)]"
          />
        </div>

        <div>
          <label class="mb-1.5 block text-xs font-semibold text-[var(--ctp-subtext1)]">
            {{ t('auth.password') }}
          </label>
          <input
            v-model="newPassword"
            type="password"
            required
            minlength="6"
            :placeholder="t('auth.passwordPlaceholder')"
            class="h-10 w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3.5 text-xs outline-none focus:border-[var(--ctp-accent)]"
          />
        </div>

        <div>
          <label class="mb-1.5 block text-xs font-semibold text-[var(--ctp-subtext1)]">
            {{ t('admin.userRoleLabel') }}
          </label>
          <AppSelect
            :model-value="newRole"
            :options="roleOptions"
            @update:model-value="newRole = $event as UserRole"
          />
        </div>
      </div>

      <button
        type="submit"
        :disabled="creatingUser"
        class="inline-flex h-10 items-center gap-2 rounded-xl bg-[var(--ctp-accent)] px-5 text-xs font-semibold text-[var(--ctp-on-accent)] shadow-sm transition hover:opacity-90 disabled:opacity-55"
      >
        <LuUserPlus class="h-4 w-4" />
        <span>{{ t('admin.createUserBtn') }}</span>
      </button>
    </form>
  </section>

  <section class="card-soft space-y-3 rounded-2xl bg-[var(--ctp-shell)] p-6">
    <div class="flex items-center justify-between gap-3">
      <h2 class="text-sm font-bold">{{ t('admin.usersListTitle') }}</h2>
      <RefreshButton
        :action="async () => { users = await api.listUsers() }"
      />
    </div>

    <div
      v-for="u in users"
      :key="u.id"
      class="flex flex-col justify-between gap-3 rounded-2xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] p-4 lg:flex-row lg:items-center"
    >
      <div class="min-w-0">
        <div class="flex items-center gap-2">
          <span class="truncate text-sm font-bold text-[var(--ctp-text)]">
            {{ u.display_name || u.username }}
          </span>
          <span class="font-mono text-xs text-[var(--ctp-subtext0)]">@{{ u.username }}</span>
        </div>
      </div>

      <div class="flex flex-wrap items-center gap-2.5">
        <div class="w-36">
          <AppSelect
            :model-value="u.role"
            :options="roleOptions"
            :disabled="u.id === authStore.user?.id"
            variant="compact"
            @update:model-value="handleRoleChange(u, $event)"
          />
        </div>

        <AppSwitch
          :model-value="u.is_active"
          :disabled="u.id === authStore.user?.id"
          :label="u.is_active ? t('auth.activeAccount') : t('auth.disabledAccount')"
          @update:model-value="handleActiveToggle(u, $event)"
        />

        <form
          class="flex items-center gap-1.5"
          @submit.prevent="handleResetUserPassword(u)"
        >
          <input
            v-model="resetPwdMap[u.id]"
            type="password"
            minlength="6"
            :placeholder="t('admin.resetPasswordPlaceholder')"
            class="h-8 w-36 rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] px-2.5 text-xs outline-none focus:border-[var(--ctp-accent)]"
          />
          <button
            type="submit"
            class="h-8 rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] px-2.5 text-xs font-medium text-[var(--ctp-text)] hover:border-[var(--ctp-accent)]"
          >
            {{ t('admin.resetPasswordBtn') }}
          </button>
        </form>

        <button
          v-if="u.id !== authStore.user?.id"
          type="button"
          class="rounded-xl p-1.5 text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-red)]/10 hover:text-[var(--ctp-red)]"
          :title="t('admin.deleteUser')"
          @click="handleDeleteUser(u)"
        >
          <LuTrash2 class="h-4 w-4" />
        </button>
      </div>
    </div>
  </section>
</template>
