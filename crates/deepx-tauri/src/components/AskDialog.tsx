import { createSignal, Show } from "solid-js";
import type { AskState } from "../store/chat";

interface AskDialogProps {
  state: () => AskState;
  onSubmit: (answer: string) => void;
  onDismiss: () => void;
}

export default function AskDialog(props: AskDialogProps) {
  const [customInput, setCustomInput] = createSignal("");
  let inputRef!: HTMLInputElement;

  function handleOptionClick(opt: string) {
    props.onSubmit(opt);
  }

  function handleCustomSubmit(e: Event) {
    e.preventDefault();
    const text = customInput().trim();
    if (text) {
      props.onSubmit(text);
      setCustomInput("");
    }
  }

  function handleDismiss() {
    setCustomInput("");
    props.onDismiss();
  }

  const s = () => props.state();
  const hasOptions = () => s().options.length > 0;
  const showCustomInput = () => s().options.length === 0 || s().allow_custom;

  return (
    <Show when={s().show}>
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
          <div class="ask-question">{s().question}</div>
          <Show when={hasOptions()}>
            <div class="ask-options">
              {s().options.map((opt) => (
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
