import { i18n } from './index'

const EXACT_ERROR_KEYS: Record<string, string> = {
  'mount path cannot overlap with xmedia data directory': 'apiErrors.mountOverlapDataDir',
  'mount path is outside the configured allowed mount prefixes': 'apiErrors.mountOutsideAllowed',
  'library root path cannot be empty': 'apiErrors.libraryPathEmpty',
  'library name must be between 1 and 128 characters': 'apiErrors.libraryNameLength',
  'managed upload library does not require directory scanning': 'apiErrors.managedLibraryNoScan',
  'library scan is already in progress': 'apiErrors.libraryScanInProgress',
  'default managed library cannot be deleted': 'apiErrors.managedLibraryNoDelete',
  'library not found': 'apiErrors.libraryNotFound',
  'scan interrupted by server restart': 'apiErrors.scanInterruptedRestart',
  'library scan aborted or terminated unexpectedly': 'apiErrors.scanAbortedUnexpectedly',
  'invalid username or password': 'apiErrors.invalidCredentials',
  'wrong username or password': 'apiErrors.invalidCredentials',
  'invalid credentials': 'apiErrors.invalidCredentials',
  'current password is incorrect': 'apiErrors.currentPasswordIncorrect',
  'username already exists': 'apiErrors.usernameExists',
  'initial administrator has already been configured': 'apiErrors.adminAlreadyConfigured',
  'too many failed authentication attempts; please try again later': 'apiErrors.rateLimitedAuth',
  'account has been disabled': 'apiErrors.accountDisabled',
  'administrator privileges required': 'apiErrors.adminRequired',
  'authentication required': 'apiErrors.authRequired',
  'token has been revoked': 'apiErrors.authRequired',
  'user no longer exists': 'apiErrors.authRequired',
  'session has been invalidated by a credential change': 'apiErrors.authRequired',
  'username length must be between 2 and 64 characters': 'apiErrors.usernameLength',
  "username may only contain ASCII letters, digits, '_', '-', or '.'": 'apiErrors.usernameChars',
  'password length must be between 6 and 128 characters': 'apiErrors.passwordLength',
  'password cannot consist solely of whitespace': 'apiErrors.passwordWhitespace',
  'display name cannot exceed 128 characters': 'apiErrors.displayNameLength',
  'display name must be between 1 and 128 characters': 'apiErrors.displayNameLength',
  'cannot deactivate your own account': 'apiErrors.cannotDeactivateSelf',
  'cannot demote your own admin role': 'apiErrors.cannotDemoteSelf',
  'cannot delete your own account': 'apiErrors.cannotDeleteSelf',
  'user not found': 'apiErrors.userNotFound',
  'target user not found': 'apiErrors.userNotFound',
  'no active local administrator account found': 'apiErrors.noActiveLocalAdmin',
  'font file cannot be empty': 'apiErrors.fontEmpty',
  'font payload cannot be empty': 'apiErrors.fontEmpty',
  'unrecognized font file header (too short)': 'apiErrors.fontUnsupported',
  'unsupported or unrecognized font magic bytes': 'apiErrors.fontUnsupported',
  'multipart form must include at least one font file': 'apiErrors.fontMissingFile',
  'custom font name field is too large': 'apiErrors.fontNameTooLarge',
  'inbox directory cannot overlap xmedia-tool data directory': 'apiErrors.inboxOverlapDataDir',
  'managed root cannot overlap xmedia-tool data directory': 'apiErrors.rootOverlapDataDir',
  'Internal storage error': 'apiErrors.internalStorage',
  'Internal database error': 'apiErrors.internalDatabase',
  'Expected JSON response from server': 'apiErrors.expectedJson',
  'Malformed JSON response from server': 'apiErrors.malformedJson',
}

interface PatternErrorRule {
  pattern: RegExp
  key: string
  params?: (match: RegExpExecArray) => Record<string, unknown>
}

const PATTERN_ERROR_RULES: PatternErrorRule[] = [
  {
    pattern: /^mount path overlaps with existing library '(.+)'$/i,
    key: 'apiErrors.mountOverlapExisting',
    params: (m) => ({ name: m[1] ?? '' }),
  },
  {
    pattern: /^mount directory does not exist or is not a directory:\s*(.+)$/i,
    key: 'apiErrors.mountDirMissing',
    params: (m) => ({ path: m[1] ?? '' }),
  },
  {
    pattern: /^mount directory '(.+)' is offline or missing;/i,
    key: 'apiErrors.mountDirOffline',
    params: (m) => ({ path: m[1] ?? '' }),
  },
  {
    pattern: /^maximum number of stored custom fonts \((\d+)\) reached$/i,
    key: 'apiErrors.fontLimitReached',
    params: (m) => ({ max: m[1] ?? '' }),
  },
  {
    pattern: /^(?:uploaded font|font file) exceeds maximum allowed size/i,
    key: 'apiErrors.fontTooLarge',
  },
  {
    pattern: /^custom font .+ not found$/i,
    key: 'apiErrors.fontNotFound',
  },
  {
    pattern: /^media module '(.+)' is not enabled in this build$/i,
    key: 'apiErrors.moduleDisabled',
    params: (m) => ({ module: m[1] ?? '' }),
  },
]

export function localizeApiErrorMessage(rawMessage: string): string {
  const trimmed = rawMessage.trim()
  if (!trimmed) return rawMessage

  const exactKey = EXACT_ERROR_KEYS[trimmed]
  if (exactKey && i18n.global.te(exactKey)) {
    return i18n.global.t(exactKey)
  }

  for (const rule of PATTERN_ERROR_RULES) {
    const match = rule.pattern.exec(trimmed)
    if (match && i18n.global.te(rule.key)) {
      return rule.params
        ? i18n.global.t(rule.key, rule.params(match))
        : i18n.global.t(rule.key)
    }
  }

  return rawMessage
}
