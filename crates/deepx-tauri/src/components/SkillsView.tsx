import { createSignal, For, Show, createMemo } from "solid-js";
import { useI18n } from "../i18n";
import type { SkillInfo } from "../lib/types";

interface SkillsViewProps {
  seed: string;
  available: SkillInfo[];
  active: string[];
  onActivate: (name: string) => Promise<void>;
  onUnload: (name: string) => Promise<void>;
  onReload: () => Promise<void>;
}

export default function SkillsView(props: SkillsViewProps) {
  const { t } = useI18n();
  const [search, setSearch] = createSignal("");
  const [expanded, setExpanded] = createSignal<Set<string>>(new Set());
  const [pending, setPending] = createSignal<Set<string>>(new Set());
  const [errors, setErrors] = createSignal<Record<string, string>>({});
  const [refreshing, setRefreshing] = createSignal(false);

  const hasSeed = () => props.seed.length > 0;

  const filtered = createMemo(() => {
    const q = search().toLowerCase().trim();
    if (!q) return props.available;
    return props.available.filter(
      (s) =>
        s.name.toLowerCase().includes(q) ||
        s.description.toLowerCase().includes(q),
    );
  });

  const grouped = createMemo(() => {
    const activeSkills: SkillInfo[] = [];
    const projectSkills: SkillInfo[] = [];
    const userSkills: SkillInfo[] = [];

    for (const s of filtered()) {
      if (props.active.includes(s.name)) {
        activeSkills.push(s);
      } else if (s.scope === "project") {
        projectSkills.push(s);
      } else {
        userSkills.push(s);
      }
    }

    return { activeSkills, projectSkills, userSkills };
  });

  function toggleExpand(name: string) {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(name)) next.delete(name);
      else next.add(name);
      return next;
    });
  }

  async function handleToggle(name: string, enable: boolean) {
    if (!hasSeed()) return;
    if (pending().has(name)) return;

    setPending((prev) => new Set(prev).add(name));
    setErrors((prev) => {
      const next = { ...prev };
      delete next[name];
      return next;
    });

    try {
      if (enable) {
        await props.onActivate(name);
      } else {
        await props.onUnload(name);
      }
    } catch (e) {
      setErrors((prev) => ({ ...prev, [name]: String(e) }));
    } finally {
      setPending((prev) => {
        const next = new Set(prev);
        next.delete(name);
        return next;
      });
    }
  }

  async function handleRefresh() {
    if (!hasSeed()) return;
    setRefreshing(true);
    try {
      await props.onReload();
    } catch (e) {
      // silently ignore — individual skill errors handled per-row
    } finally {
      setRefreshing(false);
    }
  }

  const scopeLabel = (scope: string) =>
    scope === "project" ? t().skills.scopeProject : t().skills.scopeUser;

  return (
    <div class="skills-page">
      {/* Header */}
      <div class="skills-header">
        <h1>{t().skills.title}</h1>
        <div class="skills-header-actions">
          <div class="skill-search">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <circle cx="11" cy="11" r="8" />
              <line x1="21" y1="21" x2="16.65" y2="16.65" />
            </svg>
            <input
              type="text"
              value={search()}
              onInput={(e) => setSearch(e.currentTarget.value)}
              placeholder={t().skills.searchPlaceholder}
              disabled={!hasSeed()}
            />
          </div>
          <button
            class="skill-refresh-btn"
            onClick={handleRefresh}
            disabled={!hasSeed() || refreshing()}
          >
            <svg
              width="14" height="14" viewBox="0 0 24 24"
              fill="none" stroke="currentColor" stroke-width="2"
              classList={{ "skill-refresh-spin": refreshing() }}
            >
              <polyline points="23 4 23 10 17 10" />
              <polyline points="1 20 1 14 7 14" />
              <path d="M3.51 9a9 9 0 0114.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0020.49 15" />
            </svg>
            {t().skills.refresh}
          </button>
        </div>
      </div>

      {/* No seed state */}
      <Show
        when={hasSeed()}
        fallback={
          <div class="skills-empty">
            <p>{t().skills.noSession}</p>
          </div>
        }
      >
        {/* Empty state */}
        <Show when={props.available.length > 0} fallback={
          <div class="skills-empty">
            <p>{t().skills.empty}</p>
          </div>
        }>
          <div class="skills-body">
            {/* Active (enabled) skills */}
            <Show when={grouped().activeSkills.length > 0}>
              <section class="skill-group">
                <h2 class="skill-group-title">{t().skills.groupEnabled}</h2>
                <For each={grouped().activeSkills}>
                  {(s) => (
                    <SkillRow
                      skill={s}
                      active={true}
                      pending={pending().has(s.name)}
                      error={errors()[s.name]}
                      expanded={expanded().has(s.name)}
                      onToggle={() => handleToggle(s.name, false)}
                      onExpand={() => toggleExpand(s.name)}
                      scopeLabel={scopeLabel(s.scope)}
                      disabled={!hasSeed()}
                    />
                  )}
                </For>
              </section>
            </Show>

            {/* Project skills */}
            <Show when={grouped().projectSkills.length > 0}>
              <section class="skill-group">
                <h2 class="skill-group-title">{t().skills.groupProject}</h2>
                <For each={grouped().projectSkills}>
                  {(s) => (
                    <SkillRow
                      skill={s}
                      active={false}
                      pending={pending().has(s.name)}
                      error={errors()[s.name]}
                      expanded={expanded().has(s.name)}
                      onToggle={() => handleToggle(s.name, true)}
                      onExpand={() => toggleExpand(s.name)}
                      scopeLabel={scopeLabel(s.scope)}
                      disabled={!hasSeed()}
                    />
                  )}
                </For>
              </section>
            </Show>

            {/* User skills */}
            <Show when={grouped().userSkills.length > 0}>
              <section class="skill-group">
                <h2 class="skill-group-title">{t().skills.groupUser}</h2>
                <For each={grouped().userSkills}>
                  {(s) => (
                    <SkillRow
                      skill={s}
                      active={false}
                      pending={pending().has(s.name)}
                      error={errors()[s.name]}
                      expanded={expanded().has(s.name)}
                      onToggle={() => handleToggle(s.name, true)}
                      onExpand={() => toggleExpand(s.name)}
                      scopeLabel={scopeLabel(s.scope)}
                      disabled={!hasSeed()}
                    />
                  )}
                </For>
              </section>
            </Show>

            {/* No results after search */}
            <Show when={
              grouped().activeSkills.length === 0 &&
              grouped().projectSkills.length === 0 &&
              grouped().userSkills.length === 0 &&
              search().length > 0
            }>
              <div class="skills-empty">
                <p>{t().skills.noResults}</p>
              </div>
            </Show>
          </div>
        </Show>
      </Show>
    </div>
  );
}

