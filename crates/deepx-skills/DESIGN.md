# DeepX Skill Runtime

## Design alignment

DeepX uses the community Agent Skills specification as the portable file contract: `SKILL.md`, YAML metadata, standard naming rules, and progressive disclosure. The runtime architecture follows Codex's stronger separation of catalog metadata, explicit host activation, typed activation state, and bounded resource loading. It does not copy Codex-specific plugin, admin, or enterprise scope machinery.

DeepX-specific choices are deliberate: the selected workspace is the project trust boundary, discovery remains dynamic, and activation remains sticky for the current session because DeepX keeps a long-running multi-turn message store. The agent retains an exact rendered catalog snapshot and replaces it only when the effective metadata changes. These choices preserve interoperability without importing runtime policy that DeepX does not otherwise support.

## Runtime flow

1. `deepx-skills` scans bounded project and user roots and parses only `SKILL.md` metadata.
2. `AgentState::build_context` injects a transient catalog system message in a fixed slot after the base system prefix and before activated skill bodies. Unchanged catalogs reuse the retained rendered snapshot byte-for-byte.
3. `$skill-name` is resolved and injected by the host before the user turn.
4. Implicit activation uses the read-only `skills` tool with `action=activate`, which accepts a catalog name rather than a file path and returns the full body plus a resource manifest.
5. `deepx-tools` returns a typed `SkillActivation` effect only for a successful `skills(action=activate)` call. The loop promotes that typed effect to a protected system message; ordinary tool text cannot impersonate it. Protected messages survive normal result folding, compaction, and session snapshots.
6. The same tool uses `action=resource` for contained relative resources, `action=list` for effective precedence and diagnostics, and `action=validate` for strict portable-spec validation.

## Discovery precedence

Project scopes override user scopes:

1. `<workspace>/.deepx/skills/`
2. `<workspace>/.agents/skills/`
3. `<workspace>/skills/` (legacy DeepX compatibility)
4. `~/.deepx/skills/`
5. `~/.agents/skills/`

Scanning is deterministic and bounded by depth, directory count, skill count, file size, catalog size, and resource count. Symlink directories are not traversed.

## Security boundary

Generic file or network output cannot become instructions by copying the activation marker. Promotion also requires a successful typed effect from `skills(action=activate)`. The tool resolves names only from the discovered catalog and does not accept arbitrary paths.

The selected workspace is the project-skill trust boundary, matching DeepX's existing workspace execution model. Skill `allowed-tools` metadata is descriptive only and never bypasses DeepX permissions. Resource paths are canonicalized and must remain inside the skill directory.

## Lifecycle decision

Discovery is intentionally dynamic: each catalog build and activation resolves the current filesystem state, so creation, edits, and deletion require no process restart. Activation is session-sticky because DeepX uses a long-running multi-turn message store; a same-name reactivation replaces the protected instruction snapshot. A new session starts with no active skill body and retains only the dynamic catalog.
