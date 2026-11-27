<script setup lang="ts" generic="S extends OnlineSourceSummaryDto">
import { renderSVG } from 'uqr'
import { computed, onBeforeUnmount, reactive, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  LuCheckCircle2,
  LuCookie,
  LuImage,
  LuKeyRound,
  LuLoader2,
  LuLogOut,
  LuQrCode,
  LuRefreshCw,
  LuSlidersHorizontal,
  LuSmartphone,
  LuUserCheck,
} from 'vue-icons-plus/lu'
import { MdSelect, MdSwitch, MdTabs, type MdTabItem, type SelectOption } from '@/components/ui'
import MdDialog from '@/components/ui/MdDialog.vue'
import { useSnackbarStore } from '@/stores/snackbar'
import {
  extractApiError,
  isContractAuthSupported,
  isSafeDataImageUrl,
  isSafeHttpUrl,
  type LoginActionResponseDto,
  type LoginUiItem,
  type OnlineSourceSummaryDto,
  type PluginAuthActionPayload,
  type PluginAuthActionResponseDto,
  type PluginProfileDto,
  type PluginSettingFieldDto,
  type UpdateSourcePatch,
} from './types'

type ContractAuthTab = 'qr' | 'otp' | 'password' | 'cookie'

const props = defineProps<{
  open: boolean
  domainLabel: string
  source: S | null
  getSourceVariable: (id: string) => Promise<{ variable: string }>
  runLoginAction: (
    id: string,
    loginInfo: Record<string, string>,
    action?: string,
  ) => Promise<LoginActionResponseDto<S>>
  updateSource: (id: string, patch: UpdateSourcePatch) => Promise<S>
  runPluginAuth?: (
    id: string,
    payload: PluginAuthActionPayload,
  ) => Promise<PluginAuthActionResponseDto<S>>
  updatePluginSettings?: (id: string, values: Record<string, unknown>) => Promise<S>
  buildProxyImageUrl?: (
    rawUrl: string | null | undefined,
    sourceId?: string | null,
    cacheBust?: string | number | null,
  ) => string
}>()

const emit = defineEmits<{
  close: []
  updated: [source: S]
}>()

const { t } = useI18n()
const snackbar = useSnackbarStore()

// Legacy loginUi state
const formValues = reactive<Record<string, string>>({})
const failedImageKeys = reactive<Record<string, boolean>>({})
const imageRefreshRev = ref(0)
const runningActionKey = ref<string | null>(null)
const showVarEditor = ref(false)
const rawVariableDraft = ref('')
const loadingVar = ref(false)
const varLoaded = ref(false)
const varLoadError = ref<string | null>(null)
const savingVar = ref(false)

// Plugin v2 Contract Auth state
const activeAuthTab = ref<ContractAuthTab>('qr')
const authProfile = ref<PluginProfileDto | null>(null)
const loadingStatus = ref(false)
const loggingOut = ref(false)

const qrLoading = ref(false)
const qrKey = ref('')
const qrUrl = ref('')
const qrImage = ref('')
const qrStatus = ref<'idle' | 'waiting' | 'scanned' | 'confirmed' | 'expired' | 'failed'>('idle')
const qrMessage = ref('')
let qrPollTimer: ReturnType<typeof setInterval> | null = null
let qrPollInFlight = false

const otpCountryCode = ref('86')
const otpPhone = ref('')
const otpCode = ref('')
const otpSending = ref(false)
const otpVerifying = ref(false)
const otpCooldown = ref(0)
let otpCooldownTimer: ReturnType<typeof setInterval> | null = null

const pwdUsername = ref('')
const pwdPassword = ref('')
const pwdSubmitting = ref(false)

const cookieInput = ref('')
const cookieSubmitting = ref(false)

// Declarative Plugin Settings state
const settingsDraft = reactive<Record<string, unknown>>({})
const savingSettings = ref(false)

let actionSeq = 0
let varLoadSeq = 0
let authSeq = 0

const hasContractAuth = computed(
  () => Boolean(props.runPluginAuth && isContractAuthSupported(props.source?.auth)),
)

const settingsSchema = computed<PluginSettingFieldDto[]>(
  () => props.source?.settings_schema ?? [],
)

const hasDeclarativeSettings = computed(
  () => Boolean(props.updatePluginSettings && settingsSchema.value.length > 0),
)

const contractAuthTabs = computed<MdTabItem[]>(() => {
  const auth = props.source?.auth
  if (!auth) return []
  const items: MdTabItem[] = []
  if (auth.qr_code) {
    items.push({
      value: 'qr',
      label: t('onlineSources.login.tabQr'),
      icon: LuQrCode,
    })
  }
  if (auth.otp) {
    items.push({
      value: 'otp',
      label: t('onlineSources.login.tabOtp'),
      icon: LuSmartphone,
    })
  }
  if (auth.password) {
    items.push({
      value: 'password',
      label: t('onlineSources.login.tabPassword'),
      icon: LuKeyRound,
    })
  }
  if (auth.cookie) {
    items.push({
      value: 'cookie',
      label: t('onlineSources.login.tabCookie'),
      icon: LuCookie,
    })
  }
  return items
})

function stopQrPoll(): void {
  if (qrPollTimer !== null) {
    clearInterval(qrPollTimer)
    qrPollTimer = null
  }
  qrPollInFlight = false
}

function stopOtpCountdown(): void {
  if (otpCooldownTimer !== null) {
    clearInterval(otpCooldownTimer)
    otpCooldownTimer = null
  }
  otpCooldown.value = 0
}

onBeforeUnmount(() => {
  stopQrPoll()
  stopOtpCountdown()
})

function startOtpCountdown(seconds: number): void {
  stopOtpCountdown()
  const initial = Math.max(1, Math.min(300, Math.trunc(seconds || 60)))
  otpCooldown.value = initial
  otpCooldownTimer = setInterval(() => {
    if (otpCooldown.value <= 1) {
      stopOtpCountdown()
    } else {
      otpCooldown.value -= 1
    }
  }, 1000)
}

function normalizeItemType(item: LoginUiItem): string {
  return (item.type ?? '').trim().toLowerCase()
}

function isTextOrPasswordItem(item: LoginUiItem): boolean {
  const kind = normalizeItemType(item)
  return kind === 'text' || kind === 'password'
}

function isImageItem(item: LoginUiItem): boolean {
  const kind = normalizeItemType(item)
  return kind === 'image' || kind === 'img'
}

function isQrCodeItem(item: LoginUiItem): boolean {
  return normalizeItemType(item) === 'qrcode'
}

function isButtonItem(item: LoginUiItem): boolean {
  return normalizeItemType(item) === 'button'
}

const hasCustomActionButtons = computed(() =>
  Boolean(
    props.source?.login_ui.some(
      (item) => (isButtonItem(item) || isImageItem(item) || isQrCodeItem(item)) && item.action?.trim(),
    ),
  ),
)

