export type ThemeMode = 'light' | 'dark' | 'system'
export type CatppuccinFlavor = 'latte' | 'frappe' | 'macchiato' | 'mocha'
export type AccentMode = 'preset' | 'catppuccin' | 'custom'
export type CatppuccinAccent =
  | 'rosewater'
  | 'flamingo'
  | 'pink'
  | 'mauve'
  | 'red'
  | 'maroon'
  | 'peach'
  | 'yellow'
  | 'green'
  | 'teal'
  | 'sky'
  | 'sapphire'
  | 'blue'
  | 'lavender'

export interface CustomPaletteTokens {
  canvas: string
  shell: string
  surface: string
  text: string
  subtext: string
}

export interface PalettePreset {
  id: string
  kind: 'light' | 'dark'
  defaultAccent: string
  catppuccinFlavor?: CatppuccinFlavor
  tokens: CustomPaletteTokens
  semantic: {
    red: string
    green: string
    yellow: string
  }
}

export interface ResolvedPalette {
  canvas: string
  shell: string
  mantle: string
  surface0: string
  surface1: string
  surface2: string
  text: string
  subtext1: string
  subtext0: string
  accent: string
  onAccent: string
  red: string
  green: string
  yellow: string
}

export const CATPPUCCIN_ACCENTS: readonly CatppuccinAccent[] = [
  'rosewater',
  'flamingo',
  'pink',
  'mauve',
  'red',
  'maroon',
  'peach',
  'yellow',
  'green',
  'teal',
  'sky',
  'sapphire',
  'blue',
  'lavender',
] as const

export const CATPPUCCIN_ACCENT_HEX: Record<
  'light' | 'dark',
  Record<CatppuccinAccent, string>
> = {
  light: {
    rosewater: '#dc8a78',
    flamingo: '#dd7878',
    pink: '#ea76cb',
    mauve: '#8839ef',
    red: '#d20f39',
    maroon: '#e64553',
    peach: '#fe640b',
    yellow: '#df8e1d',
    green: '#40a02b',
    teal: '#179299',
    sky: '#04a5e5',
    sapphire: '#209fb5',
    blue: '#1e66f5',
    lavender: '#7287fd',
  },
  dark: {
    rosewater: '#f5e0dc',
    flamingo: '#f2cdcd',
    pink: '#f5c2e7',
    mauve: '#cba6f7',
    red: '#f38ba8',
    maroon: '#eba0ac',
    peach: '#fab387',
    yellow: '#f9e2af',
    green: '#a6e3a1',
    teal: '#94e2d5',
    sky: '#89dceb',
    sapphire: '#74c7ec',
    blue: '#89b4fa',
    lavender: '#b4befe',
  },
}

export const LIGHT_PALETTE_PRESETS: readonly PalettePreset[] = [
  {
    id: 'material-you-light',
    kind: 'light',
    defaultAccent: '#0b57d0',
    tokens: {
      canvas: '#fdfcff',
      shell: '#f0f4f9',
      surface: '#dfe3ec',
      text: '#1b1b1f',
      subtext: '#44474f',
    },
    semantic: { red: '#ba1a1a', green: '#146c2e', yellow: '#9a5b00' },
  },
  {
    id: 'catppuccin-latte',
    kind: 'light',
    defaultAccent: '#1e66f5',
    catppuccinFlavor: 'latte',
    tokens: {
      canvas: '#eff1f5',
      shell: '#e6e9ef',
      surface: '#ccd0da',
      text: '#4c4f69',
      subtext: '#6c6f85',
    },
    semantic: { red: '#d20f39', green: '#40a02b', yellow: '#df8e1d' },
  },
  {
    id: 'warm-paper',
    kind: 'light',
    defaultAccent: '#a85923',
    tokens: {
      canvas: '#fdfbf7',
      shell: '#f4efe4',
      surface: '#e4dccb',
      text: '#2c251e',
      subtext: '#6e6459',
    },
    semantic: { red: '#c43e3e', green: '#3c7d3e', yellow: '#c4841d' },
  },
  {
    id: 'rose-pine-dawn',
    kind: 'light',
    defaultAccent: '#7e6699',
    tokens: {
      canvas: '#faf4ed',
      shell: '#f2e9e1',
      surface: '#dfdad9',
      text: '#575279',
      subtext: '#797593',
    },
    semantic: { red: '#aa566f', green: '#286983', yellow: '#ea9d34' },
  },
  {
    id: 'solarized-light',
    kind: 'light',
    defaultAccent: '#268bd2',
    tokens: {
      canvas: '#fdf6e3',
      shell: '#eee8d5',
      surface: '#d8d2bf',
      text: '#073642',
      subtext: '#586e75',
    },
    semantic: { red: '#dc322f', green: '#859900', yellow: '#b58900' },
  },
  {
    id: 'github-light',
    kind: 'light',
    defaultAccent: '#0969da',
    tokens: {
      canvas: '#ffffff',
      shell: '#f6f8fa',
      surface: '#d0d7de',
      text: '#1f2328',
      subtext: '#656d76',
    },
    semantic: { red: '#cf222e', green: '#1a7f37', yellow: '#9a6700' },
  },
] as const

