<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { LuFolderPlus, LuTrash2 } from 'vue-icons-plus/lu'
import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import AppSelect from '@/components/ui/AppSelect.vue'
import AppSwitch from '@/components/ui/AppSwitch.vue'
import type { MediaKind } from '@/types/api'
import { toolApi } from '../../api'
import type { ManagedRootDto, PublicSettingsDto } from '../../types'

const props = defineProps<{
  settings: PublicSettingsDto | null
  domainSubTab: MediaKind
}>()

const emit = defineEmits<{
  updated: [settings: PublicSettingsDto]
  error: [message: string | null]
  saved: []
}>()

const domainBadgeLabels: Record<MediaKind, string> = {
  book: '图书',
  comic: '漫画',
  music: '音乐',
  video: '影视',
}

const newRootName = ref('')
const newRootPath = ref('')
const newRootReadOnly = ref(false)
const addingRoot = ref(false)

const inboxPath = ref('')
const inboxTargetRootId = ref('')
const inboxUseHardlink = ref(true)
const inboxDeleteSource = ref(false)
const inboxAutoScrape = ref(false)
const inboxOrganize = ref(true)
const scanningInbox = ref(false)
const inboxNotice = ref<string | null>(null)

const webhookEnabled = ref(false)
const webhookBaseUrl = ref('')
const webhookApiKey = ref('')
const webhookNotifyRename = ref(true)
const webhookNotifyScrape = ref(true)
const webhookNotifyInbox = ref(true)
const testingWebhook = ref(false)
const webhookNotice = ref<string | null>(null)
const saving = ref(false)

const inboxRootOptions = computed(() => [
  { value: '', label: '请选择已登记的根目录' },
  ...(props.settings?.roots ?? []).map((r) => ({
    value: r.id,
    label: `${r.name} (${r.path})`,
  })),
])

const filteredRoots = computed<ManagedRootDto[]>(() => {
  const all = props.settings?.roots ?? []
  return all.filter((r) => r.default_kind === props.domainSubTab)
})

watch(
  () => props.settings,
  (s) => {
    if (!s) return
    if (s.inbox) {
      inboxPath.value = s.inbox.inbox_path ?? ''
      inboxTargetRootId.value = s.inbox.target_root_id ?? ''
      inboxUseHardlink.value = s.inbox.use_hardlink
      inboxDeleteSource.value = s.inbox.delete_source
      inboxAutoScrape.value = s.inbox.auto_scrape
      inboxOrganize.value = s.inbox.organize_by_template
    }
    webhookEnabled.value = Boolean(s.xmedia_webhook_enabled)
    webhookBaseUrl.value = s.xmedia_webhook_base_url ?? ''
    webhookNotifyRename.value = s.xmedia_webhook_notify_on_rename ?? true
    webhookNotifyScrape.value = s.xmedia_webhook_notify_on_scrape ?? true
    webhookNotifyInbox.value = s.xmedia_webhook_notify_on_inbox ?? true
  },
  { immediate: true },
)

async function handleAddRoot(): Promise<void> {
  if (!newRootPath.value.trim() || addingRoot.value) return
  addingRoot.value = true
  emit('error', null)
  try {
    await toolApi.addRoot(
      newRootName.value.trim(),
      newRootPath.value.trim(),
      props.domainSubTab,
      newRootReadOnly.value,
    )
    newRootName.value = ''
    newRootPath.value = ''
    newRootReadOnly.value = false
    const next = await toolApi.getSettings()
    emit('updated', next)
  } catch (err) {
    emit('error', err instanceof Error ? err.message : String(err))
  } finally {
    addingRoot.value = false
  }
}

async function handleToggleRootReadOnly(id: string, currentReadOnly: boolean): Promise<void> {
  emit('error', null)
  try {
    await toolApi.setRootReadOnly(id, !currentReadOnly)
    const next = await toolApi.getSettings()
    emit('updated', next)
  } catch (err) {
    emit('error', err instanceof Error ? err.message : String(err))
  }
}

async function handleRemoveRoot(id: string): Promise<void> {
  emit('error', null)
  try {
    await toolApi.removeRoot(id)
    const next = await toolApi.getSettings()
    emit('updated', next)
  } catch (err) {
    emit('error', err instanceof Error ? err.message : String(err))
  }
}

