import { For, Show } from "solid-js";
import { useI18n } from "../i18n";

export interface SlashCommand {
  id: string;
  trigger: string;      // e.g. "new", "compact"
  title: string;        // display name
  description?: string; // one-liner hint
  icon?: string;        // single emoji or text icon
  group?: string;       // group label (optional)
}

export default function SlashMenu(props: {
  commands: SlashCommand[];
  filter: string;
  activeIndex: number;
  onSelect: (cmd: SlashCommand) => void;
  onHover: (index: number) => void;
  visible: boolean;
}) {
  const { t } = useI18n();
  const filtered = () => {
    const q = props.filter.toLowerCase();
    if (!q) return props.commands;
    return props.commands.filter(
      (c) =>
        c.trigger.toLowerCase().startsWith(q) ||
        c.title.toLowerCase().includes(q)
    );
  };

  const grouped = () => {
    const groups: { label: string; items: SlashCommand[] }[] = [];
    const seen = new Map<string, number>();
    for (const cmd of filtered()) {
      const key = cmd.group ?? "";
      if (seen.has(key)) {
        groups[seen.get(key)!]!.items.push(cmd);
      } else {
        seen.set(key, groups.length);
        groups.push({ label: key, items: [cmd] });
      }
    }
    return groups;
  };

  return (
    <div
      class="slash-menu"
      classList={{ visible: props.visible }}
      role="listbox"
    >
      <For each={grouped()}>
        {(group) => (
          <div class="slash-group">
            <Show when={group.label}>
              <div class="slash-group-label">{group.label}</div>
            </Show>
            <For each={group.items}>
              {(cmd, i) => {
                const globalIdx = () => {
                  let idx = 0;
                  for (const g of grouped()) {
                    for (const _ of g.items) {
                      if (g.label === group.label && g.items.indexOf(cmd) === i()) return idx;
                      idx++;
                    }
                  }
                  return idx;
                };
                return (
                  <div
                    class="slash-item"
                    classList={{ active: globalIdx() === props.activeIndex }}
                    onClick={() => props.onSelect(cmd)}
                    onMouseEnter={() => props.onHover(globalIdx())}
                    role="option"
                    aria-selected={globalIdx() === props.activeIndex}
                  >
                    <span class="slash-item-icon">{cmd.icon ?? "/"}</span>
                    <span class="slash-item-trigger">/{cmd.trigger}</span>
                    <Show when={cmd.description}>
                      <span class="slash-item-desc">{cmd.description}</span>
                    </Show>
                  </div>
                );
              }}
            </For>
          </div>
        )}
      </For>
      <Show when={filtered().length === 0}>
        <div class="slash-empty">{t().slash.noMatch}</div>
      </Show>
    </div>
  );
}
