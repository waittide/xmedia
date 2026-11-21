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
  if (clean.length === 3) {
    const r = Number.parseInt(clean[0]! + clean[0]!, 16)
    const g = Number.parseInt(clean[1]! + clean[1]!, 16)
    const b = Number.parseInt(clean[2]! + clean[2]!, 16)
    return [r || 0, g || 0, b || 0]
  }
  const num = Number.parseInt(clean.padEnd(6, '0').slice(0, 6), 16)
  if (Number.isNaN(num)) return [128, 128, 128]
  return [(num >> 16) & 255, (num >> 8) & 255, num & 255]
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
  return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4
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

/** Ensure fgHex meets at least minRatio (default WCAG AA 4.5:1) against bgHex. */
function ensureContrast(fgHex: string, bgHex: string, minRatio = 4.5): string {
  if (contrastRatio(fgHex, bgHex) >= minRatio) return fgHex
  const target = chooseContrastText(bgHex, '#0b0d14', '#ffffff')
  let low = 0
  let high = 1
  let best = target
  for (let i = 0; i < 10; i++) {
    const mid = (low + high) / 2
    const candidate = mixHex(target, fgHex, mid)
    if (contrastRatio(candidate, bgHex) >= minRatio) {
      best = candidate
      high = mid
    } else {
      low = mid
    }
  }
  return best
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
  const trimmed = input.trim()
  if (/^#[0-9a-fA-F]{6}$/.test(trimmed)) {
    return trimmed.toLowerCase()
  }
  if (/^[0-9a-fA-F]{6}$/.test(trimmed)) {
    return `#${trimmed.toLowerCase()}`
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

export function applyResolvedPaletteToDom(
  resolved: ResolvedPalette,
  isDark: boolean,
  presetId: string,
): void {
  const [r, g, b] = parseHexRgb(resolved.accent)
  const rgbTuple = `${r} ${g} ${b}`
  const [primaryH, primaryS] = rgbToHsl(r, g, b)
  const root = document.documentElement

  root.dataset.theme = isDark ? 'dark' : 'light'
  root.dataset.palette = presetId

  // Material Design 3 (Material You) monotonic tonal surface & container tokens
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
    // Never allow a dark Catppuccin shell/crust (darker than canvas) to invert container hierarchy.
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

  // MD3 Primary, Secondary & Tertiary Roles (hue-shifted from primary, uncoupled from warning/success)
  const primary = resolved.accent
  const onPrimary = chooseContrastText(primary)
  const primaryContainer = isDark
    ? mixHex(primary, surface, 0.34)
    : mixHex(primary, '#ffffff', 0.22)
  const onPrimaryContainer = ensureContrast(
    isDark ? mixHex(primary, '#ffffff', 0.24) : mixHex(primary, '#081020', 0.42),
    primaryContainer,
  )

  const secondarySat = primaryS < 0.06 ? primaryS : Math.max(0.24, Math.min(0.52, primaryS * 0.64))
  const secondary = hslToHex((primaryH + 6) % 360, secondarySat, isDark ? 0.78 : 0.4)
  const onSecondary = chooseContrastText(secondary)
  const secondaryContainer = isDark
    ? mixHex(primary, containerHigh, 0.24)
    : mixHex(primary, '#ffffff', 0.16)
  const onSecondaryContainer = ensureContrast(
    isDark ? mixHex(primary, '#ffffff', 0.2) : mixHex(primary, '#0c1424', 0.36),
    secondaryContainer,
  )

  const tertiarySat = primaryS < 0.06 ? 0.24 : Math.max(0.32, Math.min(0.62, primaryS * 0.76))
  const tertiary = hslToHex((primaryH + 60) % 360, tertiarySat, isDark ? 0.78 : 0.38)
  const onTertiary = chooseContrastText(tertiary)
  const tertiaryContainer = isDark
    ? mixHex(tertiary, surface, 0.3)
    : mixHex(tertiary, '#ffffff', 0.18)
  const onTertiaryContainer = ensureContrast(
    isDark ? mixHex(tertiary, '#ffffff', 0.22) : mixHex(tertiary, '#10141c', 0.4),
    tertiaryContainer,
  )

  // Semantic roles (Error / Success / Warning) with guaranteed WCAG AA contrast
  const onError = chooseContrastText(resolved.red)
  const errorContainer = isDark
    ? mixHex(resolved.red, surface, 0.28)
    : mixHex(resolved.red, '#ffffff', 0.15)
  const onErrorContainer = ensureContrast(
    isDark ? mixHex(resolved.red, '#ffffff', 0.22) : mixHex(resolved.red, '#2b0609', 0.42),
    errorContainer,
  )

  const successContainer = isDark
    ? mixHex(resolved.green, surface, 0.26)
    : mixHex(resolved.green, '#ffffff', 0.16)
  const onSuccessContainer = ensureContrast(
    isDark ? mixHex(resolved.green, '#ffffff', 0.22) : mixHex(resolved.green, '#09210e', 0.4),
    successContainer,
  )

  const warningContainer = isDark
    ? mixHex(resolved.yellow, surface, 0.26)
    : mixHex(resolved.yellow, '#ffffff', 0.18)
  const onWarningContainer = ensureContrast(
    isDark ? mixHex(resolved.yellow, '#ffffff', 0.22) : mixHex(resolved.yellow, '#2c1900', 0.4),
    warningContainer,
  )

  const outline = mixHex(resolved.subtext0, surface, 0.65)
  const outlineVariant = mixHex(resolved.surface0, surface, 0.72)
  const scrim = '#000000'

  const inverseSurface = isDark
    ? mixHex(resolved.text, primary, 0.92)
    : mixHex(resolved.text, primary, 0.9)
  const inverseOnSurface = chooseContrastText(inverseSurface)
  const inversePrimary = ensureContrast(
    isDark ? mixHex(primary, resolved.canvas, 0.62) : mixHex(primary, '#ffffff', 0.52),
    inverseSurface,
  )

  const [tr, tg, tb] = parseHexRgb(resolved.text)
  const onSurfaceRgb = `${tr} ${tg} ${tb}`

  // Write MD3 System Color Tokens
  root.style.colorScheme = isDark ? 'dark' : 'light'
  root.style.setProperty('--md-sys-color-surface', surface)
  root.style.setProperty('--md-sys-color-surface-dim', surfaceDim)
  root.style.setProperty('--md-sys-color-surface-bright', surfaceBright)
  root.style.setProperty('--md-sys-color-surface-container-lowest', containerLowest)
  root.style.setProperty('--md-sys-color-surface-container-low', containerLow)
  root.style.setProperty('--md-sys-color-surface-container', containerDefault)
  root.style.setProperty('--md-sys-color-surface-container-high', containerHigh)
  root.style.setProperty('--md-sys-color-surface-container-highest', containerHighest)
  root.style.setProperty('--md-sys-color-on-surface', resolved.text)
  root.style.setProperty('--md-sys-color-on-surface-rgb', onSurfaceRgb)
  root.style.setProperty('--md-sys-color-on-surface-variant', resolved.subtext0)
  root.style.setProperty('--md-sys-color-outline', outline)
  root.style.setProperty('--md-sys-color-outline-variant', outlineVariant)
  root.style.setProperty('--md-sys-color-scrim', scrim)

  root.style.setProperty('--md-sys-color-primary', primary)
  root.style.setProperty('--md-sys-color-on-primary', onPrimary)
  root.style.setProperty('--md-sys-color-primary-container', primaryContainer)
  root.style.setProperty('--md-sys-color-on-primary-container', onPrimaryContainer)
  root.style.setProperty('--md-sys-color-secondary', secondary)
  root.style.setProperty('--md-sys-color-on-secondary', onSecondary)
  root.style.setProperty('--md-sys-color-secondary-container', secondaryContainer)
  root.style.setProperty('--md-sys-color-on-secondary-container', onSecondaryContainer)
  root.style.setProperty('--md-sys-color-tertiary', tertiary)
  root.style.setProperty('--md-sys-color-on-tertiary', onTertiary)
  root.style.setProperty('--md-sys-color-tertiary-container', tertiaryContainer)
  root.style.setProperty('--md-sys-color-on-tertiary-container', onTertiaryContainer)

  root.style.setProperty('--md-sys-color-error', resolved.red)
  root.style.setProperty('--md-sys-color-on-error', onError)
  root.style.setProperty('--md-sys-color-error-container', errorContainer)
  root.style.setProperty('--md-sys-color-on-error-container', onErrorContainer)
  root.style.setProperty('--md-sys-color-success', resolved.green)
  root.style.setProperty('--md-sys-color-success-container', successContainer)
  root.style.setProperty('--md-sys-color-on-success-container', onSuccessContainer)
  root.style.setProperty('--md-sys-color-warning', resolved.yellow)
  root.style.setProperty('--md-sys-color-warning-container', warningContainer)
  root.style.setProperty('--md-sys-color-on-warning-container', onWarningContainer)

  root.style.setProperty('--md-sys-color-inverse-surface', inverseSurface)
  root.style.setProperty('--md-sys-color-inverse-on-surface', inverseOnSurface)
  root.style.setProperty('--md-sys-color-inverse-primary', inversePrimary)

  // Legacy --ctp-* compatibility aliases (single write, harmonized with MD3 tonal containers)
  root.style.setProperty('--ctp-base', surface)
  root.style.setProperty('--ctp-canvas', surface)
  root.style.setProperty('--ctp-crust', containerLowest)
  root.style.setProperty('--ctp-mantle', containerLow)
  root.style.setProperty('--ctp-shell', containerDefault)
  root.style.setProperty('--ctp-search', containerHigh)
  root.style.setProperty('--ctp-surface0', containerHighest)
  root.style.setProperty('--ctp-surface1', resolved.surface1)
  root.style.setProperty('--ctp-surface2', resolved.surface2)
  root.style.setProperty('--ctp-subtext0', resolved.subtext0)
  root.style.setProperty('--ctp-subtext1', resolved.subtext1)
  root.style.setProperty('--ctp-text', resolved.text)
  root.style.setProperty('--ctp-accent', primary)
  root.style.setProperty('--ctp-accent-rgb', rgbTuple)
  root.style.setProperty('--ctp-accent-soft', `rgb(${rgbTuple} / 0.15)`)
  root.style.setProperty('--ctp-accent-medium', `rgb(${rgbTuple} / 0.25)`)
  root.style.setProperty('--ctp-on-accent', onPrimary)
  root.style.setProperty('--ctp-row-hover', mixHex(surface, containerHighest, 0.65))
  root.style.setProperty('--ctp-red', resolved.red)
  root.style.setProperty('--ctp-green', resolved.green)
  root.style.setProperty('--ctp-yellow', resolved.yellow)

  // Legacy xcomic semantic tokens synchronized with MD3 palette
  root.style.setProperty('--bg-base', surface)
  root.style.setProperty('--bg-mantle', containerDefault)
  root.style.setProperty('--bg-surface', containerLow)
  root.style.setProperty('--text-primary', resolved.text)
  root.style.setProperty('--text-secondary', resolved.subtext0)
  root.style.setProperty('--accent', primary)
  root.style.setProperty('--accent-contrast', onPrimary)
}
