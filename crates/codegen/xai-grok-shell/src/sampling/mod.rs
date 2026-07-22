pub mod conversation;
pub mod error;
pub mod types;

// `Client` is the legacy alias used throughout the shell. A later refactor
// retired the bespoke shell HTTP client and points `Client` at the sampler crate's
// `SamplingClient` -- the two have identical method sets, so call-sites
// compile unchanged.
pub use self::conversation::*;
pub use self::error::{ResponseModelMetadata, Result, SamplingError};
pub use self::types::*;
pub use xai_grok_sampler::ApiBackend;
pub use xai_grok_sampler::SamplingClient as Client;

// Re-export async-openai Responses API types under `rs` namespace
pub use async_openai::types::responses as rs;

// ---------------------------------------------------------------------------
// xai-grok-sampler re-exports
// ---------------------------------------------------------------------------
//
// The actual streaming / retry / HTTP-client logic lives in the
// `xai-grok-sampler` crate. We re-export the public surface here so
// `crate::sampling::{SamplerHandle, SamplerConfig, ...}` paths keep working
// for callers that haven't been ported to spell these directly via
// `xai_grok_sampler::*`. The shell-side `sampling::client::Config`
// composite was removed when its only remaining role -- session-snapshot
// state for `MvpAgent` -- was migrated to `RefCell<SamplerConfig>` directly.
pub use xai_grok_sampler::{
    InferenceLatencyStats, OriginClientInfo, RequestId, SamplerActor, SamplerConfig, SamplerHandle,
    SamplingChannel, SamplingClient, SamplingErrorInfo, SamplingErrorKind, SamplingEvent,
};

/// Construct a sampling client after local-only URL preflight (deny-list +
/// non-empty `base_url`). Prefer this over bare `SamplingClient::new` in shell.
pub fn new_client(config: SamplerConfig) -> std::result::Result<SamplingClient, SamplingError> {
    #[cfg(feature = "local-only")]
    {
        let trimmed = config.base_url.trim();
        if trimmed.is_empty() {
            return Err(SamplingError::InvalidConfiguration(
                "configure a local model base_url; xAI cloud is unsupported in this local-only build",
            ));
        }
        if crate::util::is_denied_cloud_host(trimmed) {
            tracing::error!(
                base_url = %trimmed,
                "blocked cloud inference URL in local-only build"
            );
            return Err(SamplingError::InvalidConfiguration(
                "blocked cloud host (*.x.ai / *.grok.com) in local-only build",
            ));
        }
    }
    SamplingClient::new(config)
}
