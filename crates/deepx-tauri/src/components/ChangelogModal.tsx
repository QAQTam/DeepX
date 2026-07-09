import { For, Show, createSignal } from "solid-js";

interface ChangelogEntry {
  tag: "feature" | "fix" | "perf";
  title: string;
}

const CHANGELOG: ChangelogEntry[] = [
  // ── Features ──
  { tag: "feature", title: "Permission level system (1-4): Lockdown → Unrestricted, with workspace boundary checks and trusted folder persistence" },
  { tag: "feature", title: "Permission dialog modal with approve/deny + trust-folder checkbox" },
  { tag: "feature", title: "sed editing tool: structured sed expression support (s/old/new/g, line addressing, chained expressions) with live diff" },
  { tag: "feature", title: "file_edit_diff: line-number addressing (start_line/end_line) bypasses content matching" },
  { tag: "feature", title: "Slash commands floating menu: /new, /compact, /undo, /settings" },
  { tag: "feature", title: "Settings page: full-page redesign with two-column responsive layout" },
  { tag: "feature", title: "Compact template: added File Inventory, Decision Log, Key Symbols sections" },
  { tag: "feature", title: "Context panel: live token breakdown (no longer waits for compact)" },
  { tag: "feature", title: "Agent mode (PLAN/CODE) persists across session restarts" },
  { tag: "feature", title: "Plan review: approve/reject with batch actions, trust-folder checkbox, submitted pulse animation" },
  // ── Fixes ──
  { tag: "fix", title: "handler! macro always returned success:true — errors now correctly flagged as failures" },
  { tag: "fix", title: "file_edit mismatch now shows closest line number + suggests file_edit_diff start_line" },
  { tag: "fix", title: "PlanReviewPanel: approve/reject action mismatch fixed (frontend/BACKEND verb agreement)" },
  { tag: "fix", title: "file_edit/format_diff_result: replacen producing malformed checkboxes fixed" },
  { tag: "fix", title: "PLAN_BLOCKED tool list unified across bridge.rs and permission.rs" },
  { tag: "fix", title: "extract_files_affected: fixed hardcoded tool names (read_file→file_read etc.)" },
  { tag: "fix", title: "Removed duplicate catch_unwind from bridge.rs (already caught in manager.rs)" },
  { tag: "fix", title: "Removed dead SafetyVerdict::RequireAuth code" },
  // ── Perf ──
  { tag: "perf", title: "Successful file_edit / write_file no longer return full diff body — saves ~80-90% context tokens per edit" },
];

const TAG_ICONS: Record<string, string> = {
  feature: "✨",
  fix: "🐛",
  perf: "⚡",
};

const TAG_LABELS: Record<string, string> = {
  feature: "Feature",
  fix: "Bug Fix",
  perf: "Performance",
};

export default function ChangelogModal(props: { onClose: () => void }) {
  return (
    <div class="changelog-overlay" onClick={props.onClose}>
      <div class="changelog-dialog" onClick={(e) => e.stopPropagation()}>
        <div class="changelog-header">
          <span class="changelog-title">What's New in v0.7.0</span>
          <button class="changelog-close" onClick={props.onClose}>✕</button>
        </div>
        <div class="changelog-body">
          <For each={CHANGELOG}>
            {(entry) => (
              <div class={`changelog-entry changelog-${entry.tag}`}>
                <span class="changelog-tag">{TAG_ICONS[entry.tag]} {TAG_LABELS[entry.tag]}</span>
                <span class="changelog-text">{entry.title}</span>
              </div>
            )}
          </For>
        </div>
      </div>
    </div>
  );
}
