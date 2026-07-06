import { createSignal, createEffect, For, Show } from "solid-js";
import { open } from "@tauri-apps/plugin-dialog";
import { useI18n } from "../i18n";

const MODES = ["normal", "plan", "code"] as const;

interface InputBarProps {
  onSend: (text: string, files: string[]) => void;
  onStop: () => void;
  isStreaming: () => boolean;
  disabled: boolean;
  restoreText: () => string | null;
  mode: string;
  onModeChange: (mode: string) => void;
}

export default function InputBar(props: InputBarProps) {
  const { t } = useI18n();
  let textareaRef!: HTMLTextAreaElement;
  const [files, setFiles] = createSignal<string[]>([]);

  createEffect(() => {
    const text = props.restoreText();
    if (text) {
      textareaRef.value = text;
      textareaRef.style.height = "auto";
      textareaRef.style.height = Math.min(textareaRef.scrollHeight, 160) + "px";
      textareaRef.focus();
    }
  });

  async function pickFiles() {
    try {
      const selected = await open({ multiple: true, title: "Add files to context" });
      if (selected) {
        const paths = Array.isArray(selected) ? selected : [selected];
        setFiles(prev => [...new Set([...prev, ...paths])]);
      }
    } catch (e) { console.error("file pick:", e); }
  }

  function removeFile(path: string) {
    setFiles(prev => prev.filter(f => f !== path));
  }

  function handleKeyDown(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      submit();
    }
  }

  function submit() {
    const text = textareaRef.value.trim();
    if ((!text && files().length === 0) || props.disabled || props.isStreaming()) return;
    props.onSend(text, files());
    textareaRef.value = "";
    textareaRef.style.height = "auto";
    setFiles([]);
  }

  function autoResize() {
    textareaRef.style.height = "auto";
    textareaRef.style.height = Math.min(textareaRef.scrollHeight, 160) + "px";
  }

  const fileName = (p: string) => p.split(/[/\\]/).pop() || p;

  return (
    <div class="input-bar">
      {/* Mode segment control — iOS style */}
      <div class="mode-segment">
        <For each={MODES}>
          {(m) => (
            <button
              class={`mode-seg-btn ${props.mode === m ? "active" : ""}`}
              onClick={() => props.onModeChange(m)}
            >{t().mode[m]}</button>
          )}
        </For>
      </div>

      {/* File chips */}
      <Show when={files().length > 0}>
        <div class="file-chips">
          <For each={files()}>
            {(path) => (
              <span class="file-chip">
                <span class="file-chip-name">{fileName(path)}</span>
                <button class="file-chip-remove" onClick={() => removeFile(path)}>×</button>
              </span>
            )}
          </For>
        </div>
      </Show>

      {/* Input row */}
      <div class="input-row">
        <button class="attach-btn" onClick={pickFiles} title={t().chatAttach}>
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M21.44 11.05l-9.19 9.19a6 6 0 0 1-8.49-8.49l9.19-9.19a4 4 0 0 1 5.66 5.66l-9.2 9.19a2 2 0 0 1-2.83-2.83l8.49-8.48" />
          </svg>
        </button>
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
    </div>
  );
}
