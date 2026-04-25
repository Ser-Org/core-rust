use axum::{routing::get, Router};
use scout_core::{
    app_state::AppState,
    billing,
    config::Config,
    db, jobs, logging, media,
    middleware::AuthConfig,
    objectstore, prompts,
    providers::{
        claude::ClaudeProvider, flux::FluxProvider, flux::MockFluxProvider, mock::*,
        ollama::OllamaProvider, runway::RunwayProvider, FlashImageProviderRef, ImageProviderRef,
        TextProviderRef, VideoProviderRef,
    },
    repos, router,
};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::oneshot;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = Config::load();
    logging::init(&cfg.log_level);

    tracing::info!(
        port = %cfg.port,
        log_level = %cfg.log_level,
        app_url = %cfg.app_url,
        runtime_environment = %cfg.runtime_environment,
        text_provider = %cfg.text_provider,
        video_provider = %cfg.video_provider,
        flash_provider = %cfg.flash_provider,
        ollama_url = %cfg.ollama_url,
        ollama_model = %cfg.ollama_model,
        llm_retry_enabled = cfg.llm_retry_enabled,
        log_llm_interaction = cfg.log_llm_interaction,
        s3_endpoint = %cfg.s3_endpoint,
        s3_region = %cfg.s3_region,
        s3_bucket = %cfg.s3_bucket,
        character_palettes_bucket = %cfg.character_palettes_bucket,
        flash_images_bucket = %cfg.flash_images_bucket,
        flash_audio_bucket = %cfg.flash_audio_bucket,
        email_from_address = %cfg.email_from_address,
        scenario_planner_dev_phase_count = cfg.scenario_planner_dev_phase_count,
        simulation_video_clip_duration_secs = cfg.simulation_video_clip_duration_secs,
        billing_enabled = cfg.billing_enabled(),
        "scout-core (rust) starting"
    );

    // Start a real HTTP server with only /api/v1/health wired up, running in
    // a background task, so Railway's healthcheck passes while we do the
    // heavy init below (db connect + migrations + bucket setup + JWKS fetch).
    // We hand off to the full router once everything is ready.
    let listen_addr = cfg.listen_addr();
    let bootstrap_listener = TcpListener::bind(&listen_addr).await?;
    tracing::info!(addr = %listen_addr, "bootstrap server listening (health-only)");

    let (bootstrap_shutdown_tx, bootstrap_shutdown_rx) = oneshot::channel::<()>();
    let bootstrap_app = Router::new().route("/api/v1/health", get(|| async { "ok" }));
    let bootstrap_handle = tokio::spawn(async move {
        axum::serve(bootstrap_listener, bootstrap_app)
            .with_graceful_shutdown(async move {
                let _ = bootstrap_shutdown_rx.await;
            })
            .await
    });

    let pool = db::connect(&cfg.database_url).await?;
    tracing::info!("database connected");
    db::migrate(&pool).await?;
    tracing::info!("migrations applied");

    let mut object_store = objectstore::ObjectStore::new(
        &cfg.s3_endpoint,
        &cfg.s3_access_key,
        &cfg.s3_secret_key,
        &cfg.s3_region,
    )
    .await?;
    object_store.set_supabase_credentials(&cfg.supabase_url, &cfg.supabase_service_key);
    for bucket in &[
        &cfg.s3_bucket,
        &cfg.character_palettes_bucket,
        &cfg.flash_images_bucket,
        &cfg.flash_audio_bucket,
    ] {
        if let Err(e) = object_store.ensure_bucket(bucket).await {
            tracing::warn!(bucket = %bucket, error = ?e, "ensure bucket failed — continuing");
        }
    }
    let object_store = Arc::new(object_store);

    let user_repo = Arc::new(repos::UserRepository::new(pool.clone()));
    let decision_repo = Arc::new(repos::DecisionRepository::new(pool.clone()));
    let simulation_repo = Arc::new(repos::SimulationRepository::new(pool.clone()));
    let media_repo = Arc::new(repos::MediaRepository::new(pool.clone()));
    let scenario_repo = Arc::new(repos::ScenarioRepository::new(pool.clone()));
    let subscription_repo = Arc::new(repos::SubscriptionRepository::new(pool.clone()));
    let flash_repo = Arc::new(repos::FlashRepository::new(pool.clone()));
    let waitlist_repo = Arc::new(repos::WaitlistRepository::new(pool.clone()));
    let components_repo = Arc::new(repos::SimulationComponentsRepo::new(pool.clone()));

    let text_provider: TextProviderRef = match cfg.text_provider.as_str() {
        "claude" => Arc::new(ClaudeProvider::new(
            cfg.claude_api_key.clone(),
            cfg.log_llm_interaction,
        )),
        "ollama" => Arc::new(OllamaProvider::new(
            cfg.ollama_url.clone(),
            cfg.ollama_model.clone(),
            cfg.log_llm_interaction,
        )),
        _ => Arc::new(MockTextProvider::new()),
    };
    let video_provider: VideoProviderRef = match cfg.video_provider.as_str() {
        "runway" | "veo3" | "seedance2" => Arc::new(RunwayProvider::new(
            cfg.runway_api_key.clone(),
            cfg.log_llm_interaction,
        )),
        _ => Arc::new(MockVideoProvider::new("testdata/placeholder.mp4")),
    };
    let image_provider: ImageProviderRef = match cfg.video_provider.as_str() {
        "runway" | "veo3" | "seedance2" => Arc::new(RunwayProvider::new(
            cfg.runway_api_key.clone(),
            cfg.log_llm_interaction,
        )),
        _ => Arc::new(MockImageProvider::new()),
    };
    let flash_image_provider: FlashImageProviderRef = match cfg.flash_provider.as_str() {
        "gen4_image" => Arc::new(RunwayProvider::new(
            cfg.runway_api_key.clone(),
            cfg.log_llm_interaction,
        )),
        "flux" => {
            if !cfg.flux_api_key.is_empty() {
                Arc::new(FluxProvider::new(cfg.flux_api_key.clone()))
            } else {
                Arc::new(MockFluxProvider::new())
            }
        }
        _ => Arc::new(MockFluxProvider::new()),
    };

    let prompt_builder = Arc::new(prompts::PromptBuilder::new());
    let job_client = Arc::new(jobs::JobClient::new(pool.clone()));

    let billing_service = Arc::new(billing::BillingService::new(
        pool.clone(),
        user_repo.clone(),
        subscription_repo.clone(),
        cfg.app_url.clone(),
        cfg.stripe_secret_key.clone(),
        cfg.stripe_webhook_secret.clone(),
        cfg.stripe_starter_price_id.clone(),
        cfg.stripe_pro_price_id.clone(),
        cfg.stripe_family_price_id.clone(),
        cfg.stripe_extra_cinematic_price_id.clone(),
    ));

    let media_pipeline = Arc::new(media::MediaPipeline::new(
        object_store.clone(),
        image_provider.clone(),
        flash_image_provider.clone(),
        video_provider.clone(),
        cfg.s3_bucket.clone(),
        cfg.video_model.clone(),
    ));

    let state = AppState {
        cfg: cfg.clone(),
        pool: pool.clone(),
        user_repo,
        decision_repo,
        simulation_repo,
        media_repo,
        scenario_repo,
        subscription_repo,
        flash_repo,
        waitlist_repo,
        components_repo,
        object_store,
        text_provider,
        video_provider,
        image_provider,
        flash_image_provider,
        prompt_builder,
        job_client,
        billing: billing_service,
        media_pipeline,
    };

    // Start background job workers.
    jobs::worker::start(state.clone(), 4).await;
    tracing::info!("job workers started");

    let jwks_url = format!("{}/auth/v1/.well-known/jwks.json", cfg.supabase_url);
    let auth_cfg = AuthConfig::new(cfg.supabase_jwt_secret.clone(), jwks_url).await;

    let app = router::build_router(state, auth_cfg);
    tracing::info!("router ready, handing off from bootstrap server");

    // Shut down the bootstrap server and wait for its listener to drop, then
    // rebind on the same port and serve the full router.
    let _ = bootstrap_shutdown_tx.send(());
    match bootstrap_handle.await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => tracing::warn!(error = ?e, "bootstrap server exited with error"),
        Err(e) => tracing::warn!(error = ?e, "bootstrap server task panicked"),
    }

    let listener = TcpListener::bind(&listen_addr).await?;
    tracing::info!(addr = %listen_addr, "full server listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    tracing::info!("scout-core stopped");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        use tokio::signal::unix::{signal, SignalKind};
        let mut sig = signal(SignalKind::terminate()).expect("failed to install SIGTERM handler");
        sig.recv().await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("shutdown signal received");
}