const hasInputFields = computed(() =>
  Boolean(props.source?.login_ui.some((item) => isTextOrPasswordItem(item))),
)

const showFallbackLoginSubmit = computed(
  () =>
    !hasContractAuth.value &&
    Boolean(props.source && (props.source.has_login_url || hasInputFields.value)) &&
    !hasCustomActionButtons.value,
)

function clearFailedImageKeys(): void {
  for (const k of Object.keys(failedImageKeys)) {
    delete failedImageKeys[k]
  }
}

function hydrateSettingsFromSource(src: S): void {
  for (const k of Object.keys(settingsDraft)) {
    delete settingsDraft[k]
  }
  const values = src.settings_values ?? {}
  for (const field of src.settings_schema ?? []) {
    const raw = values[field.key] !== undefined ? values[field.key] : field.default_value
    if (field.type === 'switch') {
      settingsDraft[field.key] = Boolean(raw)
    } else if (raw === null || raw === undefined) {
      settingsDraft[field.key] = ''
    } else {
      settingsDraft[field.key] = String(raw)
    }
  }
}

function resetEditorState(): void {
  actionSeq += 1
  varLoadSeq += 1
  authSeq += 1
  stopQrPoll()
  stopOtpCountdown()

  for (const k of Object.keys(formValues)) {
    delete formValues[k]
  }
  for (const k of Object.keys(settingsDraft)) {
    delete settingsDraft[k]
  }
  clearFailedImageKeys()
  imageRefreshRev.value = 0
  runningActionKey.value = null
  showVarEditor.value = false
  rawVariableDraft.value = ''
  loadingVar.value = false
  varLoaded.value = false
  varLoadError.value = null
  savingVar.value = false

  authProfile.value = null
  loadingStatus.value = false
  loggingOut.value = false
  qrLoading.value = false
  qrKey.value = ''
  qrUrl.value = ''
  qrImage.value = ''
  qrStatus.value = 'idle'
  qrMessage.value = ''
  otpCountryCode.value = '86'
  otpPhone.value = ''
  otpCode.value = ''
  otpSending.value = false
  otpVerifying.value = false
  pwdUsername.value = ''
  pwdPassword.value = ''
  pwdSubmitting.value = false
  cookieInput.value = ''
  cookieSubmitting.value = false
  savingSettings.value = false
}

async function loadFullVariable(sourceId: string): Promise<void> {
  const seq = ++varLoadSeq
  loadingVar.value = true
  varLoadError.value = null
  try {
    const res = await props.getSourceVariable(sourceId)
    if (seq !== varLoadSeq || !props.open || props.source?.id !== sourceId) return
    rawVariableDraft.value = res.variable ?? ''
    varLoaded.value = true
  } catch (err) {
    if (seq !== varLoadSeq || !props.open || props.source?.id !== sourceId) return
    varLoadError.value = extractApiError(err)
    varLoaded.value = false
  } finally {
    if (seq === varLoadSeq) {
      loadingVar.value = false
    }
  }
}

function startQrPollingLoop(sourceId: string): void {
  stopQrPoll()
  qrPollTimer = setInterval(() => {
    if (
      !props.open ||
      props.source?.id !== sourceId ||
      activeAuthTab.value !== 'qr' ||
      !qrKey.value ||
      (qrStatus.value !== 'waiting' && qrStatus.value !== 'scanned')
    ) {
      stopQrPoll()
      return
    }
    if (qrPollInFlight || !props.runPluginAuth) return
    qrPollInFlight = true
    const currentSeq = authSeq
    const key = qrKey.value
    void props
      .runPluginAuth(sourceId, { action: 'qr_poll', key })
      .then((res) => {
        if (currentSeq !== authSeq || !props.open || props.source?.id !== sourceId) return
        const st = (res.status ?? '').toLowerCase()
        if (res.message) qrMessage.value = res.message
        if (st === 'scanned') {
          qrStatus.value = 'scanned'
        } else if (st === 'confirmed' || (res.ok && res.profile?.logged_in)) {
          qrStatus.value = 'confirmed'
          stopQrPoll()
          if (res.profile) authProfile.value = res.profile
          emit('updated', res.source)
          snackbar.show({
            message: res.message || t('onlineSources.login.authSuccessToast'),
          })
        } else if (st === 'expired') {
          qrStatus.value = 'expired'
          stopQrPoll()
        } else if (st === 'failed') {
          qrStatus.value = 'failed'
          stopQrPoll()
        }
      })
      .catch(() => {
        // transient network hiccup during polling is ignored
      })
      .finally(() => {
        qrPollInFlight = false
      })
  }, 2000)
}

async function startQrLogin(): Promise<void> {
  const src = props.source
  if (!src || !props.runPluginAuth || !src.auth?.qr_code || qrLoading.value) return
  stopQrPoll()
  const sourceId = src.id
  const seq = ++authSeq
  qrLoading.value = true
  qrMessage.value = ''
  try {
    const res = await props.runPluginAuth(sourceId, { action: 'qr_create' })
    if (seq !== authSeq || !props.open || props.source?.id !== sourceId) return
    qrKey.value = res.qr_key ?? ''
    qrUrl.value = res.qr_url ?? ''
    qrImage.value = res.qr_image ?? ''
    qrMessage.value = res.message ?? ''
    if (res.ok && (qrUrl.value || qrImage.value)) {
      qrStatus.value = 'waiting'
      if (qrKey.value && activeAuthTab.value === 'qr') {
        startQrPollingLoop(sourceId)
      }
    } else {
      qrStatus.value = 'failed'
    }
  } catch (err) {
    if (seq !== authSeq || !props.open || props.source?.id !== sourceId) return
    qrStatus.value = 'failed'
    qrMessage.value = extractApiError(err)
  } finally {
    if (seq === authSeq) {
      qrLoading.value = false
    }
  }
}

async function initContractAuth(src: S): Promise<void> {
  if (!props.runPluginAuth || !src.auth) return
  const tabs = contractAuthTabs.value
  if (tabs.length > 0) {
    activeAuthTab.value = tabs[0]!.value as ContractAuthTab
  }

  const sourceId = src.id
  const seq = ++authSeq
  if (src.auth.has_profile) {
    loadingStatus.value = true
    try {
      const res = await props.runPluginAuth(sourceId, { action: 'status' })
      if (seq !== authSeq || !props.open || props.source?.id !== sourceId) return
      authProfile.value = res.profile ?? null
      emit('updated', res.source)
    } catch {
      // ignore initial profile check error
    } finally {
      if (seq === authSeq) {
        loadingStatus.value = false
      }
    }
  }

  if (
    seq === authSeq &&
    props.open &&
    props.source?.id === sourceId &&
    src.auth.qr_code &&
    activeAuthTab.value === 'qr' &&
    !authProfile.value?.logged_in
  ) {
    void startQrLogin()
  }
}

