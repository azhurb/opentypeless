import { useEffect, useState } from 'react'
import i18n from './i18n'
import { useTauriEvents } from './hooks/useTauriEvents'
import { useTheme } from './hooks/useTheme'
import { useDetectedLanguageNotifier } from './hooks/useDetectedLanguageNotifier'
import { useAppStore } from './stores/appStore'
import { useRoute } from './lib/router'
import {
  loadOnboardingCompleted,
  getConfig,
  getHistory,
  getDictionary,
  checkAccessibilityPermission,
  checkMicrophonePermission,
  requestMicrophonePermission,
} from './lib/tauri'
import { Capsule } from './components/Capsule'
import { Settings } from './components/Settings'
import { History } from './components/History'
import { Onboarding } from './components/Onboarding'
import { MainLayout } from './components/MainLayout'
import { HomePage } from './components/HomePage'
import { ToastContainer } from './components/Toast'

function CapsuleApp() {
  useTauriEvents()
  useTheme()

  const setConfig = useAppStore((s) => s.setConfig)

  useEffect(() => {
    // Load config so DurationTimer gets the correct max_recording_seconds
    getConfig()
      .then((config) => {
        setConfig(config)
        // Restore UI language from config
        if (config.ui_language && config.ui_language !== i18n.language) {
          i18n.changeLanguage(config.ui_language)
          localStorage.setItem('ui_language', config.ui_language)
        }
      })
      .catch((e) => {
        console.error('Failed to load config in capsule:', e)
      })
  }, [setConfig])

  // Window show is handled by useCapsuleResize (setSize → setPosition → show),
  // which works on both Windows and macOS. The previous rAF-based show approach
  // failed on macOS because WKWebView pauses requestAnimationFrame in hidden windows.
  return <Capsule />
}

function MainApp() {
  useTauriEvents()
  useTheme()
  useDetectedLanguageNotifier()

  const onboardingCompleted = useAppStore((s) => s.onboardingCompleted)
  const setOnboardingCompleted = useAppStore((s) => s.setOnboardingCompleted)
  const setConfig = useAppStore((s) => s.setConfig)
  const setSavedConfig = useAppStore((s) => s.setSavedConfig)
  const setHistory = useAppStore((s) => s.setHistory)
  const setDictionary = useAppStore((s) => s.setDictionary)
  const setAccessibilityTrusted = useAppStore((s) => s.setAccessibilityTrusted)
  const setMicAuthStatus = useAppStore((s) => s.setMicAuthStatus)
  const [loaded, setLoaded] = useState(false)
  const [loadError, setLoadError] = useState(false)
  const { route } = useRoute()

  useEffect(() => {
    loadOnboardingCompleted().then(async (done) => {
      setOnboardingCompleted(done)
      try {
        // Always pull the current config so onboarding's STT / LLM steps
        // pre-populate from any values that are already on disk. Without
        // this the Zustand store keeps `defaultConfig` (empty keys), and
        // re-running onboarding silently overwrites the user's real keys
        // when the final step saves.
        const config = await getConfig()
        setConfig(config)
        setSavedConfig(config)
        if (config.ui_language && config.ui_language !== i18n.language) {
          i18n.changeLanguage(config.ui_language)
          localStorage.setItem('ui_language', config.ui_language)
        }

        // History and dictionary are post-onboarding views — skip them
        // during the flow so we don't pay the I/O for nothing.
        if (done) {
          const [history, dictionary] = await Promise.all([getHistory(200, 0), getDictionary()])
          setHistory(history)
          setDictionary(dictionary)
        }

        if (navigator.platform.toUpperCase().indexOf('MAC') >= 0) {
          checkAccessibilityPermission().then(setAccessibilityTrusted)
          checkMicrophonePermission().then(async (status) => {
            // Auto-prompt only for already-onboarded users. While the user
            // is mid-onboarding the PermissionsStep owns the interactive
            // grant moment; auto-firing here would race that step's
            // Grant button and confuse the order of dialogs.
            if (status === 'not_determined' && done) {
              await requestMicrophonePermission()
              const next = await checkMicrophonePermission()
              setMicAuthStatus(next)
            } else {
              setMicAuthStatus(status)
            }
          })
        }
      } catch (e) {
        console.error('Failed to load initial data:', e)
        setLoadError(true)
      }
      setLoaded(true)
    })
  }, [
    setOnboardingCompleted,
    setConfig,
    setSavedConfig,
    setHistory,
    setDictionary,
    setAccessibilityTrusted,
    setMicAuthStatus,
  ])

  if (!loaded)
    return (
      <div className="flex items-center justify-center h-screen">
        <span className="text-text-tertiary text-[13px]">Loading...</span>
      </div>
    )
  if (loadError)
    return (
      <div className="flex flex-col items-center justify-center h-screen gap-3">
        <span className="text-error text-[13px]">Failed to load application data.</span>
        <button
          onClick={() => window.location.reload()}
          className="px-4 py-2 bg-accent text-white rounded-[10px] text-[13px] border-none cursor-pointer hover:bg-accent-hover transition-colors"
        >
          Retry
        </button>
      </div>
    )
  if (!onboardingCompleted) return <Onboarding />

  return (
    <MainLayout>
      {route === 'home' && <HomePage />}
      {route === 'settings' && <Settings />}
      {route === 'history' && <History />}
      <ToastContainer />
    </MainLayout>
  )
}

function App() {
  // Capsule window loads with #capsule hash — detect synchronously, no race condition
  if (window.location.hash === '#capsule') return <CapsuleApp />
  return <MainApp />
}

export default App
