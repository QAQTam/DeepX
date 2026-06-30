import * as smd from "streaming-markdown";
import { onMount, onCleanup, createEffect } from "solid-js";

interface MarkdownBodyProps {
  content: string;
  class?: string;
  /** Whether the content is finalized (streaming complete). When true, parser_end is called to flush. */
  final?: boolean;
}

export default function MarkdownBody(props: MarkdownBodyProps) {
  let container!: HTMLDivElement;
  let parser: smd.Parser | null = null;
  let lastLen = 0;
  let finalized = false; // guard against double parser_end

  onMount(() => {
    const renderer = smd.default_renderer(container);
    parser = smd.parser(renderer);
    // Feed any content that already exists at mount time
    if (props.content) {
      smd.parser_write(parser, props.content);
      lastLen = props.content.length;
    }
    // If already finalized, flush pending
    if (props.final && !finalized) {
      finalized = true;
      smd.parser_end(parser);
    }
  });

  // Feed only the new portion of content (incremental)
  createEffect(() => {
    if (!parser) return;
    const delta = props.content.slice(lastLen);
    if (delta.length > 0) {
      smd.parser_write(parser, delta);
      lastLen = props.content.length;
    }
    // Flush remaining pending when content is finalized (once)
    if (props.final && !finalized) {
      finalized = true;
      smd.parser_end(parser);
    }
  });

  onCleanup(() => {
    if (parser) {
      if (!finalized) {
        smd.parser_end(parser);
      }
      parser = null;
    }
  });

  return <div ref={container} class={props.class} />;
}
