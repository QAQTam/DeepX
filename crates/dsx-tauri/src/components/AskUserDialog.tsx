// ── AskUserDialog ──
// Modal shown when the agent needs user input.

import { onMount } from 'solid-js'
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
  let timerRef: ReturnType<typeof setTimeout> | null = null
  onMount(() => () => { if (timerRef) clearTimeout(timerRef) })

  return (
    <div class="absolute inset-0 bg-black/30 flex items-center justify-center z-50" role="dialog" aria-modal="true" aria-label={tt('chat.toolCalling')}>
      <div class="bg-[var(--bg-primary)] border border-[var(--border)] rounded-2xl p-6 max-w-md w-full mx-4 shadow-md transition-theme">
        <div class="text-sm font-bold text-[var(--text-h)] mb-1">${tt('askUser.title')}</div>
        <div class="text-xs text-[var(--text)] mb-4 whitespace-pre-wrap">{question}</div>

        {options && options.length > 0 ? (
          <div class="flex flex-wrap gap-2 mb-4">
            {options.map((opt) => (
              <Button
                variant="secondary"
                size="sm"
                onClick={() => { setAnswer(opt); timerRef = setTimeout(onSubmit, 100) }}
              >
                {opt}
              </Button>
            ))}
          </div>
        ) : (
          <div class="mb-4">
            <Input
              type="text"
              value={answer}
              onInput={(e) => setAnswer(e.currentTarget.value)}
              onKeyDown={(e) => { if (e.key === 'Enter') onSubmit() }}
              placeholder={tt("askUser.placeholder")}
              autofocus
            />
          </div>
        )}

        <div class="flex gap-2 justify-end">
          <Button variant="secondary" size="sm" onClick={onSubmit}
            disabled={!answer.trim() && (!options || options.length === 0)}
            class="flex-1"
          >
            {tt('common.confirm')}
          </Button>
        </div>
      </div>
    </div>
  )
}
