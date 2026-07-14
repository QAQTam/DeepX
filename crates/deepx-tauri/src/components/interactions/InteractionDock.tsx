import type { JSX } from "solid-js";

interface InteractionDockProps {
  children: JSX.Element;
}

export default function InteractionDock(props: InteractionDockProps) {
  return (
    <div class="interaction-dock">
      {props.children}
    </div>
  );
}
