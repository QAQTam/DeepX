# Permission Level Selector Design

## Goal

Expose DeepX permission levels 1-4 beside the composer and in Settings, with immediate persistence and a clear warning for unrestricted access.

## Design

- `App` owns the current permission level as the single UI source of truth.
- `cmd_load_config` returns `permission_level`; startup initializes the shared signal.
- A dedicated `cmd_set_permission_level(level)` validates 1-4, saves config, and broadcasts `ReloadConfig`.
- Composer renders a compact select. Settings renders the same four choices and uses the same callback.
- Level 4 uses danger styling; approval prompts retain their existing risk styling.

## Levels

1. Read only
2. Workspace write
3. Controlled execution
4. Full access

## Verification

- Component test covers all options, selection callback, and Level 4 danger state.
- Backend validation test covers invalid levels.
- Full frontend tests and build must pass.
