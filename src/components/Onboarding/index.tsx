import { AnimatePresence, motion } from 'framer-motion'
import { useAppStore } from '../../stores/appStore'
import { saveOnboardingCompleted, updateConfig as saveConfig } from '../../lib/tauri'
import { OnboardingLayout } from './OnboardingLayout'
import { WelcomeStep } from './WelcomeStep'
import { SttSetupStep } from './SttSetupStep'
import { LlmSetupStep } from './LlmSetupStep'
import { PermissionsStep } from './PermissionsStep'
import { QuickTestStep } from './QuickTestStep'
import { DoneStep } from './DoneStep'
import { slideRight } from '../../lib/animations'

const isMac =
  typeof navigator !== 'undefined' && navigator.platform.toUpperCase().indexOf('MAC') >= 0

// Permissions step exists only on macOS — Linux/Windows don't need the grants.
const TOTAL_STEPS = isMac ? 6 : 5

// Index map: on macOS we add Permissions between LLM and QuickTest.
// macOS:    0 Welcome | 1 STT | 2 LLM | 3 Permissions | 4 QuickTest | 5 Done
// non-mac:  0 Welcome | 1 STT | 2 LLM | 3 QuickTest   | 4 Done
const STEP_PERMISSIONS = isMac ? 3 : -1
const STEP_QUICK_TEST = isMac ? 4 : 3
const STEP_DONE = isMac ? 5 : 4

export function Onboarding() {
  const step = useAppStore((s) => s.onboardingStep)
  const setStep = useAppStore((s) => s.setOnboardingStep)
  const setOnboardingCompleted = useAppStore((s) => s.setOnboardingCompleted)
  const sttTestStatus = useAppStore((s) => s.sttTestStatus)
  const llmTestStatus = useAppStore((s) => s.llmTestStatus)

  const canNext = (() => {
    if (step === 0) return true // Welcome
    if (step === 1) return sttTestStatus === 'success'
    if (step === 2) return llmTestStatus === 'success'
    if (step === STEP_PERMISSIONS) return true // optional grants
    if (step === STEP_QUICK_TEST) return true
    if (step === STEP_DONE) return true
    return false
  })()

  const titles: Record<number, { title: string; subtitle?: string }> = {
    0: {
      title: 'Welcome to OpenTypeless',
      subtitle: 'A few quick steps to get started with voice input',
    },
    1: {
      title: 'Speech Recognition',
      subtitle: 'Configure your ASR service to convert speech to text',
    },
    2: {
      title: 'AI Polish',
      subtitle: 'Configure an LLM service to polish transcribed text',
    },
    [STEP_QUICK_TEST]: {
      title: 'How It Works',
      subtitle: 'See the full pipeline in action — from voice to polished text',
    },
    [STEP_DONE]: { title: 'Setup Complete', subtitle: undefined },
  }
  if (STEP_PERMISSIONS >= 0) {
    titles[STEP_PERMISSIONS] = {
      title: 'macOS Permissions',
      subtitle: 'Grant Microphone and Accessibility so dictation works on first try',
    }
  }

  const config = useAppStore((s) => s.config)

  const handleNext = async () => {
    if (step < TOTAL_STEPS - 1) {
      try {
        await saveConfig(config)
      } catch {
        // Best-effort save — continue navigation even if save fails
      }
      setStep(step + 1)
    } else {
      await saveConfig(config)
      await saveOnboardingCompleted()
      setOnboardingCompleted(true)
    }
  }

  const handleBack = async () => {
    if (step > 0) {
      try {
        await saveConfig(config)
      } catch {
        // Best-effort save
      }
      setStep(step - 1)
    }
  }

  const handleSkip = async () => {
    await saveConfig(config)
    await saveOnboardingCompleted()
    setOnboardingCompleted(true)
  }

  return (
    <OnboardingLayout
      step={step}
      totalSteps={TOTAL_STEPS}
      title={titles[step].title}
      subtitle={titles[step].subtitle}
      canNext={canNext}
      canBack={step > 0}
      nextLabel={step === TOTAL_STEPS - 1 ? 'Get Started' : 'Next'}
      onNext={handleNext}
      onBack={handleBack}
      onSkip={handleSkip}
    >
      <AnimatePresence mode="wait">
        <motion.div
          key={step}
          variants={slideRight}
          initial="initial"
          animate="animate"
          exit="exit"
          transition={{ duration: 0.2 }}
        >
          {step === 0 && <WelcomeStep />}
          {step === 1 && <SttSetupStep />}
          {step === 2 && <LlmSetupStep />}
          {step === STEP_PERMISSIONS && <PermissionsStep />}
          {step === STEP_QUICK_TEST && <QuickTestStep />}
          {step === STEP_DONE && <DoneStep />}
        </motion.div>
      </AnimatePresence>
    </OnboardingLayout>
  )
}