async function handleSaveInboxAndWebhook(): Promise<void> {
  saving.value = true
  emit('error', null)
  inboxNotice.value = null
  webhookNotice.value = null
  try {
    const next = await toolApi.updateSettings({
      inbox: {
        enabled: Boolean(inboxPath.value.trim() && inboxTargetRootId.value),
        inbox_path: inboxPath.value.trim() || null,
        target_root_id: inboxTargetRootId.value || null,
        use_hardlink: inboxUseHardlink.value,
        delete_source: inboxDeleteSource.value,
        auto_scrape: inboxAutoScrape.value,
        organize_by_template: inboxOrganize.value,
      },
      xmedia_webhook_enabled: webhookEnabled.value,
      xmedia_webhook_base_url: webhookBaseUrl.value.trim(),
      xmedia_webhook_api_key: webhookApiKey.value.trim() || undefined,
      xmedia_webhook_notify_on_rename: webhookNotifyRename.value,
      xmedia_webhook_notify_on_scrape: webhookNotifyScrape.value,
      xmedia_webhook_notify_on_inbox: webhookNotifyInbox.value,
    })
    webhookApiKey.value = ''
    emit('updated', next)
    emit('saved')
  } catch (err) {
    emit('error', err instanceof Error ? err.message : String(err))
  } finally {
    saving.value = false
  }
}

async function handleScanInbox(): Promise<void> {
  scanningInbox.value = true
  emit('error', null)
  inboxNotice.value = null
  try {
    const rep = await toolApi.scanInbox()
    inboxNotice.value =
      '导入完成：成功 ' +
      rep.imported_count +
      ' 项，跳过 ' +
      rep.skipped_count +
      ' 项' +
      (rep.failed_count > 0 ? '，失败 ' + rep.failed_count + ' 项' : '')
  } catch (err) {
    emit('error', err instanceof Error ? err.message : String(err))
  } finally {
    scanningInbox.value = false
  }
}

async function handleTestWebhook(): Promise<void> {
  testingWebhook.value = true
  emit('error', null)
  webhookNotice.value = null
  try {
    const rep = await toolApi.testWebhook()
    webhookNotice.value =
      rep.message +
      (rep.matched_libraries.length > 0
        ? '（挂载库：' + rep.matched_libraries.join('、') + '）'
        : '')
  } catch (err) {
    emit('error', err instanceof Error ? err.message : String(err))
  } finally {
    testingWebhook.value = false
  }
}
</script>

