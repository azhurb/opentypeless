import { AnimatePresence, motion } from 'framer-motion'
import { useAppStore } from '../../stores/appStore'
import { saveOnboardingCompleted, updateConfig as saveConfig } from '../../lib/tauri'
import { OnboardingLayout } from './OnboardingLayout'
import { WelcomeStep } from './WelcomeStep'
import { SttSetupStep } from './SttSetupStep'
import { LlmSetupStep } from './LlmSetupStep'
import { QuickTestStep } from './QuickTestStep'
import { DoneStep } from './DoneStep'
import { slideRight } from '../../lib/animations'

const TOTAL_STEPS = 5

export function Onboarding() {
  const step = useAppStore((s) => s.onboardingStep)
  const setStep = useAppStore((s) => s.setOnboardingStep)
  const setOnboardingCompleted = useAppStore((s) => s.setOnboardingCompleted)
  const sttTestStatus = useAppStore((s) => s.sttTestStatus)
  const llmTestStatus = useAppStore((s) => s.llmTestStatus)

  const canNext = (() => {
    switch (step) {
      case 0:
        return true // Welcome — always
      case 1:
        return sttTestStatus === 'success' // STT must pass
      case 2:
        return llmTestStatus === 'success' // LLM must pass
      case 3:
        return true // Quick test — optional
      case 4:
        return true // Done
      default:
        return false
    }
  })()

  const titles = [
    {
      title: 'Welcome to OpenTypeless',
      subtitle: 'A few quick steps to get started with voice input',
    },
    {
      title: 'Speech Recognition',
      subtitle: 'Configure your ASR service to convert speech to text',
    },
    {
      title: 'AI Polish',
      subtitle: 'Configure an LLM service to polish transcribed text',
    },
    {
      title: 'How It Works',
      subtitle: 'See the full pipeline in action — from voice to polished text',
    },
    { title: 'Setup Complete', subtitle: undefined },
  ]

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
          {step === 3 && <QuickTestStep />}
          {step === 4 && <DoneStep />}
        </motion.div>
      </AnimatePresence>
    </OnboardingLayout>
  )
}
