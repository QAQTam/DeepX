import * as Dialog from '@radix-ui/react-dialog'
import { motion } from 'framer-motion'

interface AskUserDialogProps {
  question: string
  options?: string[]
  answer: string
  setAnswer: (v: string) => void
  onSubmit: () => void
}

export function AskUserDialog({ question, options, answer, setAnswer, onSubmit }: AskUserDialogProps) {
  return (
    <Dialog.Root open onOpenChange={(open) => { if (!open) { setAnswer(''); onSubmit() } }}>
      <Dialog.Portal>
        <motion.div initial={{ opacity: 0 }} animate={{ opacity: 1 }} transition={{ duration: 0.15 }}>
          <Dialog.Overlay className="fixed inset-0 bg-black/30 z-50" />
        </motion.div>
        <Dialog.Content className="fixed inset-0 z-50 flex items-center justify-center">
          <motion.div initial={{ opacity: 0, scale: 0.95, y: 10 }} animate={{ opacity: 1, scale: 1, y: 0 }}
            transition={{ duration: 0.2 }}
            className="bg-[var(--bg-primary)] border border-[var(--border)] rounded-2xl p-6 max-w-md w-full mx-4 shadow-md">
            <Dialog.Title className="text-sm font-bold text-[var(--text-h)] mb-1">需要确认</Dialog.Title>
            <div className="text-xs text-[var(--text)] mb-4 whitespace-pre-wrap">{question}</div>
            {options && options.length > 0 ? (
              <div className="flex flex-wrap gap-2 mb-4">
                {options.map((opt, i) => (
                  <button key={i} onClick={() => { setAnswer(opt); setTimeout(onSubmit, 100) }}
                    className="px-3 py-1.5 rounded-lg text-xs border border-[var(--border)] bg-[var(--bg-tertiary)] text-[var(--text-h)] hover:bg-[var(--accent-light)] hover:border-[var(--accent)] transition-all">
                    {opt}
                  </button>
                ))}
              </div>
            ) : (
              <div className="flex gap-2 mb-4">
                <input type="text" value={answer} onChange={e => setAnswer(e.target.value)}
                  onKeyDown={e => { if (e.key === 'Enter') onSubmit() }}
                  placeholder="输入回答..."
                  className="flex-1 bg-[var(--bg-tertiary)] border border-[var(--border)] rounded-lg px-3 py-1.5 text-xs text-[var(--text-h)] outline-none focus:border-[var(--accent)]" />
              </div>
            )}
            <div className="flex gap-2">
              <button onClick={() => { setAnswer(''); onSubmit() }}
                className="flex-1 bg-[var(--bg-tertiary)] border border-[var(--border)] text-[var(--text-h)] rounded-lg py-1.5 text-xs hover:brightness-95">
                跳过
              </button>
              <button onClick={onSubmit}
                disabled={!answer.trim() && (!options || options.length === 0)}
                className="flex-1 bg-[var(--accent)] text-white rounded-lg py-1.5 text-xs font-medium hover:brightness-110 disabled:opacity-40">
                确认
              </button>
            </div>
          </motion.div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  )
}
