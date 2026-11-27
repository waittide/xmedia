import { computed } from 'vue'
import { usePreferencesStore } from '@/stores/preferences'
import { useThemeStore } from '@/stores/theme'
import { uploadedFonts } from '@/theme/customFonts'
import { normalizeHex } from '@/theme/md3'
import { resolveFontStack } from '@/utils/format'

export interface ReaderPaperPreset {
  id: 'theme' | 'parchment' | 'eyecare' | 'paper' | 'night'
  bgColor: string | null
  textColor: string | null
  previewBg: string
  previewFg: string
}

export const READER_PAPER_PRESETS: readonly ReaderPaperPreset[] = [
  {
    id: 'theme',
    bgColor: null,
    textColor: null,
    previewBg: 'var(--md-sys-color-surface)',
    previewFg: 'var(--md-sys-color-on-surface)',
  },
  {
    id: 'parchment',
    bgColor: '#f6efe2',
    textColor: '#2c2218',
    previewBg: '#f6efe2',
    previewFg: '#2c2218',
  },
  {
    id: 'eyecare',
    bgColor: '#dcecd6',
    textColor: '#1b2b1e',
    previewBg: '#dcecd6',
    previewFg: '#1b2b1e',
  },
  {
    id: 'paper',
    bgColor: '#fafafa',
    textColor: '#1f2328',
    previewBg: '#fafafa',
    previewFg: '#1f2328',
  },
  {
    id: 'night',
    bgColor: '#141619',
    textColor: '#c9d1d9',
    previewBg: '#141619',
    previewFg: '#c9d1d9',
  },
]

export function useReaderAppearance() {
  const preferencesStore = usePreferencesStore()
  const themeStore = useThemeStore()

  const customBgColor = computed<string | null>(() => {
    const raw = preferencesStore.reader.bg_color
    if (!raw) return null
    const hex = normalizeHex(raw, '')
    return hex || null
  })

  const customTextColor = computed<string | null>(() => {
    const raw = preferencesStore.reader.text_color
    if (!raw) return null
    const hex = normalizeHex(raw, '')
    return hex || null
  })

  const resolvedBgColor = computed<string>(
    () => customBgColor.value ?? themeStore.activeScheme.surface,
  )

  const resolvedTextColor = computed<string>(
    () => customTextColor.value ?? themeStore.activeScheme['on-surface'],
  )

  const resolvedLinkColor = computed<string>(() => themeStore.activeScheme.primary)

  const fontStack = computed<string>(() => {
    void uploadedFonts.value.length
    return resolveFontStack(preferencesStore.reader.font_family, themeStore.uiFont)
  })

  const fontSizePx = computed<number>(() => preferencesStore.reader.font_size || 17)
  const lineHeight = computed<number>(() => preferencesStore.reader.line_height || 1.7)

  const letterSpacingEm = computed<number>(() => {
    const val = preferencesStore.reader.letter_spacing_em
    return typeof val === 'number' && Number.isFinite(val) ? val : 0
  })

  const paragraphIndentEm = computed<number>(() => {
    const val = preferencesStore.reader.paragraph_indent_em
    return typeof val === 'number' && Number.isFinite(val) ? val : 2
  })

  const paragraphSpacingPx = computed<number>(() => {
    const val = preferencesStore.reader.paragraph_spacing_px
    return typeof val === 'number' && Number.isFinite(val) ? val : 16
  })

  const marginTopPx = computed<number>(() => {
    const val = preferencesStore.reader.margin_top_px
    return typeof val === 'number' && Number.isFinite(val) ? val : 24
  })

  const marginBottomPx = computed<number>(() => {
    const val = preferencesStore.reader.margin_bottom_px
    return typeof val === 'number' && Number.isFinite(val) ? val : 24
  })

  const marginLeftPx = computed<number>(() => {
    const val = preferencesStore.reader.margin_left_px
    if (typeof val === 'number' && Number.isFinite(val)) return val
    return preferencesStore.reader.margin_px ?? 36
  })

  const marginRightPx = computed<number>(() => {
    const val = preferencesStore.reader.margin_right_px
    if (typeof val === 'number' && Number.isFinite(val)) return val
    return preferencesStore.reader.margin_px ?? 36
  })

  const stageStyle = computed(() => ({
    backgroundColor: resolvedBgColor.value,
    color: resolvedTextColor.value,
  }))

  const viewportFrameStyle = computed(() => ({
    backgroundColor: resolvedBgColor.value,
    color: resolvedTextColor.value,
    paddingTop: `calc(var(--safe-area-top) + ${marginTopPx.value}px)`,
    paddingBottom: `calc(var(--safe-area-bottom) + ${marginBottomPx.value}px)`,
    paddingLeft: `calc(var(--safe-area-left) + ${marginLeftPx.value}px)`,
    paddingRight: `calc(var(--safe-area-right) + ${marginRightPx.value}px)`,
  }))

  return {
    customBgColor,
    customTextColor,
    resolvedBgColor,
    resolvedTextColor,
    resolvedLinkColor,
    fontStack,
    fontSizePx,
    lineHeight,
    letterSpacingEm,
    paragraphIndentEm,
    paragraphSpacingPx,
    marginTopPx,
    marginBottomPx,
    marginLeftPx,
    marginRightPx,
    stageStyle,
    viewportFrameStyle,
  }
}