function handleSwitchAuthTab(nextVal: string): void {
  const next = nextVal as ContractAuthTab
  if (next === activeAuthTab.value) return
  activeAuthTab.value = next
  if (next !== 'qr') {
    stopQrPoll()
  } else if (props.source?.auth?.qr_code && !qrUrl.value && !qrImage.value && !qrLoading.value) {
    void startQrLogin()
  } else if (
    props.source &&
    qrKey.value &&
    (qrStatus.value === 'waiting' || qrStatus.value === 'scanned')
  ) {
    startQrPollingLoop(props.source.id)
  }
}

async function handleSendOtp(): Promise<void> {
  const src = props.source
  const phone = otpPhone.value.trim()
  if (!src || !props.runPluginAuth || !phone || otpSending.value || otpCooldown.value > 0) return
  const sourceId = src.id
  const seq = ++authSeq
  otpSending.value = true
  try {
    const res = await props.runPluginAuth(sourceId, {
      action: 'otp_send',
      country_code: otpCountryCode.value.trim() || '86',
      phone,
    })
    if (seq !== authSeq || !props.open || props.source?.id !== sourceId) return
    if (res.ok) {
      startOtpCountdown(res.cooldown_secs ?? 60)
      snackbar.show({
        message: res.message || t('onlineSources.login.otpSentToast'),
      })
    } else {
      snackbar.show({
        message: res.message || t('onlineSources.login.authFailedToast'),
        tone: 'error',
      })
    }
  } catch (err) {
    if (seq !== authSeq || !props.open || props.source?.id !== sourceId) return
    snackbar.show({ message: extractApiError(err), tone: 'error' })
  } finally {
    if (seq === authSeq) {
      otpSending.value = false
    }
  }
}

async function handleVerifyOtp(): Promise<void> {
  const src = props.source
  const phone = otpPhone.value.trim()
  const code = otpCode.value.trim()
  if (!src || !props.runPluginAuth || !phone || !code || otpVerifying.value) return
  const sourceId = src.id
  const seq = ++authSeq
  otpVerifying.value = true
  try {
    const res = await props.runPluginAuth(sourceId, {
      action: 'otp_verify',
      country_code: otpCountryCode.value.trim() || '86',
      phone,
      code,
    })
    if (seq !== authSeq || !props.open || props.source?.id !== sourceId) return
    emit('updated', res.source)
    if (res.ok) {
      if (res.profile) authProfile.value = res.profile
      otpCode.value = ''
      snackbar.show({
        message: res.message || t('onlineSources.login.authSuccessToast'),
      })
    } else {
      snackbar.show({
        message: res.message || t('onlineSources.login.authFailedToast'),
        tone: 'error',
      })
    }
  } catch (err) {
    if (seq !== authSeq || !props.open || props.source?.id !== sourceId) return
    snackbar.show({ message: extractApiError(err), tone: 'error' })
  } finally {
    if (seq === authSeq) {
      otpVerifying.value = false
    }
  }
}

async function handlePasswordLogin(): Promise<void> {
  const src = props.source
  const username = pwdUsername.value.trim()
  const password = pwdPassword.value
  if (!src || !props.runPluginAuth || !username || !password || pwdSubmitting.value) return
  const sourceId = src.id
  const seq = ++authSeq
  pwdSubmitting.value = true
  try {
    const res = await props.runPluginAuth(sourceId, {
      action: 'password_login',
      username,
      password,
    })
    if (seq !== authSeq || !props.open || props.source?.id !== sourceId) return
    emit('updated', res.source)
    if (res.ok) {
      if (res.profile) authProfile.value = res.profile
      pwdPassword.value = ''
      snackbar.show({
        message: res.message || t('onlineSources.login.authSuccessToast'),
      })
    } else {
      snackbar.show({
        message: res.message || t('onlineSources.login.authFailedToast'),
        tone: 'error',
      })
    }
  } catch (err) {
    if (seq !== authSeq || !props.open || props.source?.id !== sourceId) return
    snackbar.show({ message: extractApiError(err), tone: 'error' })
  } finally {
    if (seq === authSeq) {
      pwdSubmitting.value = false
    }
  }
}

async function handleCookieLogin(): Promise<void> {
  const src = props.source
  const cookie = cookieInput.value.trim()
  if (!src || !props.runPluginAuth || !cookie || cookieSubmitting.value) return
  const sourceId = src.id
  const seq = ++authSeq
  cookieSubmitting.value = true
  try {
    const res = await props.runPluginAuth(sourceId, {
      action: 'cookie_login',
      cookie,
    })
    if (seq !== authSeq || !props.open || props.source?.id !== sourceId) return
    emit('updated', res.source)
    if (res.ok) {
      if (res.profile) authProfile.value = res.profile
      cookieInput.value = ''
      snackbar.show({
        message: res.message || t('onlineSources.login.authSuccessToast'),
      })
    } else {
      snackbar.show({
        message: res.message || t('onlineSources.login.authFailedToast'),
        tone: 'error',
      })
    }
  } catch (err) {
    if (seq !== authSeq || !props.open || props.source?.id !== sourceId) return
    snackbar.show({ message: extractApiError(err), tone: 'error' })
  } finally {
    if (seq === authSeq) {
      cookieSubmitting.value = false
    }
  }
}

async function handleContractLogout(): Promise<void> {
  const src = props.source
  if (!src || !props.runPluginAuth || loggingOut.value) return
  const sourceId = src.id
  const seq = ++authSeq
  loggingOut.value = true
  try {
    const res = await props.runPluginAuth(sourceId, { action: 'logout' })
    if (seq !== authSeq || !props.open || props.source?.id !== sourceId) return
    authProfile.value = res.profile ?? { logged_in: false }
    emit('updated', res.source)
    snackbar.show({
      message: res.message || t('onlineSources.login.loggedOutToast'),
    })
    if (src.auth?.qr_code && activeAuthTab.value === 'qr') {
      void startQrLogin()
    }
  } catch (err) {
    if (seq !== authSeq || !props.open || props.source?.id !== sourceId) return
    snackbar.show({ message: extractApiError(err), tone: 'error' })
  } finally {
    if (seq === authSeq) {
      loggingOut.value = false
    }
  }
}

async function handleSaveSettings(): Promise<void> {
  const src = props.source
  if (!src || !props.updatePluginSettings || savingSettings.value) return
  const sourceId = src.id
  savingSettings.value = true
  try {
    const payload: Record<string, unknown> = {}
    for (const field of settingsSchema.value) {
      payload[field.key] = settingsDraft[field.key]
    }
    const updated = await props.updatePluginSettings(sourceId, payload)
    if (!props.open || props.source?.id !== sourceId) return
    hydrateSettingsFromSource(updated)
    emit('updated', updated)
    snackbar.show({ message: t('onlineSources.login.settingsSavedToast') })
  } catch (err) {
    if (!props.open || props.source?.id !== sourceId) return
    snackbar.show({ message: extractApiError(err), tone: 'error' })
  } finally {
    savingSettings.value = false
  }
}

