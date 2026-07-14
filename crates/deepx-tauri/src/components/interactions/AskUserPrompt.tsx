import { createSignal, For, Show } from "solid-js";
import type { AskAnswer, AskMode, AskQuestion } from "../../lib/types";

export default function AskUserPrompt(props: {
  mode: AskMode;
  questions: AskQuestion[];
  onSubmit: (answers: AskAnswer[]) => void | Promise<void>;
  onDismiss: () => void | Promise<void>;
}) {
  const [answers, setAnswers] = createSignal<Record<string, string>>({});
  const setAnswer = (id: string, answer: string) =>
    setAnswers(current => ({ ...current, [id]: answer }));

  return (
    <section class="interaction-prompt ask-user-prompt">
      <div class="interaction-eyebrow">需要你的选择</div>
      <For each={props.questions}>{question => (
        <fieldset>
          <legend>{question.question}</legend>
          <div class="ask-options">
            <For each={question.options}>{option => (
              <label>
                <input
                  type="radio"
                  name={question.id}
                  value={option}
                  checked={answers()[question.id] === option}
                  onChange={() => setAnswer(question.id, option)}
                />
                {option}
              </label>
            )}</For>
          </div>
          <Show when={question.allow_custom}>
            <input
              class="ask-custom"
              aria-label={`${question.question} 自定义回答`}
              placeholder="其它…"
              onInput={event => setAnswer(question.id, event.currentTarget.value)}
            />
          </Show>
        </fieldset>
      )}</For>
      <div class="interaction-actions">
        <button type="button" class="interaction-reject" onClick={props.onDismiss}>跳过</button>
        <button
          type="button"
          class="interaction-approve approval-low"
          onClick={() => props.onSubmit(props.questions.map(question => ({
            question_id: question.id,
            answer: answers()[question.id] ?? "",
          })))}
        >提交</button>
      </div>
    </section>
  );
}
