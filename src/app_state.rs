use crate::{
    billing::BillingService,
    causalgraph::CausalGraph,
    config::Config,
    jobs::JobClient,
    media::MediaPipeline,
    objectstore::ObjectStore,
    prompts::PromptBuilder,
    providers::{FlashImageProviderRef, ImageProviderRef, TextProviderRef, VideoProviderRef},
    repos::*,
};
use sqlx::PgPool;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub cfg: Config,
    pub pool: PgPool,
    pub user_repo: Arc<UserRepository>,
    pub decision_repo: Arc<DecisionRepository>,
    pub simulation_repo: Arc<SimulationRepository>,
    pub media_repo: Arc<MediaRepository>,
    pub scenario_repo: Arc<ScenarioRepository>,
    pub subscription_repo: Arc<SubscriptionRepository>,
    pub flash_repo: Arc<FlashRepository>,
    pub waitlist_repo: Arc<WaitlistRepository>,
    pub components_repo: Arc<SimulationComponentsRepo>,
    pub object_store: Arc<ObjectStore>,
    pub text_provider: TextProviderRef,
    pub video_provider: VideoProviderRef,
    pub image_provider: ImageProviderRef,
    pub flash_image_provider: FlashImageProviderRef,
    pub prompt_builder: Arc<PromptBuilder>,
    pub causal_graph: Arc<CausalGraph>,
    pub job_client: Arc<JobClient>,
    pub billing: Arc<BillingService>,
    pub media_pipeline: Arc<MediaPipeline>,
}
