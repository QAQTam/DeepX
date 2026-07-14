import { createEffect, createSignal, on, Show } from "solid-js";
import type { AskState } from "../store/chat";
import type { AskAnswer } from "../lib/types";

interface AskDialogProps {
  state: () => AskState;
  onSubmit: (answers: AskAnswer[]) => void;
  onDismiss: () => void;
}

export default function AskDialog(props: AskDialogProps) {
  const [customInput, setCustomInput] = createSignal("");
  createEffect(on(
    () => props.state().askId,
    () => setCustomInput(""),
    { defer: true },
  ));
  let inputRef!: HTMLInputElement;

  function handleOptionClick(opt: string) {
    const q = props.state().questions[0];
    if (!q) return;
    props.onSubmit([{ question_id: q.id, answer: opt }]);
  }

  function handleCustomSubmit(e: Event) {
    e.preventDefault();
    const text = customInput().trim();
    const q = props.state().questions[0];
    if (text && q) {
      props.onSubmit([{ question_id: q.id, answer: text }]);
      setCustomInput("");
    }
  }

  function handleDismiss() {
    setCustomInput("");
    props.onDismiss();
  }

  const s = () => props.state();
  const q = () => s().questions[0];
  const hasOptions = () => (q()?.options?.length ?? 0) > 0;
  const showCustomInput = () => q() && (!hasOptions() || q().allow_custom !== false);

  return (
    <Show when={s().show && s().mode === "single" && q()}>
      <div class="ask-overlay" onClick={handleDismiss}>
        <div class="ask-dialog" onClick={(e) => e.stopPropagation()}>
          <div class="ask-header">
            <span class="ask-icon">?</span>
            <span class="ask-title">DeepX asks</span>
            <button class="ask-close" onClick={handleDismiss} title="Dismiss">
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <path d="M18 6L6 18M6 6l12 12" />
              </svg>
            </button>
          </div>
          <div class="ask-question">{q().question}</div>
          <Show when={hasOptions()}>
            <div class="ask-options">
              {(q().options || []).map((opt: string) => (
                <button class="ask-option-btn" onClick={() => handleOptionClick(opt)}>
                  {opt}
                </button>
              ))}
            </div>
          </Show>
          <Show when={showCustomInput()}>
            <form class="ask-custom" onSubmit={handleCustomSubmit}>
              <input
                ref={inputRef}
                type="text"
                class="ask-input"
                placeholder="Type your answer..."
                value={customInput()}
                onInput={(e) => setCustomInput(e.currentTarget.value)}
                autofocus
              />
              <button type="submit" class="ask-send-btn" disabled={!customInput().trim()}>
                <svg width="16" height="16" viewBox="0 0 16 16">
                  <path d="M2 2l12 6-12 6 3-6-3-6z" fill="currentColor" />
                </svg>
              </button>
            </form>
          </Show>
        </div>
      </div>
    </Show>
  );
}
