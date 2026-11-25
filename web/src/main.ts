import './sources/registerBuiltinDomains'
import { createApp } from 'vue'
import { createPinia } from 'pinia'
import App from './App.vue'
import { router } from './router'
import { i18n } from './i18n'
import { usePreferencesStore } from './stores/preferences'
import { registerThemeCloudSync, useThemeStore } from './stores/theme'
import './styles/main.css'

const pinia = createPinia()
const app = createApp(App)
app.use(pinia)

const themeStore = useThemeStore(pinia)
const preferencesStore = usePreferencesStore(pinia)
preferencesStore.registerVisualProvider(() => themeStore.getVisualSnapshot())
registerThemeCloudSync({
  onVisualChange: () => {
    preferencesStore.persistAndSync()
  },
  onHydrateUser: (prefs) => {
    preferencesStore.hydrateDomainPreferences(prefs)
    preferencesStore.persistLocal()
  },
})

app.use(i18n)
app.use(router)
app.mount('#app')