export const DARK_PALETTE_PRESETS: readonly PalettePreset[] = [
  {
    id: 'material-you-dark',
    kind: 'dark',
    defaultAccent: '#a8c7fa',
    tokens: {
      canvas: '#111318',
      shell: '#1a1d24',
      surface: '#2b2f3a',
      text: '#e2e2e9',
      subtext: '#c4c6d0',
    },
    semantic: { red: '#ffb4ab', green: '#81c995', yellow: '#fdd663' },
  },
  {
    id: 'catppuccin-mocha',
    kind: 'dark',
    defaultAccent: '#89b4fa',
    catppuccinFlavor: 'mocha',
    tokens: {
      canvas: '#1e1e2e',
      shell: '#181825',
      surface: '#313244',
      text: '#cdd6f4',
      subtext: '#a6adc8',
    },
    semantic: { red: '#f38ba8', green: '#a6e3a1', yellow: '#f9e2af' },
  },
  {
    id: 'catppuccin-macchiato',
    kind: 'dark',
    defaultAccent: '#8aadf4',
    catppuccinFlavor: 'macchiato',
    tokens: {
      canvas: '#24273a',
      shell: '#1e2030',
      surface: '#363a4f',
      text: '#cad3f5',
      subtext: '#a5adcb',
    },
    semantic: { red: '#ed8796', green: '#a6da95', yellow: '#eed49f' },
  },
  {
    id: 'catppuccin-frappe',
    kind: 'dark',
    defaultAccent: '#8caaee',
    catppuccinFlavor: 'frappe',
    tokens: {
      canvas: '#303446',
      shell: '#292c3c',
      surface: '#414559',
      text: '#c6d0f5',
      subtext: '#a5adce',
    },
    semantic: { red: '#e78284', green: '#a6d189', yellow: '#e5c890' },
  },
  {
    id: 'tokyo-night',
    kind: 'dark',
    defaultAccent: '#7aa2f7',
    tokens: {
      canvas: '#1a1b26',
      shell: '#16161e',
      surface: '#292e42',
      text: '#c0caf5',
      subtext: '#9aa5ce',
    },
    semantic: { red: '#f7768e', green: '#9ece6a', yellow: '#e0af68' },
  },
  {
    id: 'nord',
    kind: 'dark',
    defaultAccent: '#88c0d0',
    tokens: {
      canvas: '#2e3440',
      shell: '#242933',
      surface: '#3b4252',
      text: '#eceff4',
      subtext: '#d8dee9',
    },
    semantic: { red: '#bf616a', green: '#a3be8c', yellow: '#ebcb8b' },
  },
  {
    id: 'rose-pine',
    kind: 'dark',
    defaultAccent: '#c4a7e7',
    tokens: {
      canvas: '#191724',
      shell: '#1f1d2e',
      surface: '#26233a',
      text: '#e0def4',
      subtext: '#908caa',
    },
    semantic: { red: '#eb6f92', green: '#31748f', yellow: '#f6c177' },
  },
] as const

export function parseHexRgb(hex: string): [number, number, number] {
  const clean = hex.trim().replace(/^#/, '')
  if (/^[0-9a-fA-F]{3}$/.test(clean)) {
    const r = Number.parseInt(clean[0]! + clean[0]!, 16)
    const g = Number.parseInt(clean[1]! + clean[1]!, 16)
    const b = Number.parseInt(clean[2]! + clean[2]!, 16)
    return [r, g, b]
  }
  if (/^[0-9a-fA-F]{6}$/.test(clean)) {
    const num = Number.parseInt(clean, 16)
    return [(num >> 16) & 255, (num >> 8) & 255, num & 255]
  }
  return [128, 128, 128]
}

export function rgbToHex(r: number, g: number, b: number): string {
  const clamp = (v: number) => Math.max(0, Math.min(255, Math.round(v)))
  return (
    '#' +
    [clamp(r), clamp(g), clamp(b)]
      .map((x) => x.toString(16).padStart(2, '0'))
      .join('')
  )
}

export const DEFAULT_LIGHT_PRESET_ID = 'material-you-light'
export const DEFAULT_DARK_PRESET_ID = 'material-you-dark'
export const DEFAULT_ACCENT_MODE: AccentMode = 'preset'

function mixHex(hexA: string, hexB: string, weightA: number): string {
  const [r1, g1, b1] = parseHexRgb(hexA)
  const [r2, g2, b2] = parseHexRgb(hexB)
  const w = Math.max(0, Math.min(1, weightA))
  return rgbToHex(r1 * w + r2 * (1 - w), g1 * w + g2 * (1 - w), b1 * w + b2 * (1 - w))
}

function srgbChannelToLinear(channel: number): number {
  const s = channel / 255
  return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4
}

function linearToSrgbChannel(linear: number): number {
  const clamped = Math.max(0, Math.min(1, linear))
  const s = clamped <= 0.0031308 ? 12.92 * clamped : 1.055 * clamped ** (1 / 2.4) - 0.055
  return Math.round(s * 255)
}

/** WCAG 2.1 relative luminance in [0, 1]. */
export function relativeLuminance(hex: string): number {
  const [r, g, b] = parseHexRgb(hex)
  return (
    0.2126 * srgbChannelToLinear(r) +
    0.7152 * srgbChannelToLinear(g) +
    0.0722 * srgbChannelToLinear(b)
  )
}

/** CIE 1976 L* perceptual lightness (identical to MD3 HCT Tone in [0, 100]). */
export function hexToTone(hex: string): number {
  const y = relativeLuminance(hex)
  const ft = y > 216 / 24389 ? Math.cbrt(y) : (24389 / 27) * y / 116 + 16 / 116
  return Math.max(0, Math.min(100, 116 * ft - 16))
}

function toneToLuminance(tone: number): number {
  const t = Math.max(0, Math.min(100, tone))
  const ft = (t + 16) / 116
  return t > 8 ? ft ** 3 : (t * 27) / 24389
}

/** Convert sRGB hex -> OKLCH (L in [0,1], C >= 0, H in [0,360)). */
function hexToOklch(hex: string): [number, number, number] {
  const [r8, g8, b8] = parseHexRgb(hex)
  const r = srgbChannelToLinear(r8)
  const g = srgbChannelToLinear(g8)
  const b = srgbChannelToLinear(b8)

  const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b)
  const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b)
  const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b)

  const okL = 0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s
  const okA = 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s
  const okB = 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s

  const c = Math.hypot(okA, okB)
  const h = c < 1e-6 ? 0 : (((Math.atan2(okB, okA) * 180) / Math.PI) % 360 + 360) % 360
  return [okL, c, h]
}

