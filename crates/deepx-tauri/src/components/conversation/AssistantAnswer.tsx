import MarkdownBody from "../MarkdownBody";

export default function AssistantAnswer(props: { markdown: string }) {
  return <div class="assistant-answer" data-part="assistant-answer">
    <MarkdownBody class="md-body assistant-answer-markdown" content={props.markdown} final />
  </div>;
}
