//! Fail-closed regression suite for the local-only fork (PR6).
//!
//! Pins kill-switches from PRs 1–5 so cloud collection cannot quietly return.
//! Prefer GROK_HOME-isolated checks where filesystem state matters.

#![cfg(feature = "local-only")]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::{Router, response::IntoResponse, routing::any};
use futures_util::StreamExt;
use indexmap::IndexMap;
use serial_test::serial;
use tokio::net::TcpListener;
use xai_file_utils::storage_client::{ExistsResult, LOCAL_ONLY_STORAGE_MSG, StorageClient};
use xai_grok_shell::agent::config::{
    CLI_CHAT_PROXY_BASE_URL_DEFAULT, Config, EndpointsConfig, TelemetryMode,
    XAI_API_BASE_URL_DEFAULT,
};
use xai_grok_shell::sampling::{
    ApiBackend, ConversationItem, ConversationRequest, SamplerConfig, new_client,
};
use xai_grok_shell::upload::local_traces::{LocalTracesConfig, resolve_enabled, write_turn_trace};
use xai_grok_shell::util::config::resolve_remote_fetch_enabled;
use xai_grok_shell::util::{ensure_inference_url_allowed, is_denied_cloud_host};
use xai_grok_test_support::{EnvGuard, MockInferenceServer};

mod common;
use common::test_sampler_config;

fn empty_endpoints() -> EndpointsConfig {
    EndpointsConfig {
        cli_chat_proxy_base_url: None,
        models_base_url: None,
        xai_api_base_url: String::new(),
        ..Default::default()
    }
}

/// 1. Empty user config must never resolve to cli-chat-proxy / api.x.ai.
#[test]
#[serial]
fn no_default_cloud_host() {
    let _a = EnvGuard::unset("GROK_CLI_CHAT_PROXY_BASE_URL");
    let _b = EnvGuard::unset("GROK_XAI_API_BASE_URL");
    let _c = EnvGuard::unset("GROK_MODELS_BASE_URL");

    assert!(
        CLI_CHAT_PROXY_BASE_URL_DEFAULT.is_empty(),
        "compiled proxy default must be empty under local-only"
    );
    assert!(
        XAI_API_BASE_URL_DEFAULT.is_empty(),
        "compiled xAI API default must be empty under local-only"
    );
    assert!(
        xai_grok_shell::env::PROD_CLI_CHAT_PROXY_BASE_URL.is_empty(),
        "env bake-in proxy must be empty under local-only"
    );

    let ep = empty_endpoints();
    assert!(
        ep.proxy_url().is_empty(),
        "proxy_url must not invent a cloud host"
    );
    assert!(
        ep.resolve_inference_base_url().is_empty(),
        "inference must not fall back to cli-chat-proxy"
    );
}

/// 2. Deny-list rejects *.x.ai / *.grok.com at URL helpers + sampler construction.
#[test]
fn deny_list_blocks_cloud_inference_urls() {
    for url in [
        "https://api.x.ai/v1",
        "https://cli-chat-proxy.grok.com/v1",
        "wss://code.grok.com/ws/code-agent",
    ] {
        assert!(is_denied_cloud_host(url), "expected deny for {url}");
        let err = ensure_inference_url_allowed(url).expect_err("must reject");
        assert!(
            err.contains("blocked host") || err.contains("denied"),
            "unexpected error for {url}: {err}"
        );
        let mut cfg = test_sampler_config(url, ApiBackend::ChatCompletions, &[]);
        cfg.api_key = None;
        assert!(
            new_client(cfg).is_err(),
            "sampler must refuse cloud host {url}"
        );
    }

    assert!(ensure_inference_url_allowed("http://127.0.0.1:11434/v1").is_ok());
    assert!(
        new_client(test_sampler_config(
            "http://127.0.0.1:11434/v1",
            ApiBackend::ChatCompletions,
            &[],
        ))
        .is_ok()
    );
    assert!(
        ensure_inference_url_allowed("").is_err(),
        "empty base_url must fail closed"
    );
}

/// 3. StorageClient entrypoints perform zero HTTP under local-only.
#[tokio::test]
async fn storage_client_performs_zero_http() {
    let hits = Arc::new(AtomicUsize::new(0));
    let hits_filter = hits.clone();
    let router = Router::new().fallback(any(move || {
        let hits = hits_filter.clone();
        async move {
            hits.fetch_add(1, Ordering::SeqCst);
            (axum::http::StatusCode::OK, "should never be reached").into_response()
        }
    }));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    let client = StorageClient::new(&format!("http://{addr}/v1"), "unused-token");

    let err = client
        .get_upload_limits()
        .await
        .expect_err("upload limits must be denied");
    assert!(
        err.to_string().contains(LOCAL_ONLY_STORAGE_MSG),
        "got: {err}"
    );
    assert!(matches!(
        client.check_exists("path/a").await,
        ExistsResult::ProbeFailed
    ));
    let upload_err = client
        .upload("path/a", b"payload", "text/plain")
        .await
        .expect_err("upload must be denied");
    assert!(
        upload_err.to_string().contains(LOCAL_ONLY_STORAGE_MSG),
        "got: {upload_err}"
    );
    assert_eq!(
        hits.load(Ordering::SeqCst),
        0,
        "StorageClient must not contact the listener under local-only"
    );
}

