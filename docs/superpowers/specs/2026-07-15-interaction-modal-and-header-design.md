# Interaction Modal and Header Design

## Goal

Make blocking interactions impossible to miss, clarify context controls, and preserve user-expanded process details.

## Design

- Render Permission and AskUser through a document-level centered modal with a full-app backdrop.
- Require explicit action; backdrop clicks do not dismiss or reject.
- Keep compact progress near the composer instead of placing it in the modal.
- Replace the overflow glyph with an explicit `整理上下文` action.
- Add a header workspace button showing the current folder name; clicking it opens the workspace picker. Keep the sidebar editor.
- Auto-collapse process details only on the transition into `completed`; later projection refreshes must preserve manual expansion.

## Constraints

- High-risk approval remains red.
- Modal content scrolls within `75vh` and remains keyboard accessible.
- No backend protocol changes.
