import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import './index.css'
import App from './App.tsx'
import { registerBundle } from './i18n'
import zh from './i18n/zh'
import en from './i18n/en'
import { ThemeProvider, ToastProvider } from './components/shared'

registerBundle('zh', zh as any)
registerBundle('en', en as any)

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <ThemeProvider>
      <ToastProvider>
        <App />
      </ToastProvider>
    </ThemeProvider>
  </StrictMode>,
)