function selectOptionsFor(field: PluginSettingFieldDto): SelectOption[] {
  return (field.options ?? []).map((opt) => ({
    label: opt.label,
    value: opt.value,
  }))
}

function resolveAvatarUrl(raw: string | null | undefined): string {
  if (!raw || !raw.trim()) return ''
  const u = raw.trim()
  if (u.toLowerCase().startsWith('data:')) {
    return isSafeDataImageUrl(u) ? u : ''
  }
  if (!isSafeHttpUrl(u)) return ''
  if (props.buildProxyImageUrl) {
    return props.buildProxyImageUrl(u, props.source?.id ?? null)
  }
  return u
}

const contractQrSvg = computed<string>(() => {
  if (!qrUrl.value || qrUrl.value.toLowerCase().startsWith('data:')) return ''
  try {
    return renderSVG(qrUrl.value, {
      ecc: 'M',
      border: 2,
      whiteColor: '#FFFFFF',
      blackColor: '#111318',
    })
  } catch {
    return ''
  }
})

const contractQrDataImage = computed<string>(() => {
  const candidate = qrImage.value || qrUrl.value
  return isSafeDataImageUrl(candidate) ? candidate : ''
})

watch(
  () => [props.open, props.source?.id] as const,
  ([isOpen, srcId], prev) => {
    const src = props.source
    if (!isOpen || !src || !srcId) {
      resetEditorState()
      return
    }
    const wasOpen = prev?.[0] ?? false
    const prevId = prev?.[1]
    if (wasOpen && prevId === srcId) return

    resetEditorState()

    for (const item of src.login_ui) {
      if (isTextOrPasswordItem(item)) {
        formValues[item.name] = src.login_info[item.name] ?? ''
      }
    }
    hydrateSettingsFromSource(src)
    if (hasContractAuth.value) {
      void initContractAuth(src)
    }
  },
  { immediate: true },
)

function fieldInputId(idx: number): string {
  const sid = props.source?.id ?? 'src'
  return `online-source-login-${sid}-${idx}`
}

function actionKeyFor(item: LoginUiItem, idx: number): string {
  return `${idx}:${item.name}`
}

function toggleVarEditor(): void {
  showVarEditor.value = !showVarEditor.value
  if (showVarEditor.value && props.source && !varLoaded.value && !loadingVar.value) {
    void loadFullVariable(props.source.id)
  }
}

function fieldSpanClass(item: LoginUiItem): string {
  const basis = item.style?.layout_flex_basis_percent
  if (basis === undefined || basis === null || basis >= 0.85) return 'col-span-4'
  if (basis >= 0.4) return 'col-span-4 sm:col-span-2'
  return 'col-span-2 sm:col-span-1'
}

function buttonSpanClass(item: LoginUiItem): string {
  const basis = item.style?.layout_flex_basis_percent ?? 0.5
  if (basis >= 0.85) return 'col-span-4'
  if (basis >= 0.4) return 'col-span-2'
  return 'col-span-1'
}

function isDividerButton(item: LoginUiItem): boolean {
  return isButtonItem(item) && (!item.action || !item.action.trim())
}

function rawMediaValueFor(item: LoginUiItem): string {
  if (!props.source) return ''
  return (props.source.login_info[item.name] ?? item.url ?? '').trim()
}

function resolveDisplayImageUrl(item: LoginUiItem): string {
  const raw = rawMediaValueFor(item)
  if (!raw) return ''
  if (raw.toLowerCase().startsWith('data:')) {
    return isSafeDataImageUrl(raw) ? raw : ''
  }
  if (!isSafeHttpUrl(raw)) return ''
  if (props.buildProxyImageUrl) {
    return props.buildProxyImageUrl(raw, props.source?.id ?? null, imageRefreshRev.value)
  }
  return raw
}

function renderQrSvgFor(item: LoginUiItem): string {
  const raw = rawMediaValueFor(item)
  if (!raw || raw.toLowerCase().startsWith('data:')) return ''
  try {
    return renderSVG(raw, {
      ecc: 'M',
      border: 2,
      whiteColor: '#FFFFFF',
      blackColor: '#111318',
    })
  } catch {
    return ''
  }
}

function markImageFailed(idx: number): void {
  failedImageKeys[String(idx)] = true
}

function clearImageFailed(idx: number): void {
  delete failedImageKeys[String(idx)]
}

function isImageFailed(idx: number): boolean {
  return Boolean(failedImageKeys[String(idx)])
}

function openExternalUrlSafely(url: string): void {
  if (!isSafeHttpUrl(url)) {
    snackbar.show({ message: t('onlineSources.login.blockedExternalUrl'), tone: 'error' })
    return
  }
  window.open(url.trim(), '_blank', 'noopener,noreferrer')
}

async function executeLoginCall(
  actionLabel: string,
  actionKey: string,
  action?: string,
): Promise<void> {
  const src = props.source
  if (!src || runningActionKey.value !== null) return
  const sourceId = src.id
  const seq = ++actionSeq
  runningActionKey.value = actionKey
  try {
    const resp = await props.runLoginAction(sourceId, { ...formValues }, action)
    if (seq !== actionSeq || !props.open || props.source?.id !== sourceId) return

    varLoaded.value = false
    if (showVarEditor.value) {
      void loadFullVariable(sourceId)
    }
    clearFailedImageKeys()
    imageRefreshRev.value += 1
    emit('updated', resp.source)

    const toasts = resp.diagnostics.toasts.map((v) => v.trim()).filter(Boolean)
    const rawLinks = resp.diagnostics.browser_urls
    const safeLink = rawLinks.find((link) => isSafeHttpUrl(link.url))
    const hasBlockedLink = rawLinks.length > 0 && !safeLink

    if (toasts.length > 0) {
      const finalMsg = toasts[toasts.length - 1]!
      snackbar.show({
        message: finalMsg,
        actionLabel: safeLink ? safeLink.title || t('onlineSources.login.openLink') : undefined,
        onAction: safeLink ? () => openExternalUrlSafely(safeLink.url) : undefined,
      })
    } else if (safeLink) {
      snackbar.show({
        message: t('onlineSources.login.openExternalPage', { name: actionLabel }),
        actionLabel: safeLink.title || t('onlineSources.login.openLink'),
        onAction: () => openExternalUrlSafely(safeLink.url),
      })
    } else if (hasBlockedLink) {
      snackbar.show({
        message: t('onlineSources.login.blockedRedirectUrl', { name: actionLabel }),
        tone: 'error',
      })
    } else if (resp.result) {
      snackbar.show({ message: `${actionLabel}: ${resp.result}` })
    } else {
      snackbar.show({
        message: t('onlineSources.login.executedAction', { name: actionLabel }),
      })
    }
  } catch (err) {
    if (seq !== actionSeq || !props.open || props.source?.id !== sourceId) return
    snackbar.show({ message: extractApiError(err), tone: 'error' })
  } finally {
    if (seq === actionSeq) {
      runningActionKey.value = null
    }
  }
}

