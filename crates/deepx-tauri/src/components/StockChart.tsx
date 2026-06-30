import { createEffect, onCleanup } from "solid-js";
import { createChart, ColorType, CandlestickSeries, HistogramSeries } from "lightweight-charts";
import type { CodeDelta } from "../store/chat";
import { invoke } from "@tauri-apps/api/core";

export interface CodeDaily {
  date: string;
  lines_added: number;
  lines_removed: number;
  files_created: number;
  files_deleted: number;
}

interface StockChartProps {
  seed: string;
  codeDeltas: () => CodeDelta[];
}

export default function StockChart(props: StockChartProps) {
  let containerRef!: HTMLDivElement;
  let chart: ReturnType<typeof createChart> | null = null;

  createEffect(() => {
    const container = containerRef;
    if (!container || chart) return;

    chart = createChart(container, {
      width: container.clientWidth,
      height: 260,
      layout: {
        background: { type: ColorType.Solid, color: "transparent" },
        textColor: "#9ca3af",
      },
      grid: {
        vertLines: { color: "#1f2937" },
        horzLines: { color: "#1f2937" },
      },
      rightPriceScale: { borderColor: "#374151" },
      timeScale: { borderColor: "#374151", timeVisible: true },
    });

    // Load history
    invoke<string>("cmd_get_code_stats", { seed: props.seed, days: 30 })
      .then(raw => JSON.parse(raw) as CodeDaily[])
      .then(dailies => {
        if (!chart || dailies.length === 0) return;
        const candleData = dailies.map(d => ({
          time: d.date,
          open: 0,
          high: d.lines_added,
          low: -(d.lines_removed as number),
          close: (d.lines_added as number) - (d.lines_removed as number),
        }));
        const volumeData = dailies.map(d => ({
          time: d.date,
          value: d.files_created + d.files_deleted,
          color: d.lines_added >= d.lines_removed ? "#22c55e40" : "#ef444440",
        }));

        const candleSeries = chart!.addSeries(CandlestickSeries, {
          upColor: "#22c55e",
          downColor: "#ef4444",
          borderUpColor: "#22c55e",
          borderDownColor: "#ef4444",
          wickUpColor: "#22c55e",
          wickDownColor: "#ef4444",
        });
        candleSeries.setData(candleData as any);

        const volSeries = chart!.addSeries(HistogramSeries, {
          priceFormat: { type: "volume" },
          priceScaleId: "volume",
        });
        volSeries.setData(volumeData as any);
      })
      .catch(console.error);

    onCleanup(() => { chart?.remove(); chart = null; });
  });

  return <div ref={containerRef} style="width:100%;height:260px" />;
}
