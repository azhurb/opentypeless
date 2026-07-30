import { motion } from 'framer-motion'

interface Props {
  checked: boolean
  onChange: (checked: boolean) => void
  label?: string
  /** Renders the switch inert. Use when the setting would be a silent no-op —
   *  a toggle that can be turned on and then quietly ignored is worse than one
   *  that explains why it can't be. Pair it with a hint saying what to fix. */
  disabled?: boolean
}

export function Toggle({ checked, onChange, label, disabled = false }: Props) {
  return (
    <label
      className={`flex items-center gap-2.5 ${disabled ? 'cursor-not-allowed' : 'cursor-pointer'}`}
    >
      <button
        role="switch"
        aria-checked={checked}
        aria-disabled={disabled}
        disabled={disabled}
        onClick={() => onChange(!checked)}
        className={`relative w-[44px] h-[26px] rounded-full border-none transition-colors duration-200 disabled:opacity-40 disabled:cursor-not-allowed ${
          disabled ? '' : 'cursor-pointer'
        } ${checked ? 'bg-text-secondary' : 'bg-bg-tertiary'}`}
      >
        <motion.div
          className="absolute top-[2px] w-[22px] h-[22px] rounded-full bg-white shadow-sm"
          animate={{ left: checked ? 20 : 2 }}
          transition={{ type: 'spring', stiffness: 500, damping: 30 }}
        />
      </button>
      {label && (
        <span className={`text-[13px] ${disabled ? 'text-text-tertiary' : 'text-text-primary'}`}>
          {label}
        </span>
      )}
    </label>
  )
}
