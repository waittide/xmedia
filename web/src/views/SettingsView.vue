<script setup lang="ts">
import { onMounted, ref, watch } from 'vue'
import {
  LuFolderPlus,
  LuKeyRound,
  LuPalette,
  LuRefreshCw,
  LuTrash2,
  LuUserPlus,
} from 'vue-icons-plus/lu'
import { api } from '@/api/client'
import { CATPPUCCIN_ACCENTS } from '@/theme/catppuccin'
import { useAuthStore } from '@/stores/auth'
import { useEventsStore } from '@/stores/events'
import { useThemeStore } from '@/stores/theme'
import type { LibraryDto, MediaKind, UserDto, UserRole } from '@/types/api'

const auth = useAuthStore()
const theme = useThemeStore()
const events = useEventsStore()

const libraries = ref<LibraryDto[]>([])
const users = ref<UserDto[]>([])
const errorMsg = ref('')

const newLibName = ref('')
const newLibKind = ref<MediaKind>('book')
const newLibPath = ref('')

const newUsername = ref('')
const newDisplayName = ref('')
const newPassword = ref('')
const newRole = ref<UserRole>('member')

async function loadData(): Promise<void> {
  try {
    libraries.value = await api.listLibraries()
    if (auth.isAdmin) {
      users.value = await api.listUsers()
    }
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  }
}

onMounted(loadData)
watch(() => events.changeTick, loadData, { deep: true })

async function handleCreateLibrary(): Promise<void> {
  errorMsg.value = ''
  try {
    const lib = await api.createLibrary({
      name: newLibName.value.trim(),
      media_kind: newLibKind.value,
      root_path: newLibPath.value.trim(),
    })
    newLibName.value = ''
    newLibPath.value = ''
    await api.scanLibrary(lib.id)
    await loadData()
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  }
}

async function handleScanLibrary(id: string): Promise<void> {
  try {
    await api.scanLibrary(id)
    await loadData()
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  }
}

async function handleDeleteLibrary(id: string): Promise<void> {
  try {
    await api.deleteLibrary(id)
    await loadData()
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  }
}

async function handleCreateUser(): Promise<void> {
  errorMsg.value = ''
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
    await loadData()
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  }
}

async function handleDeleteUser(id: string): Promise<void> {
  try {
    await api.deleteUser(id)
    await loadData()
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  }
}
</script>

