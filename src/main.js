import { createApp } from 'vue'
import { createI18n } from 'vue-i18n'
import App from './App.vue'
import './styles/globals.css'
import es from './locales/es'
import en from './locales/en'

const i18n = createI18n({
  legacy: false, // Composition API mode required for useI18n() in <script setup>
  locale: 'es',  // overwritten by useLocale once the persisted value loads
  fallbackLocale: 'es',
  messages: { es, en }
})

createApp(App).use(i18n).mount('#app')