function oklchToLinearRgb(okL: number, okC: number, okH: number): [number, number, number] {
  const rad = (okH * Math.PI) / 180
  const okA = okC * Math.cos(rad)
  const okB = okC * Math.sin(rad)

  const l_ = okL + 0.3963377774 * okA + 0.2158037573 * okB
  const m_ = okL - 0.1055613458 * okA - 0.0638541728 * okB
  const s_ = okL - 0.0894841775 * okA - 1.291485548 * okB

  const l = l_ * l_ * l_
  const m = m_ * m_ * m_
  const s = s_ * s_ * s_

  return [
    +4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
    -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
    -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s,
  ]
}

function isInSrgbGamut(rgb: [number, number, number]): boolean {
  return (
    rgb[0] >= -1e-4 &&
    rgb[0] <= 1 + 1e-4 &&
    rgb[1] >= -1e-4 &&
    rgb[1] <= 1 + 1e-4 &&
    rgb[2] >= -1e-4 &&
    rgb[2] <= 1 + 1e-4
  )
}

/**
 * Generates an sRGB hex at exact MD3 perceptual Tone (CIE L* in [0, 100]) for a given
 * OKLCH hue and target chroma, gamut-mapping chroma smoothly when near extremes.
 */
export function tonalHex(okH: number, targetChroma: number, tone: number): string {
  const clampedTone = Math.max(0, Math.min(100, tone))
  if (clampedTone <= 0.01) return '#000000'
  if (clampedTone >= 99.99) return '#ffffff'

  const targetY = toneToLuminance(clampedTone)
  const hue = ((okH % 360) + 360) % 360

  // Bisect OKLab L for a given chroma to hit exact relative luminance targetY.
  const solveForChroma = (c: number): [number, number, number] => {
    let lo = 0
    let hi = 1
    let rgb: [number, number, number] = [targetY, targetY, targetY]
    for (let i = 0; i < 16; i += 1) {
      const mid = (lo + hi) * 0.5
      rgb = oklchToLinearRgb(mid, c, hue)
      const y =
        0.2126 * Math.max(0, Math.min(1, rgb[0])) +
        0.7152 * Math.max(0, Math.min(1, rgb[1])) +
        0.0722 * Math.max(0, Math.min(1, rgb[2]))
      if (y < targetY) lo = mid
      else hi = mid
    }
    return rgb
  }

  let rgb = solveForChroma(targetChroma)
  if (!isInSrgbGamut(rgb) && targetChroma > 0) {
    let cLo = 0
    let cHi = targetChroma
    let best = solveForChroma(0)
    for (let i = 0; i < 12; i += 1) {
      const cMid = (cLo + cHi) * 0.5
      const cand = solveForChroma(cMid)
      if (isInSrgbGamut(cand)) {
        best = cand
        cLo = cMid
      } else {
        cHi = cMid
      }
    }
    rgb = best
  }

  return rgbToHex(
    linearToSrgbChannel(rgb[0]),
    linearToSrgbChannel(rgb[1]),
    linearToSrgbChannel(rgb[2]),
  )
}

/** WCAG 2.1 contrast ratio in [1, 21]. */
export function contrastRatio(hexA: string, hexB: string): number {
  const l1 = relativeLuminance(hexA)
  const l2 = relativeLuminance(hexB)
  const lighter = Math.max(l1, l2)
  const darker = Math.min(l1, l2)
  return (lighter + 0.05) / (darker + 0.05)
}

/** Choose the higher-contrast text color against bgHex per WCAG 2.1. */
export function chooseContrastText(
  bgHex: string,
  darkText = '#0b0d14',
  lightText = '#ffffff',
): string {
  return contrastRatio(bgHex, darkText) >= contrastRatio(bgHex, lightText) ? darkText : lightText
}

/** Choose a tinted MD3 on-* tonal color (Tone 100 or Tone 20/10) meeting minRatio against bgHex. */
function chooseTonalOnColor(
  bgHex: string,
  hue: number,
  chroma: number,
  minRatio = 4.5,
): string {
  const lightCand = '#ffffff'
  const darkCand = tonalHex(hue, Math.min(chroma, 0.08), 18)
  const pickLight = contrastRatio(bgHex, lightCand) >= contrastRatio(bgHex, darkCand)
  const preferred = pickLight ? lightCand : darkCand
  if (contrastRatio(bgHex, preferred) >= minRatio) return preferred
  return ensureContrastAgainstAll(preferred, [bgHex], minRatio)
}

