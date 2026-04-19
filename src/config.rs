use std::env;

#[derive(Debug, Clone)]
pub struct Config {
    pub port: String,
    pub log_level: String,

    pub database_url: String,

    pub supabase_url: String,
    pub supabase_service_key: String,
    pub supabase_jwt_secret: String,

    pub text_provider: String,
    pub video_provider: String,
    /// The model identifier the media pipeline passes to the video provider
    /// at stage 2 (e.g. `"gen4.5"`, `"veo3"`). Derived from `video_provider`
    /// at load time — not a separate env var.
    pub video_model: String,
    pub flash_provider: String,

    pub claude_api_key: String,

    pub ollama_url: String,
    pub ollama_model: String,

    pub runway_api_key: String,

    pub flux_api_key: String,

    pub stripe_secret_key: String,
    pub stripe_webhook_secret: String,
    pub stripe_starter_price_id: String,
    pub stripe_pro_price_id: String,
    pub stripe_family_price_id: String,
    pub stripe_extra_cinematic_price_id: String,
    pub stripe_extra_whatif_price_id: String,
    pub stripe_whatif_10pack_price_id: String,

    pub resend_api_key: String,
    pub email_from_address: String,

    pub s3_endpoint: String,
    pub s3_access_key: String,
    pub s3_secret_key: String,
    pub s3_bucket: String,
    pub character_palettes_bucket: String,
    pub flash_images_bucket: String,
    pub flash_audio_bucket: String,
    pub s3_region: String,

    pub app_url: String,
    pub runtime_environment: String,

    pub llm_retry_enabled: bool,
    pub log_llm_interaction: bool,

    pub scenario_planner_dev_phase_count: i32,
    pub simulation_video_clip_duration_secs: i32,
}

impl Config {
    pub fn load() -> Self {
        let _ = dotenvy::dotenv();

        let app_url = env_or_default("APP_URL", "http://localhost:3000");
        let runtime_environment = detect_runtime_environment(&app_url);
        let dev_overrides_enabled = runtime_environment == "development";

        let scenario_planner_dev_phase_count = if dev_overrides_enabled {
            env_int("SCENARIO_PLANNER_DEV_PHASE_COUNT", 0)
        } else {
            0
        };
        let video_provider = env_or_default("VIDEO_PROVIDER", "mock");
        // Veo3 (Runway-hosted Google Veo 3) forces an 8-second clip duration;
        // runway/seedance2/mock use the product-standard 6s (with dev override
        // allowed). Seedance 2.0 accepts 4-15s; 6s sits inside that range.
        let video_model = match video_provider.as_str() {
            "veo3" => "veo3".to_string(),
            "seedance2" => "seedance2".to_string(),
            "runway" => "gen4.5".to_string(),
            _ => "gen4.5".to_string(),
        };
        let simulation_video_clip_duration_secs = if video_provider == "veo3" {
            8
        } else if dev_overrides_enabled {
            env_positive_int("SIMULATION_VIDEO_CLIP_DURATION_SECS", 6)
        } else {
            6
        };

        let cfg = Self {
            port: env_or_default("PORT", "8080"),
            log_level: env_or_default("LOG_LEVEL", "info"),

            database_url: require_env("DATABASE_URL"),

            supabase_url: require_env("SUPABASE_URL"),
            supabase_service_key: require_env("SUPABASE_SERVICE_KEY"),
            supabase_jwt_secret: require_env("SUPABASE_JWT_SECRET"),

            text_provider: env_or_default("TEXT_PROVIDER", "ollama"),
            video_provider,
            video_model,
            flash_provider: env_or_default("FLASH_PROVIDER", "flux"),

            claude_api_key: env::var("CLAUDE_API_KEY").unwrap_or_default(),
            ollama_url: env_or_default("OLLAMA_URL", "http://localhost:11434"),
            ollama_model: env_or_default("OLLAMA_MODEL", "llama3:8b"),
            runway_api_key: env::var("RUNWAY_API_KEY").unwrap_or_default(),
            flux_api_key: env::var("BFL_API_KEY").unwrap_or_default(),

            stripe_secret_key: env::var("STRIPE_SECRET_KEY").unwrap_or_default(),
            stripe_webhook_secret: env::var("STRIPE_WEBHOOK_SECRET").unwrap_or_default(),
            stripe_starter_price_id: env::var("STRIPE_STARTER_PRICE_ID").unwrap_or_default(),
            stripe_pro_price_id: env::var("STRIPE_PRO_PRICE_ID").unwrap_or_default(),
            stripe_family_price_id: env::var("STRIPE_FAMILY_PRICE_ID").unwrap_or_default(),
            stripe_extra_cinematic_price_id: env::var("STRIPE_EXTRA_CINEMATIC_PRICE_ID")
                .unwrap_or_default(),
            stripe_extra_whatif_price_id: env::var("STRIPE_EXTRA_WHATIF_PRICE_ID")
                .unwrap_or_default(),
            stripe_whatif_10pack_price_id: env::var("STRIPE_WHATIF_10PACK_PRICE_ID")
                .unwrap_or_default(),
            resend_api_key: env::var("RESEND_API_KEY").unwrap_or_default(),
            email_from_address: env_or_default(
                "EMAIL_FROM_ADDRESS",
                "Scout <no-reply@scout.local>",
            ),

            s3_endpoint: env_or_default("S3_ENDPOINT", "http://127.0.0.1:54321/storage/v1/s3"),
            s3_access_key: env_or_default("S3_ACCESS_KEY", ""),
            s3_secret_key: env_or_default("S3_SECRET_KEY", ""),
            s3_bucket: env_or_default("S3_BUCKET", "scout-media"),
            character_palettes_bucket: env_or_default("CHARACTER_PALETTES_BUCKET", "character-palettes"),
            flash_images_bucket: env_or_default("FLASH_IMAGES_BUCKET", "flash-images"),
            flash_audio_bucket: env_or_default("FLASH_AUDIO_BUCKET", "flash-audio"),
            s3_region: env_or_default("S3_REGION", "local"),

            app_url,
            runtime_environment,

            llm_retry_enabled: env_bool("LLM_RETRY_ENABLED", false),
            log_llm_interaction: env_bool("LOG_LLM_INTERACTIONS", false),
            scenario_planner_dev_phase_count,
            simulation_video_clip_duration_secs,
        };

        match cfg.text_provider.as_str() {
            "claude" => {
                if cfg.claude_api_key.is_empty() {
                    panic!("config: CLAUDE_API_KEY is required when TEXT_PROVIDER=claude");
                }
            }
            "ollama" | "mock" => {}
            other => panic!(
                "config: unknown TEXT_PROVIDER {:?} (expected claude, ollama, or mock)",
                other
            ),
        }

        match cfg.video_provider.as_str() {
            "runway" | "veo3" | "seedance2" => {
                if cfg.runway_api_key.is_empty() {
                    panic!(
                        "config: RUNWAY_API_KEY is required when VIDEO_PROVIDER={}",
                        cfg.video_provider
                    );
                }
            }
            "mock" => {}
            other => panic!(
                "config: unknown VIDEO_PROVIDER {:?} (expected runway, veo3, seedance2, or mock)",
                other
            ),
        }

        match cfg.flash_provider.as_str() {
            "gen4_image" => {
                if cfg.runway_api_key.is_empty() {
                    panic!("config: RUNWAY_API_KEY is required when FLASH_PROVIDER=gen4_image");
                }
            }
            "flux" | "mock" => {}
            other => panic!(
                "config: unknown FLASH_PROVIDER {:?} (expected flux, gen4_image, or mock)",
                other
            ),
        }

        cfg
    }

