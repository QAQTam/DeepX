export default function ThreadHeader(props: {
  title: string;
  environmentOpen: boolean;
  onToggleEnvironment: () => void;
  onOpenLocation: () => void;
  onCompact: () => void;
}) {
  return <header class="thread-header">
    <div class="thread-title"><span>▱</span><strong>{props.title}</strong></div>
    <div class="thread-actions">
      <button onClick={props.onOpenLocation}>打开位置</button>
      <button class={props.environmentOpen ? "active" : ""} onClick={props.onToggleEnvironment}>环境</button>
      <button aria-label="整理上下文" onClick={props.onCompact}>•••</button>
    </div>
  </header>;
}