/// 4. Remote settings cannot re-arm uploads or remote_fetch.
#[test]
#[serial]
fn remote_settings_cannot_rearm_cloud() {
    let _a = EnvGuard::unset("GROK_TELEMETRY_ENABLED");
    let _b = EnvGuard::unset("GROK_TELEMETRY_TRACE_UPLOAD");
    let _c = EnvGuard::unset("GROK_LOCAL_TRACES");

    assert!(
        !resolve_remote_fetch_enabled(),
        "remote_fetch must stay off under local-only regardless of config layers"
    );

    let mut cfg = Config::default();
    cfg.features.telemetry = Some(TelemetryMode::Enabled);
    cfg.telemetry.trace_upload = Some(true);
    cfg.remote_settings = Some(xai_grok_shell::util::config::RemoteSettings {
        trace_upload_enabled: Some(true),
        ..Default::default()
    });
    assert!(
        !cfg.is_trace_upload_enabled(),
        "trace_upload must ignore remote + config under local-only"
    );
    assert!(
        !cfg.is_telemetry_enabled(),
        "telemetry must stay forced off under local-only"
    );
}

/// 5. Local traces stay opt-in; enabled writes under GROK_HOME/traces/.
#[test]
#[serial]
fn local_traces_opt_in_only() {
    let _env = EnvGuard::unset("GROK_LOCAL_TRACES");
    let home = tempfile::tempdir().unwrap();
    let off = LocalTracesConfig::default();
    assert!(!resolve_enabled(&off));
    assert!(!write_turn_trace(home.path(), &off, "sess", 1, Some("{}")).unwrap());
    assert!(!home.path().join("traces").exists());

    let on = LocalTracesConfig {
        enabled: true,
        ..Default::default()
    };
    assert!(write_turn_trace(home.path(), &on, "sess", 2, Some("{\"ok\":1}\n")).unwrap());
    let turn = home.path().join("traces/sess/turn_2");
    assert!(turn.join("metadata.json").is_file());
    assert!(turn.join("messages.jsonl").is_file());
}

/// 6. Agent completes a turn against a local OpenAI stub without auth.json.
#[tokio::test]
async fn no_auth_local_turn() {
    let grok_home = tempfile::tempdir().unwrap();
    assert!(
        !grok_home.path().join("auth.json").exists(),
        "precondition: no auth.json"
    );

    assert!(is_denied_cloud_host("https://api.x.ai/v1"));
    assert!(
        new_client(test_sampler_config(
            "https://api.x.ai/v1",
            ApiBackend::ChatCompletions,
            &[],
        ))
        .is_err(),
        "api.x.ai must be rejected before any HTTP"
    );

    let server = MockInferenceServer::start().await.unwrap();
    server.set_response("hello from local stub");

    let mut cfg = test_sampler_config(&server.url(), ApiBackend::ChatCompletions, &[]);
    cfg.api_key = None;
    // Ensure no accidental cloud headers from defaults.
    cfg.extra_headers = IndexMap::new();
    let client = new_client(cfg).expect("local stub base_url must be allowed");

    let request = ConversationRequest::from_items(vec![
        ConversationItem::system("You are a helpful assistant."),
        ConversationItem::user("Say hello"),
    ]);
    let (mut stream, _meta) = client
        .conversation_stream(request)
        .await
        .expect("streaming turn must succeed without auth.json");

    let mut content = String::new();
    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result.expect("stream chunk");
        for choice in chunk.choices {
            if let Some(ref text) = choice.delta.content {
                content.push_str(text);
            }
        }
    }
    assert!(
        content.contains("hello"),
        "expected assistant text from local stub, got: {content:?}"
    );
    assert!(
        server.request_count() > 0,
        "local inference stub should have been hit"
    );
}

/// 7. Repo collectors stay stubbed in source (compile-time tripwire).
#[test]
fn repo_collectors_remain_unavailable() {
    let git_ext = include_str!("../src/extensions/git.rs");
    assert!(
        git_ext.contains("git serialize_changes is unavailable in this build"),
        "shell serialize_changes stub must remain"
    );
    let ws_ops = include_str!("../../xai-grok-workspace/src/workspace_ops.rs");
    assert!(
        ws_ops.contains("git collect changes is unavailable in this build"),
        "workspace GitCollectChangesReq stub must remain"
    );
}

#[allow(dead_code)]
fn _sampler_config_type_anchor() -> SamplerConfig {
    SamplerConfig::default()
}
