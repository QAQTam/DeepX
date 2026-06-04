// ── AskUserDialog ──
// Modal shown when the agent needs user input.

import { useRef, useEffect, type ChangeEvent, type KeyboardEvent } from 'react'
import { tt } from '../i18n'
import { Button, Input } from './shared'

interface AskUserDialogProps {
  question: string
  options?: string[]
  answer: string
  setAnswer: (v: string) => void
  onSubmit: () => void
}

export function AskUserDialog({ question, options, answer, setAnswer, onSubmit }: AskUserDialogProps) {
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  useEffect(() => () => clearTimeout(timerRef.current ?? undefined), [])

  return (
    <div className="absolute inset-0 bg-black/30 flex items-center justify-center z-50" role="dialog" aria-modal="true" aria-label={tt('chat.toolCalling')}>
      <div className="bg-[var(--bg-primary)] border border-[var(--border)] rounded-2xl p-6 max-w-md w-full mx-4 shadow-md transition-theme">
        <div className="text-sm font-bold text-[var(--text-h)] mb-1">需要确认</div>
        <div className="text-xs text-[var(--text)] mb-4 whitespace-pre-wrap">{question}</div>

        {options && options.length > 0 ? (
          <div className="flex flex-wrap gap-2 mb-4">
            {options.map((opt, i) => (
              <Button
                key={i}
                variant="secondary"
                size="sm"
                onClick={() => { setAnswer(opt); timerRef.current = setTimeout(onSubmit, 100) }}
              >
                {opt}
              </Button>
            ))}
          </div>
        ) : (
          <div className="mb-4">
            <Input
              type="text"
              value={answer}
              onChange={(e: ChangeEvent<HTMLInputElement>) => setAnswer(e.target.value)}
              onKeyDown={(e: KeyboardEvent<HTMLInputElement>) => { if (e.key === 'Enter') onSubmit() }}
              placeholder="输入回答..."
              autoFocus
            />
          </div>
        )}

        <div className="flex gap-2">
          <Button variant="secondary" onClick={() => { setAnswer(''); onSubmit() }} className="flex-1">
            {tt('common.skip')}
          </Button>
          <Button
            variant="primary"
            onClick={onSubmit}
            disabled={!answer.trim() && (!options || options.length === 0)}
            className="flex-1"
          >
            {tt('common.confirm')}
          </Button>
        </div>
      </div>
    </div>
  )
}
