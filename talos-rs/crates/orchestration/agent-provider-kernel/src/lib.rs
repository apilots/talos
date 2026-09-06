//! Built-in adapter between the provider-neutral Agent SPI and Agent Kernel.
//!
//! This crate is the only production boundary allowed to interpret both
//! `talos-agent-api` and `talos-agent-kernel` execution events.

mod artifacts;
mod compaction;
mod config;
mod continuation;
mod dream_memory;
mod event_mapper;
mod execution;
mod history;
mod input;
mod provider;
mod skill_roots;
mod transcript;

pub use artifacts::FileKernelSessionArtifactStore;
pub use config::normalize_kernel_config;
pub use dream_memory::{KernelDreamMemoryParams, KernelDreamMemoryResult};
pub use history::KernelSessionPromptContextSource;
pub use provider::{KernelAgentProvider, KernelAgentProviderParams, KernelAgentSession};
pub use skill_roots::{empty_skill_roots_resolver, EmptySkillRootsResolver, SkillRootsResolver};
pub use talos_agent_kernel::{KernelMemoryProgressStore, KernelSessionArtifactStore};
pub use transcript::KernelTranscriptStore;