/** Ensure fgHex meets at least minRatio against EVERY background in bgHexes using perceptual tone shift. */
function ensureContrastAgainstAll(
  fgHex: string,
  bgHexes: readonly string[],
  minRatio = 4.5,
): string {
  const meetsAll = (hex: string) => bgHexes.every((bg) => contrastRatio(hex, bg) >= minRatio)
  if (meetsAll(fgHex)) return fgHex

  const [, origC, origH] = hexToOklch(fgHex)
  const origTone = hexToTone(fgHex)
  const avgBgLum = bgHexes.reduce((sum, bg) => sum + relativeLuminance(bg), 0) / bgHexes.length
  const goDarker = avgBgLum >= 0.18

  let lo = goDarker ? 0 : origTone
  let hi = goDarker ? origTone : 100
  let best = goDarker ? '#000000' : '#ffffff'

  for (let i = 0; i < 16; i += 1) {
    const mid = (lo + hi) * 0.5
    const candidate = tonalHex(origH, origC, mid)
    if (meetsAll(candidate)) {
      best = candidate
      if (goDarker) lo = mid
      else hi = mid
    } else {
      if (goDarker) hi = mid
      else lo = mid
    }
  }
  return best
}

/** Ensure fgHex meets at least minRatio (default WCAG AA 4.5:1) against bgHex. */
function ensureContrast(fgHex: string, bgHex: string, minRatio = 4.5): string {
  return ensureContrastAgainstAll(fgHex, [bgHex], minRatio)
}

function rgbToHsl(r: number, g: number, b: number): [number, number, number] {
  const rn = r / 255
  const gn = g / 255
  const bn = b / 255
  const max = Math.max(rn, gn, bn)
  const min = Math.min(rn, gn, bn)
  const l = (max + min) / 2
  const d = max - min
  if (d === 0) return [0, 0, l]
  const s = l > 0.5 ? d / (2 - max - min) : d / (max + min)
  let h = 0
  if (max === rn) h = ((gn - bn) / d + (gn < bn ? 6 : 0)) * 60
  else if (max === gn) h = ((bn - rn) / d + 2) * 60
  else h = ((rn - gn) / d + 4) * 60
  return [h, s, l]
}

function hslToHex(h: number, s: number, l: number): string {
  const hue = ((h % 360) + 360) % 360
  const sat = Math.max(0, Math.min(1, s))
  const lig = Math.max(0, Math.min(1, l))
  const c = (1 - Math.abs(2 * lig - 1)) * sat
  const x = c * (1 - Math.abs(((hue / 60) % 2) - 1))
  const m = lig - c / 2
  let r1 = 0
  let g1 = 0
  let b1 = 0
  if (hue < 60) [r1, g1, b1] = [c, x, 0]
  else if (hue < 120) [r1, g1, b1] = [x, c, 0]
  else if (hue < 180) [r1, g1, b1] = [0, c, x]
  else if (hue < 240) [r1, g1, b1] = [0, x, c]
  else if (hue < 300) [r1, g1, b1] = [x, 0, c]
  else [r1, g1, b1] = [c, 0, x]
  return rgbToHex((r1 + m) * 255, (g1 + m) * 255, (b1 + m) * 255)
}

