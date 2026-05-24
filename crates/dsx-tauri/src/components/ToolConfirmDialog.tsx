import * as Dialog from '@radix-ui/react-dialog'
import { motion } from 'framer-motion'

interface ToolConfirmDialogProps {
  toolName: string
  action: string
  prompt: string
  onConfirm: () => void
  onDeny: () => void
}

export function ToolConfirmDialog({ toolName, action, prompt, onConfirm, onDeny }: ToolConfirmDialogProps) {
  const isDanger = action === 'sudo_run' || prompt.includes('destructive')
  return (
    <Dialog.Root open onOpenChange={() => onDeny()}>
      <Dialog.Portal>
        <motion.div initial={{ opacity: 0 }} animate={{ opacity: 1 }} transition={{ duration: 0.15 }}>
          <Dialog.Overlay className="fixed inset-0 bg-black/30 z-50" />
        </motion.div>
        <Dialog.Content className="fixed inset-0 z-50 flex items-center justify-center">
          <motion.div initial={{ opacity: 0, scale: 0.95, y: 10 }} animate={{ opacity: 1, scale: 1, y: 0 }}
            transition={{ duration: 0.2 }}
            className="bg-[var(--bg-primary)] border border-[var(--border)] rounded-2xl p-6 max-w-md w-full mx-4 shadow-md">
            <Dialog.Title className={`text-sm font-bold mb-1 ${isDanger ? 'text-[var(--error)]' : 'text-[var(--text-h)]'}`}>
              {isDanger ? '⚠ 危险操作确认' : '工具确认'}
            </Dialog.Title>
            <div className="mb-1">
              <span className="text-xs text-[var(--muted)]">工具: </span>
              <span className="text-xs font-mono text-[var(--text-h)]">{toolName}</span>
            </div>
            <div className="mb-4">
              <span className="text-xs text-[var(--muted)]">操作: </span>
              <span className="text-xs font-mono text-[var(--text)]">{action}</span>
            </div>
            <div className="text-xs text-[var(--text)] bg-[var(--bg-tertiary)] rounded-lg p-3 mb-4 font-mono whitespace-pre-wrap border border-[var(--border)]">
              {prompt}
            </div>
            <div className="flex gap-2">
              <Dialog.Close asChild>
                <button
                  className="flex-1 bg-[var(--bg-tertiary)] border border-[var(--border)] text-[var(--text-h)] rounded-lg py-1.5 text-xs hover:brightness-95">
                  拒绝
                </button>
              </Dialog.Close>
              <button onClick={onConfirm}
                className={`flex-1 rounded-lg py-1.5 text-xs font-medium text-white ${isDanger ? 'bg-[var(--error)]' : 'bg-[var(--accent)]'}`}>
                允许
              </button>
            </div>
          </motion.div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  )
}
