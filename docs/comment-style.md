# DeepX Comment Style Guide

## Module-level docs (`//!`)

Every `lib.rs` and `mod.rs` MUST start with a `//!` block:

```rust
//! crate-name — one-line summary.
//!
//! Expanded description of what this crate/module does.
//!
//! ## Key concepts (optional)
//!
//! ## Architecture (optional)
```

## Public API docs (`///`)

### Structs

```rust
/// Brief purpose.
///
/// Longer description if the struct has non-obvious state rules
/// (e.g. must call `init()` before use, not Clone, etc.).
pub struct MyStruct {
    /// What this field stores. Include constraints:
    /// "Non-empty", "Must be a valid absolute path", etc.
    pub field: Type,
}
```

### Enums

```rust
/// What this enum classifies / represents.
pub enum MyEnum {
    /// When this variant is produced and what it means.
    VariantA,
    /// When this variant is produced and what it means.
    VariantB,
}
```

### Functions

```rust
/// Brief: what this function does (imperative mood).
///
/// # Arguments
/// * `param` — description (only if non-obvious from name).
///
/// # Returns
/// Description of return value.
///
/// # Errors
/// When and why this returns an error.
///
/// # Panics
/// Conditions that cause panics (if any).
pub fn do_thing(param: &str) -> Result<Output, Error> { ... }
```

### Traits

```rust
/// What this trait abstracts.
///
/// # Implementors
/// Who should implement this, and any constraints.
pub trait MyTrait {
    /// What this method does.
    fn method(&self) -> Output;
}
```

## Inline comments (`//`)

Use `//` only when the code is non-obvious. Explain **why**, not **what**.

```rust
// State machine: Idle → Running → WaitingUser → Idle.
// We skip Running→Idle transition when tools are still executing.
if phase == LoopPhase::ToolsRunning { ... }
```

```rust
// Lock ordering: pending lock must be acquired BEFORE session lock
// to prevent deadlock with handle_cancel().
let pending = self.pending.lock().unwrap();
```

## Unsafe blocks

Every `unsafe { }` block MUST be immediately preceded by a `// SAFETY:` comment listing the invariants that make it sound.

```rust
// SAFETY: `ptr` was allocated by Vec::with_capacity(n) and we just
// verified that `i < n`, so the pointer is valid for writes and
// does not alias any other live reference.
unsafe { ptr.add(i).write(value); }
```

## Anti-patterns

| Don't | Do |
|-------|-----|
| `/// The name field.` | `/// User's display name. Non-empty, max 64 chars.` |
| `/// Obvious helper.` | Delete the comment — it adds nothing. |
| `/// Calls foo() and returns result.` | Explain *why* foo() is called here. |
| `// TODO: add docs` | Write the docs, or leave no comment. |
| Comment in Chinese | Stick to English (existing convention). |