async function triggerAction(item: LoginUiItem, idx: number): Promise<void> {
  const act = item.action?.trim()
  if (!act) return
  await executeLoginCall(item.name, actionKeyFor(item, idx), act)
}

async function triggerFallbackLogin(): Promise<void> {
  await executeLoginCall(t('onlineSources.login.submitLoginBtn'), '__default_login__')
}

async function saveRawVariable(): Promise<void> {
  const src = props.source
  if (!src || savingVar.value || loadingVar.value || !varLoaded.value || varLoadError.value) return
  const sourceId = src.id
  const seq = varLoadSeq
  savingVar.value = true
  try {
    const updated = await props.updateSource(sourceId, {
      variable: rawVariableDraft.value,
    })
    if (seq !== varLoadSeq || !props.open || props.source?.id !== sourceId) return
    emit('updated', updated)
    showVarEditor.value = false
    snackbar.show({ message: t('onlineSources.login.variableSaved') })
  } catch (err) {
    if (seq !== varLoadSeq || !props.open || props.source?.id !== sourceId) return
    snackbar.show({ message: extractApiError(err), tone: 'error' })
  } finally {
    if (seq === varLoadSeq) {
      savingVar.value = false
    }
  }
}
</script>

<template>
  <MdDialog
    :open="open && source !== null"
    max-width="max-w-2xl"
    @close="emit('close')"
  >
    <template #header>
      <div v-if="source" class="flex min-w-0 items-start gap-3.5">
        <div
          class="flex h-10 w-10 shrink-0 items-center justify-center rounded-corner-lg bg-primary-container text-on-primary-container"
        >
          <LuKeyRound class="h-5 w-5" />
        </div>
        <div class="min-w-0">
          <div class="flex flex-wrap items-center gap-2">
            <h3 class="text-title-medium truncate text-on-surface">
              {{ t('onlineSources.login.title', { domain: domainLabel, name: source.name }) }}
            </h3>
            <span
              v-if="authProfile?.logged_in"
              class="text-label-small inline-flex shrink-0 items-center gap-1 rounded-corner-full bg-success-container px-2 py-0.5 text-on-success-container"
            >
              <LuCheckCircle2 class="h-3 w-3" />
              <span>{{ t('onlineSources.login.loggedInBadge') }}</span>
            </span>
            <span
              v-else-if="source.cookie_hosts.length > 0"
              class="text-label-small inline-flex shrink-0 items-center gap-1 rounded-corner-full bg-success-container px-2 py-0.5 text-on-success-container"
            >
              <LuCheckCircle2 class="h-3 w-3" />
              <span>{{ t('onlineSources.login.cookieStored', { count: source.cookie_hosts.length }) }}</span>
            </span>
          </div>
          <p class="mt-0.5 truncate text-xs text-[var(--md-sys-color-on-surface-variant)]">
            <template v-if="source.auth?.description">
              {{ source.auth.description }}
            </template>
            <template v-else-if="source.cookie_hosts.length > 0">
              {{
                t('onlineSources.login.cookieHostsPrefix', {
                  hosts: source.cookie_hosts.slice(0, 3).join(', '),
                })
              }}{{ source.cookie_hosts.length > 3 ? t('onlineSources.login.cookieHostsMore') : '' }}
            </template>
            <template v-else>
              {{ t('onlineSources.login.subtitleEmpty') }}
            </template>
          </p>
        </div>
      </div>
    </template>

    <div v-if="source" class="flex flex-col gap-4 py-1">
      <!-- 1. Plugin v2 Contract Auth Section -->
      <template v-if="hasContractAuth">
        <!-- Profile Status Card -->
        <div
          v-if="loadingStatus"
          class="text-body-small flex items-center gap-2 rounded-corner-lg bg-surface-container px-4 py-3 text-on-surface-variant"
        >
          <LuLoader2 class="h-4 w-4 animate-spin" />
          <span>{{ t('onlineSources.login.checkingStatus') }}</span>
        </div>

        <div
          v-else-if="authProfile?.logged_in"
          data-plugin-profile-card
          class="flex flex-wrap items-center justify-between gap-3 rounded-corner-lg bg-surface-container-high px-4 py-3.5"
        >
          <div class="flex min-w-0 items-center gap-3">
            <img
              v-if="resolveAvatarUrl(authProfile.avatar_url)"
              :src="resolveAvatarUrl(authProfile.avatar_url)"
              alt=""
              class="h-10 w-10 shrink-0 rounded-corner-full object-cover"
            />
            <div
              v-else
              class="flex h-10 w-10 shrink-0 items-center justify-center rounded-corner-full bg-primary-container text-on-primary-container"
            >
              <LuUserCheck class="h-5 w-5" />
            </div>

            <div class="min-w-0">
              <div class="flex flex-wrap items-center gap-2">
                <span class="text-title-small truncate text-on-surface">
                  {{
                    authProfile.nickname ||
                    authProfile.user_id ||
                    t('onlineSources.login.loggedInBadge')
                  }}
                </span>
                <span
                  v-if="authProfile.vip_label"
                  class="text-label-small rounded-corner-full bg-tertiary-container px-2 py-0.5 text-on-tertiary-container"
                >
                  {{ authProfile.vip_label }}
                </span>
              </div>
              <p
                v-if="authProfile.user_id || authProfile.message"
                class="text-body-small truncate text-on-surface-variant"
              >
                {{
                  [
                    authProfile.user_id ? `ID: ${authProfile.user_id}` : '',
                    authProfile.message || '',
                  ]
                    .filter(Boolean)
                    .join(' · ')
                }}
              </p>
            </div>
          </div>

          <button
            v-if="source.auth?.has_logout"
            type="button"
            :disabled="loggingOut"
            class="md-btn md-btn-tonal md-state shrink-0"
            @click="handleContractLogout"
          >
            <LuLoader2 v-if="loggingOut" class="h-4 w-4 animate-spin" />
            <LuLogOut v-else class="h-4 w-4" />
            <span>{{ t('onlineSources.login.logoutBtn') }}</span>
          </button>
        </div>

        <!-- Auth Method Tabs & Forms -->
        <div
          v-if="contractAuthTabs.length > 0"
          class="flex flex-col gap-3.5 rounded-corner-lg bg-surface-container p-4"
        >
          <MdTabs
            v-if="contractAuthTabs.length > 1"
            :model-value="activeAuthTab"
            :tabs="contractAuthTabs"
            variant="secondary"
            @update:model-value="handleSwitchAuthTab"
          />

          <!-- QR Code Tab -->
          <div
            v-if="activeAuthTab === 'qr' && source.auth?.qr_code"
            data-contract-auth-qr
            class="flex flex-col items-center justify-center gap-3 py-2 text-center"
          >
            <div class="flex items-center justify-center rounded-corner-lg bg-white p-3 shadow-elevation-1">
              <div
                v-if="qrLoading"
                class="flex h-44 w-44 items-center justify-center text-on-surface-variant"
              >
                <LuLoader2 class="h-7 w-7 animate-spin text-primary" />
              </div>
              <img
                v-else-if="contractQrDataImage"
                :src="contractQrDataImage"
                alt="QR Code"
                class="h-44 w-44 object-contain select-none"
              />
              <!-- eslint-disable-next-line vue/no-v-html -->
              <div
                v-else-if="contractQrSvg"
                data-qrcode-svg
                class="h-44 w-44 [&>svg]:block [&>svg]:h-full [&>svg]:w-full"
                v-html="contractQrSvg"
              />
              <div
                v-else
                class="flex h-44 w-44 flex-col items-center justify-center gap-2 px-3 text-on-surface-variant"
              >
                <LuQrCode class="h-8 w-8 opacity-60" />
                <span class="text-xs">{{ t('onlineSources.login.qrGenerateHint') }}</span>
              </div>
            </div>

            <div class="flex flex-wrap items-center justify-center gap-2">
              <span
                class="text-label-small rounded-corner-full px-2.5 py-0.5"
                :class="
                  qrStatus === 'confirmed'
                    ? 'bg-success-container text-on-success-container'
                    : qrStatus === 'expired' || qrStatus === 'failed'
                      ? 'bg-error-container text-on-error-container'
                      : 'bg-surface-container-high text-on-surface-variant'
                "
              >
                {{
                  qrMessage ||
                  (qrStatus === 'scanned'
                    ? t('onlineSources.login.qrScannedHint')
                    : qrStatus === 'confirmed'
                      ? t('onlineSources.login.authSuccessToast')
                      : qrStatus === 'expired'
                        ? t('onlineSources.login.qrExpiredHint')
                        : t('onlineSources.login.qrcodeScanHint'))
                }}
              </span>

              <button
                type="button"
                :disabled="qrLoading"
                class="md-btn md-btn-text md-state px-2.5 text-xs"
                @click="startQrLogin"
              >
                <LuRefreshCw class="h-3.5 w-3.5" :class="qrLoading ? 'animate-spin' : ''" />
                <span>{{ t('onlineSources.login.clickToRefresh') }}</span>
              </button>
            </div>
          </div>

          <!-- SMS OTP Tab -->
          <form
            v-else-if="activeAuthTab === 'otp' && source.auth?.otp"
            data-contract-auth-otp
            class="flex flex-col gap-3"
            @submit.prevent="handleVerifyOtp"
          >
            <div class="grid grid-cols-4 gap-2.5">
              <div class="col-span-1">
                <label class="mb-1 block text-xs text-on-surface-variant">
                  {{ t('onlineSources.login.countryCodeLabel') }}
                </label>
                <input
                  v-model="otpCountryCode"
                  type="text"
                  inputmode="numeric"
                  class="md-input h-10 w-full text-sm"
                  placeholder="86"
                />
              </div>
              <div class="col-span-3">
                <label class="mb-1 block text-xs text-on-surface-variant">
                  {{ t('onlineSources.login.phoneLabel') }}
                </label>
                <input
                  v-model="otpPhone"
                  type="tel"
                  inputmode="tel"
                  class="md-input h-10 w-full text-sm"
                  :placeholder="t('onlineSources.login.phonePlaceholder')"
                />
              </div>
            </div>

            <div class="flex items-end gap-2.5">
              <div class="min-w-0 flex-1">
                <label class="mb-1 block text-xs text-on-surface-variant">
                  {{ t('onlineSources.login.otpCodeLabel') }}
                </label>
                <input
                  v-model="otpCode"
                  type="text"
                  inputmode="numeric"
                  autocomplete="one-time-code"
                  class="md-input h-10 w-full text-sm"
                  :placeholder="t('onlineSources.login.otpCodePlaceholder')"
                />
              </div>
              <button
                type="button"
                :disabled="!otpPhone.trim() || otpSending || otpCooldown > 0"
                class="md-btn md-btn-tonal md-state h-10 shrink-0"
                @click="handleSendOtp"
              >
                <LuLoader2 v-if="otpSending" class="h-4 w-4 animate-spin" />
                <span>
                  {{
                    otpCooldown > 0
                      ? t('onlineSources.login.sendOtpCooldownBtn', { secs: otpCooldown })
                      : t('onlineSources.login.sendOtpBtn')
                  }}
                </span>
              </button>
            </div>

            <button
              type="submit"
              :disabled="!otpPhone.trim() || !otpCode.trim() || otpVerifying"
              class="md-btn md-btn-filled md-state mt-1 w-full justify-center"
            >
              <LuLoader2 v-if="otpVerifying" class="h-4 w-4 animate-spin" />
              <span>{{ t('onlineSources.login.submitLoginBtn') }}</span>
            </button>
          </form>

          <!-- Username + Password Tab -->
          <form
            v-else-if="activeAuthTab === 'password' && source.auth?.password"
            data-contract-auth-password
            class="flex flex-col gap-3"
            @submit.prevent="handlePasswordLogin"
          >
            <div>
              <label class="mb-1 block text-xs text-on-surface-variant">
                {{ t('auth.username') }}
              </label>
              <input
                v-model="pwdUsername"
                type="text"
                autocomplete="off"
                class="md-input h-10 w-full text-sm"
                :placeholder="t('onlineSources.login.accountPlaceholder')"
              />
            </div>
            <div>
              <label class="mb-1 block text-xs text-on-surface-variant">
                {{ t('auth.password') }}
              </label>
              <input
                v-model="pwdPassword"
                type="password"
                autocomplete="off"
                class="md-input h-10 w-full text-sm"
                :placeholder="t('onlineSources.login.passwordPlaceholder')"
              />
            </div>
            <button
              type="submit"
              :disabled="!pwdUsername.trim() || !pwdPassword || pwdSubmitting"
              class="md-btn md-btn-filled md-state mt-1 w-full justify-center"
            >
              <LuLoader2 v-if="pwdSubmitting" class="h-4 w-4 animate-spin" />
              <span>{{ t('onlineSources.login.submitLoginBtn') }}</span>
            </button>
          </form>

          <!-- Cookie Tab -->
          <form
            v-else-if="activeAuthTab === 'cookie' && source.auth?.cookie"
            data-contract-auth-cookie
            class="flex flex-col gap-2.5"
            @submit.prevent="handleCookieLogin"
          >
            <p class="text-body-small text-on-surface-variant">
              {{ source.auth.cookie_hint || t('onlineSources.login.cookieHintDefault') }}
            </p>
            <textarea
              v-model="cookieInput"
              rows="4"
              class="md-input w-full py-2 font-mono text-xs"
              placeholder="MUSIC_U=...; __csrf=...;"
            />
            <button
              type="submit"
              :disabled="!cookieInput.trim() || cookieSubmitting"
              class="md-btn md-btn-filled md-state w-full justify-center"
            >
              <LuLoader2 v-if="cookieSubmitting" class="h-4 w-4 animate-spin" />
              <span>{{ t('onlineSources.login.saveCookieBtn') }}</span>
            </button>
          </form>
        </div>
      </template>

      <!-- 2. Legacy Dynamic loginUi Controls -->
      <template v-else>
        <div
          v-if="source.login_ui.length > 0"
          class="grid grid-cols-4 gap-2.5"
        >
          <template v-for="(item, idx) in source.login_ui" :key="`ui-${idx}-${item.name}`">
            <div
              v-if="isTextOrPasswordItem(item)"
              :class="fieldSpanClass(item)"
            >
              <label
                :for="fieldInputId(idx)"
                class="mb-1 block text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]"
              >
                {{ item.name }}
              </label>
              <input
                :id="fieldInputId(idx)"
                v-model="formValues[item.name]"
                :type="normalizeItemType(item) === 'password' ? 'password' : 'text'"
                :autocomplete="normalizeItemType(item) === 'password' ? 'off' : undefined"
                class="md-input h-9 w-full text-xs"
                :placeholder="t('onlineSources.login.inputPlaceholder', { name: item.name })"
              />
            </div>

            <div
              v-else-if="isQrCodeItem(item)"
              data-login-ui-qrcode
              :class="fieldSpanClass(item)"
            >
              <div class="mb-1 flex items-center justify-between gap-2">
                <span class="text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">
                  {{ item.name }}
                </span>
                <button
                  v-if="item.action?.trim()"
                  type="button"
                  :disabled="runningActionKey !== null"
                  class="inline-flex cursor-pointer items-center gap-1 text-[11px] text-primary hover:underline disabled:cursor-not-allowed disabled:opacity-40"
                  @click="triggerAction(item, idx)"
                >
                  <LuRefreshCw
                    class="h-3 w-3"
                    :class="runningActionKey === actionKeyFor(item, idx) ? 'animate-spin' : ''"
                  />
                  <span>{{ t('onlineSources.login.clickToRefresh') }}</span>
                </button>
              </div>

              <div
                class="flex flex-col items-center justify-center gap-2.5 rounded-corner-lg bg-surface-container p-4 text-center"
                :class="item.action?.trim() && runningActionKey === null ? 'cursor-pointer' : ''"
                @click="item.action?.trim() && triggerAction(item, idx)"
              >
                <template v-if="rawMediaValueFor(item)">
                  <div
                    class="flex items-center justify-center rounded-corner-md bg-white p-2.5 shadow-elevation-1"
                  >
                    <img
                      v-if="isSafeDataImageUrl(rawMediaValueFor(item))"
                      :src="rawMediaValueFor(item)"
                      :alt="item.name"
                      class="h-44 w-44 object-contain select-none"
                    />
                    <!-- eslint-disable-next-line vue/no-v-html -->
                    <div
                      v-else-if="renderQrSvgFor(item)"
                      data-qrcode-svg
                      class="h-44 w-44 [&>svg]:block [&>svg]:h-full [&>svg]:w-full"
                      v-html="renderQrSvgFor(item)"
                    />
                    <div
                      v-else
                      class="flex h-44 w-44 items-center justify-center px-3 text-xs text-error"
                    >
                      {{ t('onlineSources.login.imageLoadFailed') }}
                    </div>
                  </div>
                  <p class="text-body-small text-on-surface-variant">
                    {{ t('onlineSources.login.qrcodeScanHint') }}
                  </p>
                </template>

                <div
                  v-else
                  class="flex min-h-36 flex-col items-center justify-center gap-2 py-3 text-on-surface-variant"
                >
                  <LuQrCode class="h-8 w-8 opacity-60" />
                  <p class="text-body-small max-w-xs">
                    {{ t('onlineSources.login.qrcodeEmptyHint') }}
                  </p>
                </div>
              </div>
            </div>

            <div
              v-else-if="isImageItem(item)"
              data-login-ui-image
              :class="fieldSpanClass(item)"
            >
              <div class="mb-1 flex items-center justify-between gap-2">
                <span class="text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">
                  {{ item.name }}
                </span>
                <button
                  v-if="item.action?.trim()"
                  type="button"
                  :disabled="runningActionKey !== null"
                  class="inline-flex cursor-pointer items-center gap-1 text-[11px] text-primary hover:underline disabled:cursor-not-allowed disabled:opacity-40"
                  @click="triggerAction(item, idx)"
                >
                  <LuRefreshCw
                    class="h-3 w-3"
                    :class="runningActionKey === actionKeyFor(item, idx) ? 'animate-spin' : ''"
                  />
                  <span>{{ t('onlineSources.login.clickToRefresh') }}</span>
                </button>
              </div>

              <div
                class="relative flex min-h-14 flex-col items-center justify-center rounded-corner-md bg-surface-container p-2.5 text-center"
                :class="item.action?.trim() && runningActionKey === null ? 'cursor-pointer hover:bg-surface-container-high' : ''"
                :title="item.action?.trim() ? t('onlineSources.login.clickToRefresh') : item.name"
                @click="item.action?.trim() && triggerAction(item, idx)"
              >
                <template v-if="resolveDisplayImageUrl(item)">
                  <img
                    v-if="!isImageFailed(idx)"
                    :key="`${imageRefreshRev}-${resolveDisplayImageUrl(item)}`"
                    :src="resolveDisplayImageUrl(item)"
                    :alt="item.name"
                    class="max-h-48 max-w-full rounded-corner-sm bg-white object-contain p-1 select-none"
                    @load="clearImageFailed(idx)"
                    @error="markImageFailed(idx)"
                  />
                  <p
                    v-else
                    class="text-body-small py-2 text-error"
                  >
                    {{ t('onlineSources.login.imageLoadFailed') }}
                  </p>
                </template>

                <div
                  v-else
                  class="flex items-center justify-center gap-2 py-2 text-on-surface-variant"
                >
                  <LuImage class="h-4 w-4 shrink-0 opacity-65" />
                  <span class="text-body-small">
                    {{ t('onlineSources.login.imageEmptyHint') }}
                  </span>
                </div>
              </div>
            </div>

            <div
              v-else-if="isDividerButton(item)"
              class="col-span-4 pt-2 text-center text-[11px] font-semibold text-[var(--md-sys-color-on-surface-variant)]"
            >
              {{ item.name }}
            </div>

            <button
              v-else-if="isButtonItem(item)"
              type="button"
              :disabled="runningActionKey !== null"
              class="md-btn md-btn-tonal md-state w-full justify-center"
              :class="buttonSpanClass(item)"
              @click="triggerAction(item, idx)"
            >
              <LuLoader2
                v-if="runningActionKey === actionKeyFor(item, idx)"
                class="h-4 w-4 animate-spin"
              />
              <span class="truncate">{{ item.name }}</span>
            </button>
          </template>

          <div v-if="showFallbackLoginSubmit" class="col-span-4 pt-1">
            <button
              type="button"
              :disabled="runningActionKey !== null"
              class="md-btn md-btn-filled md-state w-full justify-center"
              @click="triggerFallbackLogin"
            >
              <LuLoader2
                v-if="runningActionKey === '__default_login__'"
                class="h-4 w-4 animate-spin"
              />
              <span>
                {{
                  runningActionKey === '__default_login__'
                    ? t('onlineSources.login.submittingBtn')
                    : t('onlineSources.login.submitLoginBtn')
                }}
              </span>
            </button>
          </div>
        </div>

        <div
          v-else-if="!hasDeclarativeSettings"
          class="text-body-small rounded-corner-lg bg-surface-container p-4 text-center text-on-surface-variant"
        >
          <p>{{ t('onlineSources.login.noLoginUi', { domain: domainLabel }) }}</p>
          <button
            v-if="showFallbackLoginSubmit"
            type="button"
            :disabled="runningActionKey !== null"
            class="md-btn md-btn-filled md-state mt-3"
            @click="triggerFallbackLogin"
          >
            <LuLoader2
              v-if="runningActionKey === '__default_login__'"
              class="h-4 w-4 animate-spin"
            />
            <span>
              {{
                runningActionKey === '__default_login__'
                  ? t('onlineSources.login.submittingBtn')
                  : t('onlineSources.login.submitLoginBtn')
              }}
            </span>
          </button>
        </div>
      </template>

      <!-- 3. Declarative Plugin Settings Section -->
      <div
        v-if="hasDeclarativeSettings"
        data-plugin-settings-section
        class="flex flex-col gap-3.5 rounded-corner-lg bg-surface-container p-4"
      >
        <div class="flex items-center gap-2">
          <LuSlidersHorizontal class="h-4 w-4 text-primary" />
          <h4 class="text-title-small text-on-surface">
            {{ t('onlineSources.login.settingsSectionTitle') }}
          </h4>
        </div>

        <div class="flex flex-col gap-3">
          <div
            v-for="field in settingsSchema"
            :key="field.key"
            class="flex flex-col gap-1"
          >
            <div
              v-if="field.type === 'switch'"
              class="flex items-center justify-between gap-3 py-1"
            >
              <div class="min-w-0">
                <p class="text-body-medium text-on-surface">{{ field.label }}</p>
                <p v-if="field.description" class="text-body-small text-on-surface-variant">
                  {{ field.description }}
                </p>
              </div>
              <MdSwitch
                :model-value="Boolean(settingsDraft[field.key])"
                @update:model-value="settingsDraft[field.key] = $event"
              />
            </div>

            <template v-else-if="field.type === 'select'">
              <label class="text-label-medium text-on-surface-variant">
                {{ field.label }}
              </label>
              <MdSelect
                :model-value="String(settingsDraft[field.key] ?? '')"
                :options="selectOptionsFor(field)"
                size="sm"
                @update:model-value="settingsDraft[field.key] = $event"
              />
              <p v-if="field.description" class="text-body-small text-on-surface-variant">
                {{ field.description }}
              </p>
            </template>

            <template v-else>
              <label class="text-label-medium text-on-surface-variant">
                {{ field.label }}
              </label>
              <input
                :value="String(settingsDraft[field.key] ?? '')"
                :type="field.type === 'password' ? 'password' : 'text'"
                :autocomplete="field.type === 'password' ? 'off' : undefined"
                :placeholder="field.placeholder || ''"
                class="md-input h-9 w-full text-xs"
                @input="settingsDraft[field.key] = ($event.target as HTMLInputElement).value"
              />
              <p v-if="field.description" class="text-body-small text-on-surface-variant">
                {{ field.description }}
              </p>
            </template>
          </div>
        </div>

        <div class="flex justify-end pt-1">
          <button
            type="button"
            :disabled="savingSettings"
            class="md-btn md-btn-filled md-state"
            @click="handleSaveSettings"
          >
            <LuLoader2 v-if="savingSettings" class="h-4 w-4 animate-spin" />
            <span>
              {{
                savingSettings
                  ? t('onlineSources.login.savingVariableBtn')
                  : t('onlineSources.login.saveSettingsBtn')
              }}
            </span>
          </button>
        </div>
      </div>

      <!-- 4. Advanced Raw Plugin Variable Editor -->
      <div class="rounded-corner-md bg-surface-container p-3">
        <button
          type="button"
          class="text-label-small cursor-pointer text-primary hover:underline"
          @click="toggleVarEditor"
        >
          {{
            showVarEditor
              ? t('onlineSources.login.hideVariable')
              : t('onlineSources.login.showVariable')
          }}
        </button>

        <div v-if="showVarEditor" class="mt-2 space-y-2">
          <div
            v-if="loadingVar"
            class="text-label-small flex items-center gap-2 rounded-corner-md bg-surface-container px-3 py-2 text-on-surface-variant"
          >
            <LuLoader2 class="h-3.5 w-3.5 animate-spin" />
            <span>{{ t('onlineSources.login.loadingVariable') }}</span>
          </div>
          <div
            v-else-if="varLoadError"
            class="text-label-small flex items-center justify-between gap-2 rounded-corner-md bg-error-container px-3 py-2 text-on-error-container"
          >
            <span class="min-w-0 truncate">
              {{ t('onlineSources.login.loadVariableFailed', { error: varLoadError }) }}
            </span>
            <button
              type="button"
              class="shrink-0 font-medium underline"
              @click="source && loadFullVariable(source.id)"
            >
              {{ t('onlineSources.login.retryBtn') }}
            </button>
          </div>
          <textarea
            v-else
            v-model="rawVariableDraft"
            rows="4"
            class="md-input w-full py-2 font-mono text-[11px]"
            :placeholder="t('onlineSources.login.variablePlaceholder')"
          />
          <div class="flex justify-end">
            <button
              type="button"
              :disabled="savingVar || loadingVar || !varLoaded || Boolean(varLoadError)"
              class="md-btn md-btn-filled md-state"
              @click="saveRawVariable"
            >
              {{
                savingVar
                  ? t('onlineSources.login.savingVariableBtn')
                  : t('onlineSources.login.saveVariableBtn')
              }}
            </button>
          </div>
        </div>
      </div>
    </div>

    <template #footer>
      <div class="flex w-full items-center justify-end gap-2">
        <button
          type="button"
          class="md-btn md-btn-tonal md-state"
          @click="emit('close')"
        >
          {{ t('onlineSources.login.doneBtn') }}
        </button>
      </div>
    </template>
  </MdDialog>
</template>
