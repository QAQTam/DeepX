export type ProgressEvent = {
  stream: "stdout" | "stderr";
  seq: number;
  chunk: string;
};

export type OrderedProgress = {
  chunks: Map<number, ProgressEvent>;
  nextExpectedSeq: number;
};

export const emptyProgress = (): OrderedProgress => ({
  chunks: new Map(),
  nextExpectedSeq: 0,
});

export function appendProgress(
  buffer: OrderedProgress,
  event: ProgressEvent,
): OrderedProgress {
  const chunks = new Map(buffer.chunks);
  chunks.set(event.seq, event);
  return { chunks, nextExpectedSeq: buffer.nextExpectedSeq };
}

export function materializeProgress(buffer: OrderedProgress): ProgressEvent[] {
  return [...buffer.chunks.values()].sort((a, b) => a.seq - b.seq);
}
