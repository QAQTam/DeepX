import { Show } from "solid-js";

interface StatusBarProps {
  model: string;
  sessionSeed: string;
  contextTokens: number;
  contextLimit: number;
  sessionTokens: number;
  isStreaming: boolean;
  error: string | null;
}

export default function StatusBar(props: StatusBarProps) {
  return (
    <footer class="status-bar">
      <div class="status-item">
        <span class={`status-dot ${props.isStreaming ? "active" : props.error ? "error" : "idle"}`} />
        <span>{props.model}</span>
      </div>
      <Show when={props.sessionSeed}>
        <div class="status-item">session: {props.sessionSeed}</div>
      </Show>
      <div class="status-item">ctx: {props.contextTokens}/{props.contextLimit}</div>
      <div class="status-item">total: {props.sessionTokens.toLocaleString()}</div>
    </footer>
  );
}
