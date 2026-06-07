// ── AskUserDialog ──
// Modal shown when the agent needs user input.
// Shows options as quick-select buttons + a free-text input for custom answers.

import { onMount } from 'solid-js'
import { tt } from '../i18n'
import { Button, Input } from './shared'

interface AskUserDialogProps {
  question: string
  options?: string[]
  answer: string
  setAnswer: (v: string) => void
  onSubmit: () => void
  onDismiss: () => void
}

export function AskUserDialog(props: AskUserDialogProps) {
  let inputRef!: HTMLInputElement

  const submit = () => {
    if (props.answer.trim()) props.onSubmit()
  }

  onMount(() => inputRef?.focus())

  return (
    <div class="absolute inset-0 bg-black/30 flex items-center justify-center z-50"
      role="dialog" aria-modal="true"
      onKeyDown={(e) => { if (e.key === 'Escape') props.onDismiss() }}
      onClick={(e) => { if (e.target === e.currentTarget) props.onDismiss() }}
    >
      <div class="bg-[var(--bg-primary)] border border-[var(--border)] rounded-2xl p-6 max-w-md w-full mx-4 shadow-md transition-theme">
        <div class="text-sm font-bold text-[var(--text-h)] mb-1">{tt('askUser.title')}</div>
        <div class="text-xs text-[var(--text)] mb-4 whitespace-pre-wrap">{props.question}</div>

        <div class="mb-4">
          <Input
            ref={inputRef}
            type="text"
            value={props.answer}
            onInput={(e) => props.setAnswer(e.currentTarget.value)}
            onKeyDown={(e) => { if (e.key === 'Enter') submit() }}
            placeholder={tt("askUser.placeholder")}
          />
        </div>

        {props.options && props.options.length > 0 && (
          <div class="flex flex-wrap gap-2 mb-4">
            {props.options.map((opt) => (
              <Button
                variant="secondary"
                size="sm"
                onClick={() => {
                  const prev = props.answer
                  props.setAnswer(prev ? `${prev}; ${opt}` : opt)
                }}
              >
                {opt}
              </Button>
            ))}
          </div>
        )}

        <div class="flex gap-2 justify-end">
          <Button variant="secondary" size="sm" onClick={props.onDismiss} class="flex-1">
              {tt('common.skip')}
          </Button>
          <Button variant="primary" size="sm" onClick={submit}
            disabled={!props.answer.trim()}
            class="flex-1"
          >
            {tt('common.confirm')}
          </Button>
        </div>
      </div>
    </div>
  )
}