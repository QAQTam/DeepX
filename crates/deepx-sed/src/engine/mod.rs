// Copyright (c) 2026 Red Authors
// License: MIT
//

pub mod addr;
pub mod exec;
pub mod pattern_space;
pub mod types;

pub use addr::{AddressEvaluator, ExecutionContext};
pub use exec::apply_commands_with_context;
pub use types::{Command, CommandResult, CompiledSubstitution, SedRegex};