/* ── SkillRow ── */
function SkillRow(props: {
  skill: SkillInfo;
  active: boolean;
  pending: boolean;
  error?: string;
  expanded: boolean;
  onToggle: () => void;
  onExpand: () => void;
  scopeLabel: string;
  disabled: boolean;
}) {
  return (
    <div class={`skill-row${props.active ? " enabled" : ""}${props.pending ? " pending" : ""}`}>
      <div class="skill-row-main" onClick={props.onExpand}>
        <div class="skill-row-left">
          <span class="skill-name">{props.skill.name}</span>
          <span class="skill-desc-excerpt">
            {props.expanded ? props.skill.description : props.skill.description.slice(0, 80) + (props.skill.description.length > 80 ? "…" : "")}
          </span>
        </div>
        <div class="skill-row-meta">
          <span class="skill-scope-badge">{props.scopeLabel}</span>
          <span class="skill-source">{props.skill.source}</span>
        </div>
      </div>
      <div class="skill-row-actions">
        <Show when={props.pending}>
          <span class="skill-spinner" />
        </Show>
        <Show when={!props.pending}>
          <label class="skill-toggle">
            <input
              type="checkbox"
              checked={props.active}
              onChange={props.onToggle}
              disabled={props.disabled}
            />
            <span class="skill-toggle-track" />
          </label>
        </Show>
      </div>

      {/* Expanded detail */}
      <Show when={props.expanded}>
        <div class="skill-detail">
          <p class="skill-detail-desc">{props.skill.description}</p>
          <div class="skill-detail-meta">
            <span>{props.scopeLabel}</span>
            <span class="skill-detail-source">{props.skill.source}</span>
          </div>
        </div>
      </Show>

      {/* Error */}
      <Show when={props.error}>
        <div class="skill-error">{props.error}</div>
      </Show>
    </div>
  );
}
