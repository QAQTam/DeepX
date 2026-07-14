import { createEffect, createSignal, on, Show } from "solid-js";
import type { AskState } from "../store/chat";
import type { AskAnswer, AskQuestion } from "../lib/types";

interface AskFormProps {
  state: () => AskState;
  onSubmit: (answers: AskAnswer[]) => void;
  onDismiss: () => void;
}

export default function AskForm(props: AskFormProps) {
  const [answers, setAnswers] = createSignal<Record<string, string>>({});
  const [customInputs, setCustomInputs] = createSignal<Record<string, string>>({});
  const [page, setPage] = createSignal(0);
  const [slideDir, setSlideDir] = createSignal<"forward" | "back">("forward");
  createEffect(on(
    () => props.state().askId,
    () => {
      setAnswers({});
      setCustomInputs({});
      setPage(0);
    },
    { defer: true },
  ));

  const s = () => props.state();
  const total = () => s().questions.length;
  const currentQ = () => s().questions[page()] as AskQuestion | undefined;
  const isLast = () => page() === total() - 1;
  const isFirst = () => page() === 0;

  // ── Answer tracking ──

  function handleOptionClick(q: AskQuestion, opt: string) {
    setAnswers(prev => ({ ...prev, [q.id]: opt }));
    setCustomInputs(prev => ({ ...prev, [q.id]: "" }));
  }

  function handleCustomChange(qid: string, value: string) {
    setCustomInputs(prev => ({ ...prev, [qid]: value }));
    if (value.trim()) {
      setAnswers(prev => {
        const next = { ...prev };
        delete next[qid];
        return next;
      });
    }
  }

  // ── Navigation ──

  function goPrev() { if (!isFirst()) { setSlideDir("back"); setPage(p => p - 1); } }
  function goNext() { if (!isLast()) { setSlideDir("forward"); setPage(p => p + 1); } }

  // ── Can submit? ──

  const allAnswered = () =>
    s().questions.every(q =>
      (customInputs()[q.id] || "").trim() || answers()[q.id]
    );

  // ── Submit ──

  function handleSubmitAll() {
    if (!allAnswered()) return;
    const currentAnswers = answers();
    const result: AskAnswer[] = s().questions.map(q => ({
      question_id: q.id,
      answer: (customInputs()[q.id] || "").trim() || currentAnswers[q.id] || "",
    }));
    props.onSubmit(result);
  }

  function handleDismiss() {
    props.onDismiss();
  }

  // ── Render ──

  return (
    <Show when={s().show && s().mode === "batch" && currentQ()}>
      <div class="ask-overlay" onClick={handleDismiss}>
        <div class="ask-dialog ask-dialog-batch" onClick={(e) => e.stopPropagation()}>

          {/* Header */}
          <div class="ask-header">
            <span class="ask-icon">?</span>
            <span class="ask-title">DeepX asks</span>
            <button class="ask-close" onClick={handleDismiss} title="Dismiss">
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <path d="M18 6L6 18M6 6l12 12" />
              </svg>
            </button>
          </div>

          {/* Orange progress bar — signature element */}
          <div class="ask-progress-bar">
            <div
              class="ask-progress-fill"
              style={{ width: `${((page() + 1) / total()) * 100}%` }}
            />
          </div>

          {/* Step dots with labels */}
          <div class="ask-progress-steps">
            {Array.from({ length: total() }, (_, i) => (
              <button
                class={`ask-step-dot ${i === page() ? "ask-step-active" : ""} ${answers()[s().questions[i]?.id ?? ""] || (customInputs()[s().questions[i]?.id ?? ""] || "").trim() ? "ask-step-filled" : ""}`}
                onClick={() => setPage(i)}
              >
                Q{i + 1}
              </button>
            ))}
          </div>

          {/* Question body */}
          <div class="ask-form-body">
            <div class={`ask-form-page ask-slide-${slideDir()}`}>
              <div class="ask-form-section">
                <div class="ask-form-label">
                  <span class="ask-question">{currentQ()!.question}</span>
                </div>

                {/* Options (if present) */}
                <Show when={currentQ()!.options && currentQ()!.options!.length > 0}>
                  <div class="ask-options">
                    {currentQ()!.options!.map((opt: string) => (
                      <button
                        class={`ask-option-btn ${answers()[currentQ()!.id] === opt ? "ask-option-selected" : ""}`}
                        onClick={() => handleOptionClick(currentQ()!, opt)}
                      >
                        <span class="ask-option-radio" />
                        <span>{opt}</span>
                      </button>
                    ))}
                  </div>
                </Show>

                {/* Custom input — always visible, users can type their own answer */}
                <div class="ask-custom">
                  <input
                    type="text"
                    class="ask-input"
                    placeholder={currentQ()!.options?.length
                      ? "Or type your own answer..."
                      : "Type your answer..."}
                    value={customInputs()[currentQ()!.id] || ""}
                    onInput={(e) => handleCustomChange(currentQ()!.id, e.currentTarget.value)}
                  />
                </div>
              </div>
            </div>
          </div>

          {/* Footer: navigation + submit (always visible) */}
          <div class="ask-form-footer">
            <div class="ask-nav">
              <button class="ask-nav-btn" onClick={goPrev} disabled={isFirst()}>
                <svg width="12" height="12" viewBox="0 0 16 16"><path d="M10 2L4 8l6 6" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"/></svg>
                Prev
              </button>
              <button class="ask-nav-btn" onClick={goNext} disabled={isLast()}>
                Next
                <svg width="12" height="12" viewBox="0 0 16 16"><path d="M6 2l6 6-6 6" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"/></svg>
              </button>
            </div>
            <button
              class="ask-submit-btn"
              onClick={handleSubmitAll}
              disabled={!allAnswered()}
            >
              Submit
            </button>
          </div>

        </div>
      </div>
    </Show>
  );
}
