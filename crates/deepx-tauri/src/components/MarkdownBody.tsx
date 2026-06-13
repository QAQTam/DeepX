import markdownit from "markdown-it";
import { createMemo } from "solid-js";

const md = markdownit({
  html: true,
  linkify: true,
  typographer: true,
  breaks: true,
});

interface MarkdownBodyProps {
  content: string;
  class?: string;
}

export default function MarkdownBody(props: MarkdownBodyProps) {
  const html = createMemo(() => md.render(props.content));
  return <div class={props.class} innerHTML={html()} />;
}