<template>
  <div class="mx-auto max-w-5xl space-y-8 p-6">
    <div v-if="errorMsg" class="rounded-xl bg-[var(--ctp-red)]/15 px-4 py-3 text-xs text-[var(--ctp-red)]">
      {{ errorMsg }}
    </div>

    <!-- Theme & Catppuccin Palette -->
    <section class="rounded-2xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] p-6 space-y-4">
      <div class="flex items-center gap-2 text-base font-semibold">
        <LuPalette class="h-5 w-5 text-[var(--ctp-accent)]" />
        <span>Appearance & Catppuccin Theme</span>
      </div>

      <div class="grid grid-cols-1 gap-4 md:grid-cols-3">
        <div>
          <label class="mb-1 block text-xs text-[var(--ctp-subtext0)]">Theme Mode</label>
          <select
            v-model="theme.themeMode"
            class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-sm"
            @change="theme.persistPreferences()"
          >
            <option value="system">System</option>
            <option value="light">Light</option>
            <option value="dark">Dark</option>
          </select>
        </div>

        <div>
          <label class="mb-1 block text-xs text-[var(--ctp-subtext0)]">Light Preset</label>
          <select
            v-model="theme.lightPreset"
            class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-sm"
            @change="theme.persistPreferences()"
          >
            <option v-for="p in theme.lightPresets" :key="p.id" :value="p.id">{{ p.id }}</option>
          </select>
        </div>

        <div>
          <label class="mb-1 block text-xs text-[var(--ctp-subtext0)]">Dark Preset</label>
          <select
            v-model="theme.darkPreset"
            class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-sm"
            @change="theme.persistPreferences()"
          >
            <option v-for="p in theme.darkPresets" :key="p.id" :value="p.id">{{ p.id }}</option>
          </select>
        </div>
      </div>

      <div>
        <label class="mb-2 block text-xs text-[var(--ctp-subtext0)]">Catppuccin Accent</label>
        <div class="flex flex-wrap gap-2">
          <button
            v-for="acc in CATPPUCCIN_ACCENTS"
            :key="acc"
            type="button"
            class="rounded-lg border px-2.5 py-1 text-xs capitalize transition"
            :class="
              theme.accent === acc
                ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)] font-semibold'
                : 'border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] text-[var(--ctp-subtext1)]'
            "
            @click="
              theme.accentMode = 'catppuccin';
              theme.accent = acc;
              theme.persistPreferences()
            "
          >
            {{ acc }}
          </button>
        </div>
      </div>
    </section>

    <!-- API Key & Subsonic / OPDS Credentials -->
    <section class="rounded-2xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] p-6 space-y-3">
      <div class="flex items-center justify-between">
        <div class="flex items-center gap-2 text-base font-semibold">
          <LuKeyRound class="h-5 w-5 text-[var(--ctp-accent)]" />
          <span>API Key (OpenSubsonic / OPDS / External Clients)</span>
        </div>
        <button
          type="button"
          class="rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-1.5 text-xs font-medium hover:border-[var(--ctp-accent)]"
          @click="auth.rotateApiKey()"
        >
          Rotate Key
        </button>
      </div>
      <code class="block rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-4 py-2.5 text-xs select-all">
        {{ auth.user?.api_key }}
      </code>
    </section>

    <!-- Unified Multi-Domain Libraries -->
    <section class="rounded-2xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] p-6 space-y-4">
      <div class="flex items-center gap-2 text-base font-semibold">
        <LuFolderPlus class="h-5 w-5 text-[var(--ctp-accent)]" />
        <span>Media Libraries (Book / Comic / Music / Video)</span>
      </div>

      <div class="divide-y divide-[var(--ctp-surface0)] rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)]">
        <div
          v-for="lib in libraries"
          :key="lib.id"
          class="flex flex-wrap items-center justify-between gap-3 p-3.5 text-sm"
        >
          <div>
            <div class="flex items-center gap-2 font-semibold">
              <span class="rounded bg-[var(--ctp-accent-soft)] px-2 py-0.5 text-[10px] uppercase text-[var(--ctp-accent)]">
                {{ lib.media_kind }}
              </span>
              <span>{{ lib.name }}</span>
              <span class="text-xs font-normal text-[var(--ctp-subtext0)]">
                ({{ lib.item_count }} items)
              </span>
            </div>
            <div class="mt-0.5 text-xs text-[var(--ctp-subtext0)]">
              {{ lib.kind.type === 'local_mount' ? lib.kind.root_path : 'Managed Uploads' }}
            </div>
          </div>

          <div v-if="auth.isAdmin && lib.kind.type === 'local_mount'" class="flex items-center gap-2">
            <button
              type="button"
              class="inline-flex items-center gap-1 rounded-lg border border-[var(--ctp-surface0)] px-2.5 py-1 text-xs hover:border-[var(--ctp-accent)]"
              @click="handleScanLibrary(lib.id)"
            >
              <LuRefreshCw class="h-3.5 w-3.5" /> Scan
            </button>
            <button
              type="button"
              class="rounded-lg p-1.5 text-[var(--ctp-subtext0)] hover:text-[var(--ctp-red)]"
              @click="handleDeleteLibrary(lib.id)"
            >
              <LuTrash2 class="h-4 w-4" />
            </button>
          </div>
        </div>
      </div>

      <form
        v-if="auth.isAdmin"
        class="grid grid-cols-1 gap-3 pt-2 md:grid-cols-4"
        @submit.prevent="handleCreateLibrary"
      >
        <select
          v-model="newLibKind"
          class="rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-sm"
        >
          <option v-for="m in auth.enabledModules" :key="m" :value="m">{{ m.toUpperCase() }}</option>
        </select>
        <input
          v-model="newLibName"
          required
          placeholder="Library Name"
          class="rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-sm"
        />
        <input
          v-model="newLibPath"
          required
          placeholder="/absolute/mount/path"
          class="rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-sm"
        />
        <button
          type="submit"
          class="rounded-xl bg-[var(--ctp-accent)] px-4 py-2 text-sm font-semibold text-[var(--ctp-on-accent)]"
        >
          Mount & Scan
        </button>
      </form>
    </section>

    <!-- Admin User Management -->
    <section
      v-if="auth.isAdmin"
      class="rounded-2xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] p-6 space-y-4"
    >
      <div class="flex items-center gap-2 text-base font-semibold">
        <LuUserPlus class="h-5 w-5 text-[var(--ctp-accent)]" />
        <span>Users</span>
      </div>

      <div class="divide-y divide-[var(--ctp-surface0)] rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)]">
        <div
          v-for="u in users"
          :key="u.id"
          class="flex items-center justify-between p-3.5 text-sm"
        >
          <div>
            <span class="font-semibold">{{ u.display_name || u.username }}</span>
            <span class="ml-2 text-xs text-[var(--ctp-subtext0)]">@{{ u.username }} · {{ u.role }}</span>
          </div>
          <button
            v-if="u.id !== auth.user?.id"
            type="button"
            class="rounded-lg p-1.5 text-[var(--ctp-subtext0)] hover:text-[var(--ctp-red)]"
            @click="handleDeleteUser(u.id)"
          >
            <LuTrash2 class="h-4 w-4" />
          </button>
        </div>
      </div>

      <form class="grid grid-cols-1 gap-3 pt-2 md:grid-cols-4" @submit.prevent="handleCreateUser">
        <input
          v-model="newUsername"
          required
          placeholder="Username"
          class="rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-sm"
        />
        <input
          v-model="newPassword"
          type="password"
          required
          minlength="6"
          placeholder="Password"
          class="rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-sm"
        />
        <select
          v-model="newRole"
          class="rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-sm"
        >
          <option value="member">Member</option>
          <option value="admin">Admin</option>
        </select>
        <button
          type="submit"
          class="rounded-xl bg-[var(--ctp-accent)] px-4 py-2 text-sm font-semibold text-[var(--ctp-on-accent)]"
        >
          Create User
        </button>
      </form>
    </section>
  </div>
</template>
