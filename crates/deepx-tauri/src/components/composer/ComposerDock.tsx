import { createSignal } from "solid-js";
import type { createFollowUpQueue } from "../../store/followUpQueue";
import ComposerQueue from "./ComposerQueue";

type Queue = ReturnType<typeof createFollowUpQueue>;
export default function ComposerDock(props: {
  isStreaming: () => boolean;
  hasPendingGate: () => boolean;
  queue: Queue;
  onSend: (text: string, files: string[]) => Promise<void>;
  onStop: () => Promise<void>;
  mode: string;
  onModeChange: (mode: string) => void;
  model?: string;
}) {
  const [text, setText] = createSignal("");
  const submit = async () => {
    const value = text().trim();
    if (!value || props.hasPendingGate()) return;
    if (props.isStreaming()) props.queue.enqueue(value, []);
    else await props.onSend(value, []);
    setText("");
  };
  return <div class="composer-wrap">
    <ComposerQueue queue={props.queue} />
    <section class="composer-dock" data-composer-dock>
      <textarea value={text()} onInput={event => setText(event.currentTarget.value)} onKeyDown={event => {
        if (event.key === "Enter" && !event.shiftKey) { event.preventDefault(); void submit(); }
      }} placeholder={props.hasPendingGate() ? "请先处理当前授权请求" : "向 DeepX 提问…"} />
      <footer>
        <div><button class="composer-attach" aria-label="添加附件">＋</button><button class="composer-mode" onClick={() => props.onModeChange(props.mode === "plan" ? "code" : "plan")}>{props.mode === "plan" ? "规划" : "执行"}</button></div>
        <div class="composer-meta"><span>{props.model}</span>{props.isStreaming()
          ? <button class="composer-stop" onClick={() => void props.onStop()}>■</button>
          : <button class="composer-send" disabled={!text().trim() || props.hasPendingGate()} onClick={() => void submit()}>↑</button>}</div>
      </footer>
    </section>
  </div>;
}
