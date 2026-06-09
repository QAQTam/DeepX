import { useI18n } from "../i18n";

interface InputBarProps {
  onSend: (text: string) => void;
  onStop: () => void;
  isStreaming: () => boolean;
  disabled: boolean;
}

export default function InputBar(props: InputBarProps) {
  const { t } = useI18n();
  let textareaRef!: HTMLTextAreaElement;

  function handleKeyDown(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      submit();
    }
  }

  function submit() {
    const text = textareaRef.value.trim();
    if (!text || props.disabled || props.isStreaming()) return;
    props.onSend(text);
    textareaRef.value = "";
    textareaRef.style.height = "auto";
  }

  function autoResize() {
    textareaRef.style.height = "auto";
    textareaRef.style.height = Math.min(textareaRef.scrollHeight, 160) + "px";
  }

  return (
    <div class="input-bar">
      <textarea
        ref={textareaRef}
        rows={1}
        placeholder={t().chat.placeholder}
        disabled={props.disabled}
        onKeyDown={handleKeyDown}
        onInput={autoResize}
      />
      {props.isStreaming() ? (
        <button class="stop" onClick={props.onStop} title={t().chat.stop}>
          <svg width="16" height="16" viewBox="0 0 16 16"><rect x="3" y="3" width="10" height="10" rx="1" fill="currentColor"/></svg>
        </button>
      ) : (
        <button class="send" onClick={submit} disabled={props.disabled} title={t().chat.send}>
          <svg width="16" height="16" viewBox="0 0 16 16"><path d="M2 2l12 6-12 6 3-6-3-6z" fill="currentColor"/></svg>
        </button>
      )}
    </div>
  );
}
