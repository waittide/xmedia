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
    defaultAccent: '#b4632a',
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
    defaultAccent: '#907aa9',
    tokens: {
      canvas: '#faf4ed',
      shell: '#f2e9e1',
      surface: '#dfdad9',
      text: '#575279',
      subtext: '#797593',
    },
    semantic: { red: '#b4637a', green: '#286983', yellow: '#ea9d34' },
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

function mixHex(hexA: string, hexB: string, weightA: number): string {
  const [r1, g1, b1] = parseHexRgb(hexA)
  const [r2, g2, b2] = parseHexRgb(hexB)
  const w = Math.max(0, Math.min(1, weightA))
  return rgbToHex(r1 * w + r2 * (1 - w), g1 * w + g2 * (1 - w), b1 * w + b2 * (1 - w))
}

function chooseContrastText(bgHex: string): string {
  const [r, g, b] = parseHexRgb(bgHex)
  const luminance = (0.299 * r + 0.587 * g + 0.114 * b) / 255
  return luminance > 0.58 ? '#11111b' : '#ffffff'
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
  const root = document.documentElement

  root.dataset.theme = isDark ? 'dark' : 'light'
  root.dataset.palette = presetId

  root.style.setProperty('--ctp-base', resolved.canvas)
  root.style.setProperty('--ctp-mantle', resolved.mantle)
  root.style.setProperty('--ctp-crust', resolved.shell)
  root.style.setProperty('--ctp-surface0', resolved.surface0)
  root.style.setProperty('--ctp-surface1', resolved.surface1)
  root.style.setProperty('--ctp-surface2', resolved.surface2)
  root.style.setProperty('--ctp-subtext0', resolved.subtext0)
  root.style.setProperty('--ctp-subtext1', resolved.subtext1)
  root.style.setProperty('--ctp-text', resolved.text)
  root.style.setProperty('--ctp-accent', resolved.accent)
  root.style.setProperty('--ctp-accent-rgb', rgbTuple)
  root.style.setProperty('--ctp-accent-soft', `rgb(${rgbTuple} / 0.15)`)
  root.style.setProperty('--ctp-accent-medium', `rgb(${rgbTuple} / 0.25)`)
  root.style.setProperty('--ctp-on-accent', resolved.onAccent)
  root.style.setProperty('--ctp-canvas', resolved.canvas)
  root.style.setProperty('--ctp-shell', resolved.shell)
  root.style.setProperty('--ctp-search', resolved.mantle)
  root.style.setProperty('--ctp-row-hover', mixHex(resolved.canvas, resolved.surface0, 0.65))
  root.style.setProperty('--ctp-red', resolved.red)
  root.style.setProperty('--ctp-green', resolved.green)
  root.style.setProperty('--ctp-yellow', resolved.yellow)

  // Material Design 3 (Material You) dynamic tonal surface & container tokens
  const surface = resolved.canvas
  const surfaceDim = isDark
    ? mixHex(surface, '#000000', 0.86)
    : mixHex(surface, resolved.surface0, 0.62)
  const surfaceBright = isDark
    ? mixHex(surface, '#ffffff', 0.86)
    : mixHex(surface, '#ffffff', 0.25)

  const containerLowest = isDark
    ? mixHex(surface, '#000000', 0.72)
    : mixHex(surface, '#ffffff', 0.15)
  const containerLow = isDark
    ? mixHex(mixHex(surface, resolved.shell, 0.35), resolved.accent, 0.96)
    : mixHex(mixHex(surface, resolved.shell, 0.45), resolved.accent, 0.97)
  const containerDefault = isDark
    ? mixHex(resolved.shell, resolved.accent, 0.94)
    : mixHex(resolved.shell, resolved.accent, 0.95)
  const containerHigh = isDark
    ? mixHex(mixHex(resolved.shell, resolved.surface0, 0.5), resolved.accent, 0.91)
    : mixHex(mixHex(resolved.shell, resolved.surface0, 0.62), resolved.accent, 0.93)
  const containerHighest = isDark
    ? mixHex(resolved.surface0, resolved.accent, 0.88)
    : mixHex(resolved.surface0, resolved.accent, 0.90)

  const primaryContainer = isDark
    ? mixHex(resolved.accent, surface, 0.34)
    : mixHex(resolved.accent, '#ffffff', 0.18)
  const onPrimaryContainer = isDark
    ? mixHex(resolved.accent, '#ffffff', 0.22)
    : mixHex(resolved.accent, '#081020', 0.45)

  const secondaryContainer = isDark
    ? mixHex(resolved.accent, containerHighest, 0.24)
    : mixHex(resolved.accent, containerHigh, 0.16)
  const onSecondaryContainer = isDark
    ? mixHex(resolved.text, '#ffffff', 0.78)
    : mixHex(resolved.accent, '#10141c', 0.32)

  const tertiarySeed = isDark ? resolved.yellow : resolved.green
  const tertiaryContainer = isDark
    ? mixHex(tertiarySeed, surface, 0.24)
    : mixHex(tertiarySeed, '#ffffff', 0.16)
  const onTertiaryContainer = isDark
    ? mixHex(tertiarySeed, '#ffffff', 0.25)
    : mixHex(tertiarySeed, '#0e1a12', 0.45)

  const onError = chooseContrastText(resolved.red)
  const errorContainer = isDark
    ? mixHex(resolved.red, surface, 0.28)
    : mixHex(resolved.red, '#ffffff', 0.14)
  const onErrorContainer = isDark
    ? mixHex(resolved.red, '#ffffff', 0.2)
    : mixHex(resolved.red, '#2b0609', 0.45)

  const successContainer = isDark
    ? mixHex(resolved.green, surface, 0.24)
    : mixHex(resolved.green, '#ffffff', 0.16)
  const onSuccessContainer = isDark
    ? mixHex(resolved.green, '#ffffff', 0.2)
    : mixHex(resolved.green, '#09210e', 0.42)

  const warningContainer = isDark
    ? mixHex(resolved.yellow, surface, 0.24)
    : mixHex(resolved.yellow, '#ffffff', 0.18)
  const onWarningContainer = isDark
    ? mixHex(resolved.yellow, '#ffffff', 0.22)
    : mixHex(resolved.yellow, '#2c1900', 0.42)

  const outline = mixHex(resolved.subtext0, surface, 0.65)
  const outlineVariant = mixHex(resolved.surface0, surface, 0.72)

  const inverseSurface = isDark
    ? mixHex(resolved.text, resolved.accent, 0.9)
    : mixHex(resolved.text, resolved.accent, 0.88)
  const inverseOnSurface = chooseContrastText(inverseSurface)
  const inversePrimary = isDark
    ? mixHex(resolved.accent, resolved.canvas, 0.68)
    : mixHex(resolved.accent, '#ffffff', 0.55)

  const [tr, tg, tb] = parseHexRgb(resolved.text)
  const onSurfaceRgb = [tr, tg, tb].join(' ')

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

  root.style.setProperty('--md-sys-color-primary', resolved.accent)
  root.style.setProperty('--md-sys-color-on-primary', resolved.onAccent)
  root.style.setProperty('--md-sys-color-primary-container', primaryContainer)
  root.style.setProperty('--md-sys-color-on-primary-container', onPrimaryContainer)
  root.style.setProperty('--md-sys-color-secondary-container', secondaryContainer)
  root.style.setProperty('--md-sys-color-on-secondary-container', onSecondaryContainer)
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

  // Keep legacy --ctp-shell / --ctp-mantle harmonized with MD3 tonal containers
  root.style.setProperty('--ctp-shell', containerDefault)
  root.style.setProperty('--ctp-mantle', containerLow)
  root.style.setProperty('--ctp-search', containerHigh)

  // Synchronize xcomic semantic tokens to the exact same resolved palette
  root.style.colorScheme = isDark ? 'dark' : 'light'
  root.style.setProperty('--bg-base', surface)
  root.style.setProperty('--bg-mantle', containerDefault)
  root.style.setProperty('--bg-surface', containerLow)
  root.style.setProperty('--text-primary', resolved.text)
  root.style.setProperty('--text-secondary', resolved.subtext0)
  root.style.setProperty('--accent', resolved.accent)
  root.style.setProperty('--accent-contrast', resolved.onAccent)
}