    pub fn port_int(&self) -> u16 {
        self.port
            .parse()
            .unwrap_or_else(|_| panic!("config: PORT {:?} is not a valid integer", self.port))
    }

    pub fn listen_addr(&self) -> String {
        format!("0.0.0.0:{}", self.port)
    }

    pub fn is_development(&self) -> bool {
        self.runtime_environment == "development"
    }

    pub fn billing_enabled(&self) -> bool {
        !self.stripe_secret_key.is_empty()
            && !self.stripe_starter_price_id.is_empty()
            && !self.stripe_pro_price_id.is_empty()
            && !self.stripe_family_price_id.is_empty()
    }
}

fn require_env(key: &str) -> String {
    env::var(key).ok().filter(|v| !v.is_empty()).unwrap_or_else(|| {
        panic!("config: required environment variable {} is not set", key)
    })
}

fn env_or_default(key: &str, fallback: &str) -> String {
    env::var(key)
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| fallback.to_string())
}

fn env_bool(key: &str, fallback: bool) -> bool {
    match env::var(key) {
        Ok(v) if !v.is_empty() => match v.to_lowercase().as_str() {
            "1" | "t" | "true" | "yes" | "on" => true,
            "0" | "f" | "false" | "no" | "off" => false,
            _ => fallback,
        },
        _ => fallback,
    }
}

fn env_int(key: &str, fallback: i32) -> i32 {
    env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(fallback)
}

fn env_positive_int(key: &str, fallback: i32) -> i32 {
    let n = env_int(key, fallback);
    if n <= 0 {
        fallback
    } else {
        n
    }
}

fn detect_runtime_environment(app_url: &str) -> String {
    for key in ["APP_ENV", "ENVIRONMENT", "GO_ENV", "NODE_ENV", "VERCEL_ENV"] {
        if let Ok(v) = env::var(key) {
            let n = normalize_runtime(&v);
            if !n.is_empty() {
                return n;
            }
        }
    }
    let lower = app_url.trim().to_lowercase();
    if lower.contains("localhost") || lower.contains("127.0.0.1") {
        "development".to_string()
    } else {
        "production".to_string()
    }
}

fn normalize_runtime(raw: &str) -> String {
    match raw.trim().to_lowercase().as_str() {
        "" => String::new(),
        "production" | "prod" => "production".to_string(),
        "development" | "dev" | "local" => "development".to_string(),
        "test" | "testing" => "test".to_string(),
        "staging" | "stage" | "preview" => "staging".to_string(),
        other => other.to_string(),
    }
}
