import { createSignal, createEffect, createMemo, For, Show } from "solid-js";
import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import { useI18n } from "../i18n";
import type { SkillInfo } from "../store/chat";
import SlashMenu, { type SlashCommand } from "./SlashMenu";

const MODES = ["plan", "code"] as const;

interface InputBarProps {
  onSend: (text: string, files: string[]) => void;
  onStop: () => void;
  isStreaming: () => boolean;
  disabled: boolean;
  restoreText: () => string | null;
  mode: string;
  onModeChange: (mode: string) => void;
  onSlashCommand: (cmd: SlashCommand) => void;
  seed: string;
  skillCatalog: () => SkillInfo[];
  activeSkillNames: () => string[];
}

const SLASH_COMMANDS: SlashCommand[] = [
  { id: "new",      trigger: "new",      title: "New Session",     description: "Start a new conversation",  icon: "🆕" },
  { id: "compact",  trigger: "compact",  title: "Compact Context", description: "Summarize & trim history",   icon: "📦" },
  { id: "undo",     trigger: "undo",     title: "Undo Last Turn",  description: "Remove last exchange",      icon: "↩" },
  { id: "settings", trigger: "settings", title: "Settings",        description: "Open settings page",         icon: "🔧" },
];

export default function InputBar(props: InputBarProps) {
  const { t } = useI18n();
  let textareaRef!: HTMLTextAreaElement;
  const [files, setFiles] = createSignal<string[]>([]);
  const [slashFilter, setSlashFilter] = createSignal("");
  const [slashActive, setSlashActive] = createSignal(0);
  const [slashVisible, setSlashVisible] = createSignal(false);

  const filteredCommands = createMemo(() => {
    const q = slashFilter().toLowerCase();
    // Static commands filtered by trigger/title
    const cmds = q
      ? SLASH_COMMANDS.filter(
          (c) => c.trigger.toLowerCase().startsWith(q) || c.title.toLowerCase().includes(q)
        )
      : SLASH_COMMANDS;
    // Dynamic skills — show all when no filter, or match by name
    const skills: SlashCommand[] = props.skillCatalog()
      .filter((s) => !q || s.name.toLowerCase().includes(q))
      .map((s) => ({
        id: `skill:${s.name}`,
        trigger: s.name,
        title: s.name,
        description: s.description,
        icon: props.activeSkillNames().includes(s.name) ? "\u2713" : "\u25cb",
        group: "Skills",
      }));
    return [...cmds, ...skills];
  });

  function closeSlash() {
    setSlashVisible(false);
    setSlashFilter("");
    setSlashActive(0);
  }

  function openSlash(filter: string) {
    setSlashFilter(filter);
    setSlashActive(0);
    setSlashVisible(true);
  }

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

  function handleSlashSelect(cmd: SlashCommand) {
    closeSlash();
    textareaRef.value = "";
    textareaRef.style.height = "auto";
    if (cmd.id.startsWith("skill:")) {
      // Activate skill via backend command
      invoke("cmd_activate_skill", { seed: props.seed, name: cmd.trigger }).catch(() => {});
    } else {
      props.onSlashCommand(cmd);
    }
  }

  function handleKeyDown(e: KeyboardEvent) {
    if (slashVisible()) {
      // Slash menu keyboard nav
      if (e.key === "ArrowDown") {
        e.preventDefault();
        setSlashActive(i => (i + 1) % Math.max(filteredCommands().length, 1));
        return;
      }
      if (e.key === "ArrowUp") {
        e.preventDefault();
        setSlashActive(i => (i - 1 + filteredCommands().length) % Math.max(filteredCommands().length, 1));
        return;
      }
      if (e.key === "Enter") {
        e.preventDefault();
        const cmds = filteredCommands();
        if (cmds.length > 0) {
          handleSlashSelect(cmds[slashActive()]);
        }
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        closeSlash();
        return;
      }
      if (e.key === "Backspace") {
        // Update filter on backspace
        const val = textareaRef.value;
        const match = val.match(/^\/(\S*)$/);
        if (match) {
          setSlashFilter(match[1] || "");
          setSlashActive(0);
        } else {
          closeSlash();
        }
        return;
      }
      // Any other typing — check if still a slash pattern
      const val = textareaRef.value + (e.key.length === 1 ? e.key : "");
      const match = val.match(/^\/(\S*)$/);
      if (match) {
        setSlashFilter(match[1] || "");
        setSlashActive(0);
      } else {
        closeSlash();
      }
      return;
    }

    // Normal mode: detect "/" trigger
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      submit();
    }
  }

  function handleInput() {
    autoResize();
    const val = textareaRef.value;
    const match = val.match(/^\/(\S*)$/);
    if (match) {
      openSlash(match[1] || "");
    } else {
      closeSlash();
    }
  }

  function submit() {
    const text = textareaRef.value.trim();
    if (slashVisible()) return; // Don't send while slash menu is open
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
    <div class="input-bar" style={{ position: "relative" }}>
      {/* Slash menu popover */}
      <SlashMenu
        commands={filteredCommands()}
        filter={slashFilter()}
        activeIndex={slashActive()}
        onSelect={handleSlashSelect}
        onHover={setSlashActive}
        visible={slashVisible()}
      />

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
          onInput={handleInput}
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