<template>
  <section class="card-soft rounded-2xl bg-[var(--ctp-shell)] p-6">
    <h2 class="text-sm font-bold text-[var(--ctp-text)]">
      新增「{{ domainBadgeLabels[domainSubTab] }}」本地目录
    </h2>
    <p class="mt-1 text-xs text-[var(--ctp-subtext0)]">
      各媒体类型的根目录白名单彼此隔离，切换上方类型 Tab 可分别管理不同领域的目录
    </p>

    <form class="mt-4 space-y-4" @submit.prevent="handleAddRoot">
      <div class="grid grid-cols-1 gap-4 sm:grid-cols-12">
        <div class="sm:col-span-4">
          <label class="mb-1.5 block text-xs font-semibold text-[var(--ctp-subtext1)]">
            显示别名（可选）
          </label>
          <input
            v-model="newRootName"
            type="text"
            placeholder="默认取文件夹名"
            class="h-10 w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3.5 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
          />
        </div>

        <div class="sm:col-span-8">
          <label class="mb-1.5 block text-xs font-semibold text-[var(--ctp-subtext1)]">
            服务器本地绝对路径
          </label>
          <input
            v-model="newRootPath"
            type="text"
            required
            :placeholder="`例如 /data/media/${domainSubTab}`"
            class="h-10 w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3.5 font-mono text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
          />
        </div>
      </div>

      <div class="flex justify-end">
        <button
          type="submit"
          :disabled="!newRootPath.trim() || addingRoot"
          class="inline-flex h-10 items-center gap-2 rounded-xl bg-[var(--ctp-accent)] px-5 text-xs font-semibold text-[var(--ctp-on-accent)] shadow-sm transition hover:opacity-95 disabled:opacity-50"
        >
          <LuFolderPlus class="h-4 w-4" />
          <span>添加到「{{ domainBadgeLabels[domainSubTab] }}」</span>
        </button>
      </div>
    </form>
  </section>

  <section class="card-soft space-y-3 rounded-2xl bg-[var(--ctp-shell)] p-6">
    <h2 class="text-sm font-bold text-[var(--ctp-text)]">
      已登记「{{ domainBadgeLabels[domainSubTab] }}」根目录 ({{ filteredRoots.length }})
    </h2>

    <div
      v-if="filteredRoots.length === 0"
      class="rounded-2xl border border-dashed border-[var(--ctp-surface1)] bg-[var(--ctp-canvas)]/60 py-10 text-center text-xs text-[var(--ctp-subtext0)]"
    >
      当前「{{ domainBadgeLabels[domainSubTab] }}」领域尚未添加本地目录
    </div>

    <div
      v-for="r in filteredRoots"
      :key="r.id"
      class="flex items-center justify-between gap-3 rounded-2xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] p-4"
    >
      <div class="min-w-0">
        <div class="flex items-center gap-2">
          <span class="truncate text-sm font-bold text-[var(--ctp-text)]">{{ r.name }}</span>
          <span
            class="rounded-full bg-[var(--ctp-accent-soft)] px-2 py-0.5 text-[10px] font-semibold text-[var(--ctp-accent)]"
          >
            {{ domainBadgeLabels[domainSubTab] }}
          </span>
          <span
            v-if="r.read_only"
            class="rounded-full bg-amber-500/15 px-2 py-0.5 text-[10px] font-semibold text-amber-500"
          >
            只读保护
          </span>
        </div>
        <p class="mt-1 truncate font-mono text-xs text-[var(--ctp-subtext0)]">{{ r.path }}</p>
      </div>

      <div class="flex shrink-0 items-center gap-1.5">
        <button
          type="button"
          class="rounded-xl border px-2.5 py-1.5 text-[11px] font-medium transition"
          :class="
            r.read_only
              ? 'border-amber-500/45 bg-amber-500/10 text-amber-500 hover:bg-amber-500/20'
              : 'border-[var(--ctp-surface0)] bg-[var(--ctp-panel)] text-[var(--ctp-subtext1)] hover:border-[var(--ctp-accent)]'
          "
          title="切换只读保护模式（只读状态下禁止任何元数据写入、改名与归档修改）"
          @click="handleToggleRootReadOnly(r.id, Boolean(r.read_only))"
        >
          {{ r.read_only ? '解除只读' : '设为只读' }}
        </button>
        <button
          type="button"
          class="flex h-9 w-9 shrink-0 items-center justify-center rounded-xl text-[var(--ctp-subtext0)] transition hover:bg-red-500/10 hover:text-red-500"
          title="移除目录白名单（不会删除磁盘原文件）"
          @click="handleRemoveRoot(r.id)"
        >
          <LuTrash2 class="h-4 w-4" />
        </button>
      </div>
    </div>
  </section>

  <!-- Inbox / Watch Folder Card -->
  <section class="card-soft space-y-4 rounded-2xl bg-[var(--ctp-shell)] p-6">
    <div>
      <h2 class="text-sm font-bold text-[var(--ctp-text)]">
        热导入收件箱（Inbox / Watch Folder）
      </h2>
      <p class="mt-1 text-xs text-[var(--ctp-subtext0)]">
        将新下载的媒体文件放入收件箱，一键通过同卷硬链接（PT 保种零额外空间）或移动导入正式根目录并可选自动刮削。
      </p>
    </div>
    <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
      <label class="block">
        <span class="mb-1.5 block text-xs font-semibold text-[var(--ctp-subtext1)]">收件箱目录路径</span>
        <input
          v-model="inboxPath"
          type="text"
          placeholder="/data/inbox"
          class="h-10 w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3.5 font-mono text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
        />
      </label>
      <div class="block">
        <span class="mb-1.5 block text-xs font-semibold text-[var(--ctp-subtext1)]">目标导入根目录</span>
        <AppSelect v-model="inboxTargetRootId" :options="inboxRootOptions" />
      </div>
    </div>
    <div class="flex flex-wrap items-center gap-5">
      <AppCheckbox v-model="inboxUseHardlink" label="优先硬链接（同卷保种零复制）" />
      <AppCheckbox v-model="inboxDeleteSource" label="导入后移除收件箱源文件" />
      <AppCheckbox v-model="inboxAutoScrape" label="导入后自动刮削归档" />
    </div>
    <div v-if="inboxNotice" class="rounded-xl bg-emerald-500/10 px-3 py-2 text-xs text-emerald-500">
      {{ inboxNotice }}
    </div>
    <div class="flex flex-wrap items-center gap-2">
      <button
        type="button"
        :disabled="saving"
        class="inline-flex items-center gap-1.5 rounded-xl bg-[var(--ctp-accent)] px-4 py-2 text-xs font-semibold text-[var(--ctp-on-accent)] disabled:opacity-50"
        @click="handleSaveInboxAndWebhook"
      >
        <LuSave class="h-3.5 w-3.5" />
        <span>保存收件箱与联动配置</span>
      </button>
      <button
        type="button"
        :disabled="scanningInbox || !inboxPath.trim() || !inboxTargetRootId"
        class="inline-flex items-center gap-1.5 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-4 py-2 text-xs font-semibold text-[var(--ctp-text)] hover:border-[var(--ctp-accent)] disabled:opacity-50"
        @click="handleScanInbox"
      >
        <LuRotateCw :class="{ 'animate-spin': scanningInbox }" class="h-3.5 w-3.5" />
        <span>{{ scanningInbox ? '扫描导入中…' : '立即扫描导入收件箱' }}</span>
      </button>
    </div>
  </section>

  <!-- xmedia Main Server Webhook Card -->
  <section class="card-soft space-y-4 rounded-2xl bg-[var(--ctp-shell)] p-6">
    <div class="flex items-center justify-between">
      <div>
        <h2 class="text-sm font-bold text-[var(--ctp-text)]">
          xmedia 主站增量扫描联动（Webhook）
        </h2>
        <p class="mt-1 text-xs text-[var(--ctp-subtext0)]">
          整理工具完成重命名、刮削或收件箱导入后，自动通知 xmedia 主站对匹配的挂载媒体库执行增量扫描。
        </p>
      </div>
      <AppSwitch v-model="webhookEnabled" label="启用联动" />
    </div>
    <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
      <label class="block">
        <span class="mb-1.5 block text-xs font-semibold text-[var(--ctp-subtext1)]">主站地址 (Base URL)</span>
        <input
          v-model="webhookBaseUrl"
          type="text"
          placeholder="http://127.0.0.1:8080"
          class="h-10 w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3.5 font-mono text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
        />
      </label>
      <label class="block">
        <span class="mb-1.5 block text-xs font-semibold text-[var(--ctp-subtext1)]">
          主站管理员 API Key
          <span v-if="props.settings?.xmedia_webhook_api_key_masked" class="ml-1 font-normal text-emerald-500">
            (已保存: {{ props.settings.xmedia_webhook_api_key_masked }})
          </span>
        </span>
        <input
          v-model="webhookApiKey"
          type="password"
          placeholder="留空则保持已保存的 API Key"
          class="h-10 w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3.5 font-mono text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
        />
      </label>
    </div>
    <div class="flex flex-wrap items-center gap-5">
      <AppCheckbox v-model="webhookNotifyRename" label="批量重命名后通知" />
      <AppCheckbox v-model="webhookNotifyScrape" label="刮削落盘后通知" />
      <AppCheckbox v-model="webhookNotifyInbox" label="收件箱导入后通知" />
    </div>
    <div v-if="webhookNotice" class="rounded-xl bg-emerald-500/10 px-3 py-2 text-xs text-emerald-500">
      {{ webhookNotice }}
    </div>
    <div class="flex flex-wrap items-center gap-2">
      <button
        type="button"
        :disabled="saving"
        class="inline-flex items-center gap-1.5 rounded-xl bg-[var(--ctp-accent)] px-4 py-2 text-xs font-semibold text-[var(--ctp-on-accent)] disabled:opacity-50"
        @click="handleSaveInboxAndWebhook"
      >
        <LuSave class="h-3.5 w-3.5" />
        <span>保存联动设置</span>
      </button>
      <button
        type="button"
        :disabled="testingWebhook"
        class="inline-flex items-center gap-1.5 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-4 py-2 text-xs font-semibold text-[var(--ctp-text)] hover:border-[var(--ctp-accent)] disabled:opacity-50"
        @click="handleTestWebhook"
      >
        <LuCheckCircle2 class="h-3.5 w-3.5" />
        <span>{{ testingWebhook ? '测试中…' : '测试主站连通性' }}</span>
      </button>
    </div>
  </section>
</template>