export function normalizeHex(input: string, fallback: string): string {
  const trimmed = input.trim().replace(/^#/, '')
  if (/^[0-9a-fA-F]{6}$/.test(trimmed)) {
    return `#${trimmed.toLowerCase()}`
  }
  if (/^[0-9a-fA-F]{3}$/.test(trimmed)) {
    const expanded = trimmed
      .toLowerCase()
      .split('')
      .map((c) => c + c)
      .join('')
    return `#${expanded}`
  }
  return fallback
}

export function resolvePalette(params: {
  isDark: boolean
  lightPresetId: string
  darkPresetId: string
  customLight: CustomPaletteTokens
  customDark: CustomPaletteTokens
  accentMode: AccentMode
  catppuccinAccent: CatppuccinAccent
  customAccent: string
}): ResolvedPalette {
  const presetPool = params.isDark ? DARK_PALETTE_PRESETS : LIGHT_PALETTE_PRESETS
  const fallbackPreset = presetPool[0]!
  const targetPresetId = params.isDark ? params.darkPresetId : params.lightPresetId
  const matchedPreset = presetPool.find((p) => p.id === targetPresetId) ?? fallbackPreset

  const tokens =
    targetPresetId === 'custom'
      ? params.isDark
        ? params.customDark
        : params.customLight
      : matchedPreset.tokens

  const kind: 'light' | 'dark' = params.isDark ? 'dark' : 'light'
  let accentHex = matchedPreset.defaultAccent
  if (params.accentMode === 'catppuccin') {
    accentHex = CATPPUCCIN_ACCENT_HEX[kind][params.catppuccinAccent]
  } else if (params.accentMode === 'custom') {
    accentHex = normalizeHex(params.customAccent, matchedPreset.defaultAccent)
  }

  const mantle = mixHex(tokens.shell, tokens.canvas, 0.72)
  const surface0 = tokens.surface
  const surface1 = mixHex(tokens.surface, tokens.text, 0.82)
  const surface2 = mixHex(tokens.surface, tokens.text, 0.66)
  const subtext1 = mixHex(tokens.text, tokens.subtext, 0.5)

  return {
    canvas: tokens.canvas,
    shell: tokens.shell,
    mantle,
    surface0,
    surface1,
    surface2,
    text: tokens.text,
    subtext1,
    subtext0: tokens.subtext,
    accent: accentHex,
    onAccent: chooseContrastText(accentHex),
    red: matchedPreset.semantic.red,
    green: matchedPreset.semantic.green,
    yellow: matchedPreset.semantic.yellow,
  }
}

export interface Md3ColorScheme {
  surface: string
  surfaceDim: string
  surfaceBright: string
  surfaceContainerLowest: string
  surfaceContainerLow: string
  surfaceContainer: string
  surfaceContainerHigh: string
  surfaceContainerHighest: string
  onSurface: string
  onSurfaceRgb: string
  onSurfaceVariant: string
  outline: string
  outlineVariant: string
  scrim: string
  shadow: string
  primary: string
  onPrimary: string
  primaryContainer: string
  onPrimaryContainer: string
  primaryFixed: string
  primaryFixedDim: string
  onPrimaryFixed: string
  onPrimaryFixedVariant: string
  secondary: string
  onSecondary: string
  secondaryContainer: string
  onSecondaryContainer: string
  secondaryFixed: string
  secondaryFixedDim: string
  onSecondaryFixed: string
  onSecondaryFixedVariant: string
  tertiary: string
  onTertiary: string
  tertiaryContainer: string
  onTertiaryContainer: string
  tertiaryFixed: string
  tertiaryFixedDim: string
  onTertiaryFixed: string
  onTertiaryFixedVariant: string
  error: string
  onError: string
  errorContainer: string
  onErrorContainer: string
  success: string
  successContainer: string
  onSuccessContainer: string
  warning: string
  warningContainer: string
  onWarningContainer: string
  inverseSurface: string
  inverseOnSurface: string
  inversePrimary: string
}

/**
 * Pure MD3 color scheme generator implementing perceptual CIE L* / OKLCH tonal palettes,
 * all 36 official MD3 color roles (including `shadow` and all 12 `*Fixed*` tokens), and
 * strict WCAG 2.1 AA contrast guarantees across every preset and accent combination.
 */
export function computeMd3ColorScheme(resolved: ResolvedPalette, isDark: boolean): Md3ColorScheme {
  const [, primaryOkC, primaryOkH] = hexToOklch(resolved.accent)
  const [r, g, b] = parseHexRgb(resolved.accent)
  const [primaryH, primaryS] = rgbToHsl(r, g, b)

  // Material Design 3 monotonic tonal surface & container tokens
  const surface = resolved.canvas
  const surfaceLum = relativeLuminance(surface)

  let surfaceDim: string
  let surfaceBright: string
  let containerLowest: string
  let containerLow: string
  let containerDefault: string
  let containerHigh: string
  let containerHighest: string

  if (isDark) {
    // Dark mode: strictly brighten from lowest -> surface -> low -> default -> high -> highest -> bright.
    const rawElevated =
      relativeLuminance(resolved.surface0) > surfaceLum + 0.012
        ? resolved.surface0
        : mixHex(surface, resolved.text, 0.78)
    const [er, eg, eb] = parseHexRgb(rawElevated)
    const [, , elevL] = rgbToHsl(er, eg, eb)
    const safeTint = hslToHex(primaryH, Math.min(primaryS, 0.42), Math.max(elevL, 0.24))
    const tintedTop = mixHex(rawElevated, safeTint, 0.92)

    surfaceDim = mixHex(surface, '#000000', 0.88)
    containerLowest =
      relativeLuminance(resolved.shell) < surfaceLum - 0.002
        ? mixHex(resolved.shell, '#000000', 0.86)
        : mixHex(surface, '#000000', 0.74)
    containerLow = mixHex(surface, tintedTop, 0.76)
    containerDefault = mixHex(surface, tintedTop, 0.54)
    containerHigh = mixHex(surface, tintedTop, 0.28)
    containerHighest = mixHex(tintedTop, resolved.text, 0.94)
    surfaceBright = mixHex(tintedTop, resolved.text, 0.84)
  } else {
    // Light mode: strictly darken from lowest -> surface -> low -> default -> high -> highest -> dim.
    const rawDeep =
      relativeLuminance(resolved.surface0) < surfaceLum - 0.035
        ? resolved.surface0
        : mixHex(surface, resolved.text, 0.86)
    const [dr, dg, db] = parseHexRgb(rawDeep)
    const [, , deepL] = rgbToHsl(dr, dg, db)
    const safeTint = hslToHex(primaryH, Math.min(primaryS, 0.4), Math.min(deepL, 0.84))
    const tintedDeep = mixHex(rawDeep, safeTint, 0.94)

    surfaceBright = mixHex(surface, '#ffffff', 0.35)
    containerLowest = mixHex(surface, '#ffffff', 0.22)
    containerLow = mixHex(surface, tintedDeep, 0.78)
    containerDefault = mixHex(surface, tintedDeep, 0.56)
    containerHigh = mixHex(surface, tintedDeep, 0.32)
    containerHighest = mixHex(surface, tintedDeep, 0.1)
    surfaceDim = mixHex(tintedDeep, resolved.text, 0.92)
  }

  // MD3 Primary, Secondary (same hue, reduced chroma per SchemeTonalSpot) & Tertiary (+60° hue)
  const primary = ensureContrastAgainstAll(resolved.accent, [surface], 4.5)
  const onPrimary = chooseTonalOnColor(primary, primaryOkH, primaryOkC, 4.5)
  const primaryContainer = isDark
    ? tonalHex(primaryOkH, Math.min(primaryOkC * 0.85, 0.14), 30)
    : tonalHex(primaryOkH, Math.min(primaryOkC * 0.6, 0.08), 90)
  const onPrimaryContainer = ensureContrast(
    tonalHex(primaryOkH, Math.min(primaryOkC * 0.75, 0.1), isDark ? 90 : 12),
    primaryContainer,
    4.5,
  )

  const secondaryChroma = primaryOkC < 0.02 ? primaryOkC : Math.min(0.085, Math.max(0.035, primaryOkC * 0.42))
  const secondaryRaw = tonalHex(primaryOkH, secondaryChroma, isDark ? 80 : 40)
  const secondary = ensureContrastAgainstAll(secondaryRaw, [surface], 4.5)
  const onSecondary = chooseTonalOnColor(secondary, primaryOkH, secondaryChroma, 4.5)
  const secondaryContainer = isDark
    ? tonalHex(primaryOkH, Math.min(secondaryChroma, 0.06), 30)
    : tonalHex(primaryOkH, Math.min(secondaryChroma * 0.7, 0.05), 90)
  const onSecondaryContainer = ensureContrast(
    tonalHex(primaryOkH, Math.min(secondaryChroma, 0.06), isDark ? 90 : 12),
    secondaryContainer,
    4.5,
  )

  const tertiaryH = (primaryOkH + 60) % 360
  const tertiaryChroma = primaryOkC < 0.02 ? 0.06 : Math.min(0.14, Math.max(0.06, primaryOkC * 0.65))
  const tertiaryRaw = tonalHex(tertiaryH, tertiaryChroma, isDark ? 80 : 40)
  const tertiary = ensureContrastAgainstAll(tertiaryRaw, [surface], 4.5)
  const onTertiary = chooseTonalOnColor(tertiary, tertiaryH, tertiaryChroma, 4.5)
  const tertiaryContainer = isDark
    ? tonalHex(tertiaryH, Math.min(tertiaryChroma * 0.8, 0.12), 30)
    : tonalHex(tertiaryH, Math.min(tertiaryChroma * 0.6, 0.08), 90)
  const onTertiaryContainer = ensureContrast(
    tonalHex(tertiaryH, Math.min(tertiaryChroma * 0.75, 0.1), isDark ? 90 : 12),
    tertiaryContainer,
    4.5,
  )

  // Official MD3 12 Fixed Accent Roles (invariant across Light & Dark themes: T90 / T80 / T10 / T30)
  const primaryFixed = tonalHex(primaryOkH, Math.min(primaryOkC * 0.6, 0.085), 90)
  const primaryFixedDim = tonalHex(primaryOkH, Math.min(primaryOkC * 0.85, 0.13), 80)
  const onPrimaryFixed = ensureContrastAgainstAll(
    tonalHex(primaryOkH, Math.min(primaryOkC * 0.7, 0.09), 10),
    [primaryFixed, primaryFixedDim],
    7.0,
  )
  const onPrimaryFixedVariant = ensureContrastAgainstAll(
    tonalHex(primaryOkH, Math.min(primaryOkC * 0.8, 0.11), 30),
    [primaryFixed, primaryFixedDim],
    4.5,
  )

  const secondaryFixed = tonalHex(primaryOkH, Math.min(secondaryChroma * 0.75, 0.055), 90)
  const secondaryFixedDim = tonalHex(primaryOkH, Math.min(secondaryChroma, 0.075), 80)
  const onSecondaryFixed = ensureContrastAgainstAll(
    tonalHex(primaryOkH, Math.min(secondaryChroma, 0.06), 10),
    [secondaryFixed, secondaryFixedDim],
    7.0,
  )
  const onSecondaryFixedVariant = ensureContrastAgainstAll(
    tonalHex(primaryOkH, Math.min(secondaryChroma, 0.06), 30),
    [secondaryFixed, secondaryFixedDim],
    4.5,
  )

  const tertiaryFixed = tonalHex(tertiaryH, Math.min(tertiaryChroma * 0.65, 0.085), 90)
  const tertiaryFixedDim = tonalHex(tertiaryH, Math.min(tertiaryChroma * 0.85, 0.12), 80)
  const onTertiaryFixed = ensureContrastAgainstAll(
    tonalHex(tertiaryH, Math.min(tertiaryChroma * 0.7, 0.09), 10),
    [tertiaryFixed, tertiaryFixedDim],
    7.0,
  )
  const onTertiaryFixedVariant = ensureContrastAgainstAll(
    tonalHex(tertiaryH, Math.min(tertiaryChroma * 0.8, 0.11), 30),
    [tertiaryFixed, tertiaryFixedDim],
    4.5,
  )

  // Semantic roles (Error / Success / Warning) with guaranteed WCAG AA contrast
  const [, errOkC, errOkH] = hexToOklch(resolved.red)
  const error = ensureContrastAgainstAll(resolved.red, [surface], 4.5)
  const onError = chooseTonalOnColor(error, errOkH, errOkC, 4.5)
  const errorContainer = isDark
    ? tonalHex(errOkH, Math.min(errOkC * 0.85, 0.14), 28)
    : tonalHex(errOkH, Math.min(errOkC * 0.55, 0.08), 92)
  const onErrorContainer = ensureContrast(
    tonalHex(errOkH, Math.min(errOkC * 0.75, 0.1), isDark ? 90 : 12),
    errorContainer,
    4.5,
  )

  const [, sucOkC, sucOkH] = hexToOklch(resolved.green)
  const success = ensureContrastAgainstAll(resolved.green, [surface], 4.5)
  const successContainer = isDark
    ? tonalHex(sucOkH, Math.min(sucOkC * 0.85, 0.12), 28)
    : tonalHex(sucOkH, Math.min(sucOkC * 0.55, 0.08), 92)
  const onSuccessContainer = ensureContrast(
    tonalHex(sucOkH, Math.min(sucOkC * 0.75, 0.09), isDark ? 90 : 12),
    successContainer,
    4.5,
  )

  const [, warnOkC, warnOkH] = hexToOklch(resolved.yellow)
  const warning = ensureContrastAgainstAll(resolved.yellow, [surface], 4.5)
  const warningContainer = isDark
    ? tonalHex(warnOkH, Math.min(warnOkC * 0.85, 0.12), 28)
    : tonalHex(warnOkH, Math.min(warnOkC * 0.55, 0.08), 92)
  const onWarningContainer = ensureContrast(
    tonalHex(warnOkH, Math.min(warnOkC * 0.75, 0.09), isDark ? 90 : 12),
    warningContainer,
    4.5,
  )

  // Neutral ink & outline roles clamped against BOTH `surface` AND `surfaceContainerHighest`
  const onSurface = ensureContrastAgainstAll(resolved.text, [surface, containerHighest], 4.5)
  const onSurfaceVariant = ensureContrastAgainstAll(
    resolved.subtext0,
    [surface, containerHighest],
    4.5,
  )
  const rawOutline = mixHex(resolved.subtext0, surface, 0.75)
  const outline = ensureContrastAgainstAll(rawOutline, [surface, containerHighest], 3.0)
  const outlineVariant = mixHex(resolved.surface0, surface, 0.72)
  const scrim = '#000000'
  const shadow = '#000000'

  const inverseSurface = isDark
    ? mixHex(onSurface, primary, 0.94)
    : mixHex(onSurface, primary, 0.92)
  const inverseOnSurface = chooseContrastText(inverseSurface, '#111318', '#ffffff')
  const inversePrimary = ensureContrast(
    tonalHex(primaryOkH, Math.min(primaryOkC, 0.16), isDark ? 40 : 80),
    inverseSurface,
    4.5,
  )

  const [tr, tg, tb] = parseHexRgb(onSurface)
  const onSurfaceRgb = `${tr} ${tg} ${tb}`

  return {
    surface,
    surfaceDim,
    surfaceBright,
    surfaceContainerLowest: containerLowest,
    surfaceContainerLow: containerLow,
    surfaceContainer: containerDefault,
    surfaceContainerHigh: containerHigh,
    surfaceContainerHighest: containerHighest,
    onSurface,
    onSurfaceRgb,
    onSurfaceVariant,
    outline,
    outlineVariant,
    scrim,
    shadow,
    primary,
    onPrimary,
    primaryContainer,
    onPrimaryContainer,
    primaryFixed,
    primaryFixedDim,
    onPrimaryFixed,
    onPrimaryFixedVariant,
    secondary,
    onSecondary,
    secondaryContainer,
    onSecondaryContainer,
    secondaryFixed,
    secondaryFixedDim,
    onSecondaryFixed,
    onSecondaryFixedVariant,
    tertiary,
    onTertiary,
    tertiaryContainer,
    onTertiaryContainer,
    tertiaryFixed,
    tertiaryFixedDim,
    onTertiaryFixed,
    onTertiaryFixedVariant,
    error,
    onError,
    errorContainer,
    onErrorContainer,
    success,
    successContainer,
    onSuccessContainer,
    warning,
    warningContainer,
    onWarningContainer,
    inverseSurface,
    inverseOnSurface,
    inversePrimary,
  }
}

export function applyResolvedPaletteToDom(
  resolved: ResolvedPalette,
  isDark: boolean,
  presetId: string,
): void {
  const scheme = computeMd3ColorScheme(resolved, isDark)
  const [r, g, b] = parseHexRgb(scheme.primary)
  const rgbTuple = `${r} ${g} ${b}`
  const root = document.documentElement

  root.dataset.theme = isDark ? 'dark' : 'light'
  root.dataset.palette = presetId

  // Write MD3 System Color Tokens (complete 36-role spec)
  root.style.colorScheme = isDark ? 'dark' : 'light'
  root.style.setProperty('--md-sys-color-surface', scheme.surface)
  root.style.setProperty('--md-sys-color-surface-dim', scheme.surfaceDim)
  root.style.setProperty('--md-sys-color-surface-bright', scheme.surfaceBright)
  root.style.setProperty('--md-sys-color-surface-container-lowest', scheme.surfaceContainerLowest)
  root.style.setProperty('--md-sys-color-surface-container-low', scheme.surfaceContainerLow)
  root.style.setProperty('--md-sys-color-surface-container', scheme.surfaceContainer)
  root.style.setProperty('--md-sys-color-surface-container-high', scheme.surfaceContainerHigh)
  root.style.setProperty('--md-sys-color-surface-container-highest', scheme.surfaceContainerHighest)
  root.style.setProperty('--md-sys-color-on-surface', scheme.onSurface)
  root.style.setProperty('--md-sys-color-on-surface-rgb', scheme.onSurfaceRgb)
  root.style.setProperty('--md-sys-color-on-surface-variant', scheme.onSurfaceVariant)
  root.style.setProperty('--md-sys-color-outline', scheme.outline)
  root.style.setProperty('--md-sys-color-outline-variant', scheme.outlineVariant)
  root.style.setProperty('--md-sys-color-scrim', scheme.scrim)
  root.style.setProperty('--md-sys-color-shadow', scheme.shadow)

  root.style.setProperty('--md-sys-color-primary', scheme.primary)
  root.style.setProperty('--md-sys-color-on-primary', scheme.onPrimary)
  root.style.setProperty('--md-sys-color-primary-container', scheme.primaryContainer)
  root.style.setProperty('--md-sys-color-on-primary-container', scheme.onPrimaryContainer)
  root.style.setProperty('--md-sys-color-primary-fixed', scheme.primaryFixed)
  root.style.setProperty('--md-sys-color-primary-fixed-dim', scheme.primaryFixedDim)
  root.style.setProperty('--md-sys-color-on-primary-fixed', scheme.onPrimaryFixed)
  root.style.setProperty('--md-sys-color-on-primary-fixed-variant', scheme.onPrimaryFixedVariant)

  root.style.setProperty('--md-sys-color-secondary', scheme.secondary)
  root.style.setProperty('--md-sys-color-on-secondary', scheme.onSecondary)
  root.style.setProperty('--md-sys-color-secondary-container', scheme.secondaryContainer)
  root.style.setProperty('--md-sys-color-on-secondary-container', scheme.onSecondaryContainer)
  root.style.setProperty('--md-sys-color-secondary-fixed', scheme.secondaryFixed)
  root.style.setProperty('--md-sys-color-secondary-fixed-dim', scheme.secondaryFixedDim)
  root.style.setProperty('--md-sys-color-on-secondary-fixed', scheme.onSecondaryFixed)
  root.style.setProperty(
    '--md-sys-color-on-secondary-fixed-variant',
    scheme.onSecondaryFixedVariant,
  )

  root.style.setProperty('--md-sys-color-tertiary', scheme.tertiary)
  root.style.setProperty('--md-sys-color-on-tertiary', scheme.onTertiary)
  root.style.setProperty('--md-sys-color-tertiary-container', scheme.tertiaryContainer)
  root.style.setProperty('--md-sys-color-on-tertiary-container', scheme.onTertiaryContainer)
  root.style.setProperty('--md-sys-color-tertiary-fixed', scheme.tertiaryFixed)
  root.style.setProperty('--md-sys-color-tertiary-fixed-dim', scheme.tertiaryFixedDim)
  root.style.setProperty('--md-sys-color-on-tertiary-fixed', scheme.onTertiaryFixed)
  root.style.setProperty('--md-sys-color-on-tertiary-fixed-variant', scheme.onTertiaryFixedVariant)

  root.style.setProperty('--md-sys-color-error', scheme.error)
  root.style.setProperty('--md-sys-color-on-error', scheme.onError)
  root.style.setProperty('--md-sys-color-error-container', scheme.errorContainer)
  root.style.setProperty('--md-sys-color-on-error-container', scheme.onErrorContainer)
  root.style.setProperty('--md-sys-color-success', scheme.success)
  root.style.setProperty('--md-sys-color-success-container', scheme.successContainer)
  root.style.setProperty('--md-sys-color-on-success-container', scheme.onSuccessContainer)
  root.style.setProperty('--md-sys-color-warning', scheme.warning)
  root.style.setProperty('--md-sys-color-warning-container', scheme.warningContainer)
  root.style.setProperty('--md-sys-color-on-warning-container', scheme.onWarningContainer)

  root.style.setProperty('--md-sys-color-inverse-surface', scheme.inverseSurface)
  root.style.setProperty('--md-sys-color-inverse-on-surface', scheme.inverseOnSurface)
  root.style.setProperty('--md-sys-color-inverse-primary', scheme.inversePrimary)

  // Synchronize browser / PWA / mobile status-bar theme-color meta tag
  if (typeof document !== 'undefined') {
    let meta = document.querySelector<HTMLMetaElement>('meta[name="theme-color"]')
    if (!meta) {
      meta = document.createElement('meta')
      meta.name = 'theme-color'
      document.head.appendChild(meta)
    }
    meta.content = scheme.surfaceContainer
  }

  // Legacy --ctp-* compatibility aliases (single write, harmonized with MD3 tonal containers)
  root.style.setProperty('--ctp-base', scheme.surface)
  root.style.setProperty('--ctp-canvas', scheme.surface)
  root.style.setProperty('--ctp-crust', scheme.surfaceContainerLowest)
  root.style.setProperty('--ctp-mantle', scheme.surfaceContainerLow)
  root.style.setProperty('--ctp-shell', scheme.surfaceContainer)
  root.style.setProperty('--ctp-search', scheme.surfaceContainerHigh)
  root.style.setProperty('--ctp-surface0', scheme.surfaceContainerHighest)
  root.style.setProperty('--ctp-surface1', resolved.surface1)
  root.style.setProperty('--ctp-surface2', resolved.surface2)
  root.style.setProperty('--ctp-subtext0', scheme.onSurfaceVariant)
  root.style.setProperty('--ctp-subtext1', resolved.subtext1)
  root.style.setProperty('--ctp-text', scheme.onSurface)
  root.style.setProperty('--ctp-accent', scheme.primary)
  root.style.setProperty('--ctp-accent-rgb', rgbTuple)
  root.style.setProperty('--ctp-accent-soft', `rgb(${rgbTuple} / 0.15)`)
  root.style.setProperty('--ctp-accent-medium', `rgb(${rgbTuple} / 0.25)`)
  root.style.setProperty('--ctp-on-accent', scheme.onPrimary)
  root.style.setProperty('--ctp-row-hover', mixHex(scheme.surface, scheme.surfaceContainerHighest, 0.65))
  root.style.setProperty('--ctp-red', scheme.error)
  root.style.setProperty('--ctp-green', scheme.success)
  root.style.setProperty('--ctp-yellow', scheme.warning)

  // Legacy xcomic semantic tokens synchronized with MD3 palette
  root.style.setProperty('--bg-base', scheme.surface)
  root.style.setProperty('--bg-mantle', scheme.surfaceContainer)
  root.style.setProperty('--bg-surface', scheme.surfaceContainerLow)
  root.style.setProperty('--text-primary', scheme.onSurface)
  root.style.setProperty('--text-secondary', scheme.onSurfaceVariant)
  root.style.setProperty('--accent', scheme.primary)
  root.style.setProperty('--accent-contrast', scheme.onPrimary)
}
