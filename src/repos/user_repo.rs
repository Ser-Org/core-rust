use crate::models::*;
use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use serde_json::Value as JsonValue;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

#[derive(Clone)]
pub struct UserRepository {
    pool: PgPool,
}

#[derive(Debug, Clone, Default)]
pub struct UserCalibrationProfilePatch {
    pub estimated_net_worth: Option<f64>,
    pub estimated_yearly_salary: Option<f64>,
    pub risk_tolerance: Option<String>,
    pub follow_through: Option<String>,
    pub optimism_bias: Option<String>,
    pub stress_response: Option<String>,
    pub decision_style: Option<String>,
    pub saving_habits: Option<String>,
    pub debt_comfort: Option<String>,
    pub housing_stability: Option<String>,
    pub income_stability: Option<String>,
    pub liquid_net_worth_source: Option<String>,
    pub relationship_status: Option<String>,
    pub household_income_structure: Option<String>,
    pub dependent_count: Option<i32>,
    pub life_stability: Option<String>,
}

impl UserCalibrationProfilePatch {
    pub fn has_updates(&self) -> bool {
        self.estimated_net_worth.is_some()
            || self.estimated_yearly_salary.is_some()
            || self.risk_tolerance.is_some()
            || self.follow_through.is_some()
            || self.optimism_bias.is_some()
            || self.stress_response.is_some()
            || self.decision_style.is_some()
            || self.saving_habits.is_some()
            || self.debt_comfort.is_some()
            || self.housing_stability.is_some()
            || self.income_stability.is_some()
            || self.liquid_net_worth_source.is_some()
            || self.relationship_status.is_some()
            || self.household_income_structure.is_some()
            || self.dependent_count.is_some()
            || self.life_stability.is_some()
    }
}

#[derive(Debug, Clone)]
pub struct CinematicContextInput {
    pub age_bracket: String,
    pub gender: String,
    pub relationship_status: String,
    pub dependent_count: i32,
    pub living_situation: String,
    pub industry: String,
    pub career_stage: String,
    pub net_worth_bracket: String,
    pub income_bracket: String,
}

impl UserRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn create_user(&self, user: &User) -> Result<()> {
        sqlx::query(
            "INSERT INTO users (id, email, created_at, updated_at) VALUES ($1, $2, now(), now())",
        )
        .bind(user.id)
        .bind(&user.email)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_user_by_id(&self, user_id: Uuid) -> Result<User> {
        let u = sqlx::query_as::<_, User>(
            "SELECT id, email, created_at, updated_at FROM users WHERE id = $1",
        )
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(u)
    }

    pub async fn ensure_profile(&self, user_id: Uuid) -> Result<()> {
        sqlx::query(
            "INSERT INTO user_profiles (id, user_id, estimated_net_worth, estimated_yearly_salary, onboarding_status, created_at, updated_at)
             VALUES (gen_random_uuid(), $1, 0, 0, 'story_submitted', now(), now())
             ON CONFLICT (user_id) DO NOTHING",
        )
        .bind(user_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_profile_by_user_id(&self, user_id: Uuid) -> Result<UserProfile> {
        let p = sqlx::query_as::<_, UserProfile>(
            "SELECT id, user_id,
                estimated_net_worth::float8 AS estimated_net_worth,
                estimated_yearly_salary::float8 AS estimated_yearly_salary,
                onboarding_status,
                risk_tolerance, follow_through, optimism_bias, stress_response, decision_style,
                saving_habits, debt_comfort, housing_stability, income_stability,
                liquid_net_worth_source,
                relationship_status, household_income_structure, dependent_count, life_stability,
                onboarding_path,
                age_bracket, gender, living_situation, industry, career_stage,
                net_worth_bracket, income_bracket, cinematic_context_completed,
                created_at, updated_at
             FROM user_profiles WHERE user_id = $1",
        )
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(p)
    }

    pub async fn update_financials(
        &self,
        user_id: Uuid,
        net_worth: f64,
        yearly_salary: f64,
    ) -> Result<()> {
        let res = sqlx::query(
            "UPDATE user_profiles SET estimated_net_worth = $1, estimated_yearly_salary = $2, updated_at = now() WHERE user_id = $3",
        )
        .bind(net_worth)
        .bind(yearly_salary)
        .bind(user_id)
        .execute(&self.pool)
        .await?;
        if res.rows_affected() == 0 {
            return Err(anyhow!("no profile found for user {}", user_id));
        }
        Ok(())
    }

    pub async fn update_behavioral_profile(
        &self,
        user_id: Uuid,
        risk: Option<&str>,
        follow: Option<&str>,
        optim: Option<&str>,
        stress: Option<&str>,
        style: Option<&str>,
    ) -> Result<()> {
        let res = sqlx::query(
            "UPDATE user_profiles SET
               risk_tolerance = COALESCE($1, risk_tolerance),
               follow_through = COALESCE($2, follow_through),
               optimism_bias = COALESCE($3, optimism_bias),
               stress_response = COALESCE($4, stress_response),
               decision_style = COALESCE($5, decision_style),
               updated_at = now()
             WHERE user_id = $6",
        )
        .bind(risk)
        .bind(follow)
        .bind(optim)
        .bind(stress)
        .bind(style)
        .bind(user_id)
        .execute(&self.pool)
        .await?;
        if res.rows_affected() == 0 {
            return Err(anyhow!("no profile found for user {}", user_id));
        }
        Ok(())
    }

    pub async fn update_onboarding_status(&self, user_id: Uuid, status: &str) -> Result<()> {
        let res = sqlx::query(
            "UPDATE user_profiles SET onboarding_status = $1, updated_at = now() WHERE user_id = $2",
        )
        .bind(status)
        .bind(user_id)
        .execute(&self.pool)
        .await?;
        if res.rows_affected() == 0 {
            return Err(anyhow!("no profile found for user {}", user_id));
        }
        Ok(())
    }

    pub async fn set_onboarding_path(&self, user_id: Uuid, path: &str) -> Result<()> {
        let res = sqlx::query(
            "UPDATE user_profiles SET onboarding_path = $1, updated_at = now() WHERE user_id = $2",
        )
        .bind(path)
        .bind(user_id)
        .execute(&self.pool)
        .await?;
        if res.rows_affected() == 0 {
            return Err(anyhow!("no profile found for user {}", user_id));
        }
        Ok(())
    }

    pub async fn ensure_life_story(&self, user_id: Uuid) -> Result<()> {
        sqlx::query(
            "INSERT INTO life_stories (id, user_id)
             VALUES ($1, $2)
             ON CONFLICT (user_id) DO NOTHING",
        )
        .bind(Uuid::new_v4())
        .bind(user_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn upsert_life_story(&self, story: &LifeStory) -> Result<Uuid> {
        let row: (Uuid,) = sqlx::query_as(
            "INSERT INTO life_stories (id, user_id, raw_input, input_method, ai_summary, extracted_context, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, now(), now())
             ON CONFLICT (user_id) DO UPDATE SET
               raw_input = EXCLUDED.raw_input,
               input_method = EXCLUDED.input_method,
               ai_summary = EXCLUDED.ai_summary,
               extracted_context = EXCLUDED.extracted_context,
               updated_at = now()
             RETURNING id",
        )
        .bind(story.id)
        .bind(story.user_id)
        .bind(&story.raw_input)
        .bind(&story.input_method)
        .bind(&story.ai_summary)
        .bind(&story.extracted_context)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0)
    }

    pub async fn get_life_story_by_user_id(&self, user_id: Uuid) -> Result<LifeStory> {
        let s = sqlx::query_as::<_, LifeStory>(
            "SELECT id, user_id, raw_input, input_method, ai_summary, COALESCE(extracted_context, 'null'::jsonb) AS extracted_context, created_at, updated_at
             FROM life_stories WHERE user_id = $1",
        )
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(s)
    }

    pub async fn bulk_insert_routines(&self, routines: &[Routine]) -> Result<()> {
        if routines.is_empty() {
            return Ok(());
        }
        let mut tx = self.pool.begin().await?;
        for rt in routines {
            sqlx::query(
                "INSERT INTO routines (id, user_id, period, activity, confirmed, sort_order, created_at)
                 VALUES ($1, $2, $3, $4, $5, $6, now())",
            )
            .bind(rt.id)
            .bind(rt.user_id)
            .bind(&rt.period)
            .bind(&rt.activity)
            .bind(rt.confirmed)
            .bind(rt.sort_order)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn update_routine_confirmation(&self, id: Uuid, confirmed: bool) -> Result<()> {
        let res = sqlx::query("UPDATE routines SET confirmed = $1 WHERE id = $2")
            .bind(confirmed)
            .bind(id)
            .execute(&self.pool)
            .await?;
        if res.rows_affected() == 0 {
            return Err(anyhow!("routine {} not found", id));
        }
        Ok(())
    }

    pub async fn get_routines_by_user_id(&self, user_id: Uuid) -> Result<Vec<Routine>> {
        let rows = sqlx::query_as::<_, Routine>(
            "SELECT id, user_id, period, activity, confirmed, sort_order, created_at
             FROM routines WHERE user_id = $1
             ORDER BY
               CASE period WHEN 'morning' THEN 1 WHEN 'afternoon' THEN 2 WHEN 'night' THEN 3 END,
               sort_order",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn delete_routines_by_user_id(&self, user_id: Uuid) -> Result<()> {
        sqlx::query("DELETE FROM routines WHERE user_id = $1")
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn insert_user_photo(&self, photo: &UserPhoto) -> Result<()> {
        let ptype = if photo.photo_type.is_empty() {
            photo_type::FACE
        } else {
            &photo.photo_type
        };
        sqlx::query(
            "INSERT INTO user_photos (id, user_id, storage_url, storage_path, mime_type,
                                       is_primary, photo_type, flux_storage_url, flux_storage_path, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, now())
             ON CONFLICT (user_id, photo_type) WHERE is_primary = true DO UPDATE SET
                 storage_url       = EXCLUDED.storage_url,
                 storage_path      = EXCLUDED.storage_path,
                 mime_type         = EXCLUDED.mime_type,
                 flux_storage_url  = EXCLUDED.flux_storage_url,
                 flux_storage_path = EXCLUDED.flux_storage_path",
        )
        .bind(photo.id)
        .bind(photo.user_id)
        .bind(&photo.storage_url)
        .bind(&photo.storage_path)
        .bind(&photo.mime_type)
        .bind(true)
        .bind(ptype)
        .bind(&photo.flux_storage_url)
        .bind(&photo.flux_storage_path)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_primary_photo_by_user_id(&self, user_id: Uuid) -> Result<UserPhoto> {
        let p = sqlx::query_as::<_, UserPhoto>(
            "SELECT id, user_id, storage_url, storage_path, mime_type, is_primary, photo_type, created_at, flux_storage_url, flux_storage_path
             FROM user_photos
             WHERE user_id = $1 AND is_primary = true
             ORDER BY CASE photo_type WHEN 'face' THEN 0 WHEN 'full_body' THEN 1 ELSE 2 END
             LIMIT 1",
        )
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(p)
    }

    pub async fn get_flux_photo_by_user_id(&self, user_id: Uuid) -> Result<UserPhoto> {
        let p = sqlx::query_as::<_, UserPhoto>(
            "SELECT id, user_id, storage_url, storage_path, mime_type, is_primary, photo_type, created_at, flux_storage_url, flux_storage_path
             FROM user_photos
             WHERE user_id = $1 AND is_primary = true
             ORDER BY CASE photo_type WHEN 'full_body' THEN 0 WHEN 'face' THEN 1 ELSE 2 END
             LIMIT 1",
        )
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(p)
    }

    pub async fn count_user_photos(&self, user_id: Uuid) -> Result<i64> {
        let row: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM user_photos WHERE user_id = $1 AND is_primary = true")
                .bind(user_id)
                .fetch_one(&self.pool)
                .await?;
        Ok(row.0)
    }

    pub async fn upsert_identity(
        &self,
        user_id: Uuid,
        age_bracket: &str,
        gender: &str,
    ) -> Result<()> {
        let gender_opt = if gender.is_empty() { None } else { Some(gender) };
        let res = sqlx::query(
            "UPDATE user_profiles
             SET age_bracket = $1,
                 gender      = COALESCE($2, gender),
                 updated_at  = now()
             WHERE user_id = $3",
        )
        .bind(age_bracket)
        .bind(gender_opt)
        .bind(user_id)
        .execute(&self.pool)
        .await?;
        if res.rows_affected() == 0 {
            return Err(anyhow!("no profile found for user {}", user_id));
        }
        Ok(())
    }

    pub async fn get_cinematic_context_status(&self, user_id: Uuid) -> Result<bool> {
        let row: (bool,) = sqlx::query_as(
            "SELECT cinematic_context_completed FROM user_profiles WHERE user_id = $1",
        )
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0)
    }

    pub async fn upsert_cinematic_context(
        &self,
        user_id: Uuid,
        input: &CinematicContextInput,
    ) -> Result<()> {
        let age_opt = if input.age_bracket.is_empty() {
            None
        } else {
            Some(&input.age_bracket)
        };
        let gender_opt = if input.gender.is_empty() {
            None
        } else {
            Some(&input.gender)
        };
        let res = sqlx::query(
            "UPDATE user_profiles
             SET age_bracket                 = COALESCE($1, age_bracket),
                 gender                      = COALESCE($2, gender),
                 relationship_status         = $3,
                 dependent_count             = $4,
                 living_situation            = $5,
                 industry                    = $6,
                 career_stage                = $7,
                 net_worth_bracket           = $8,
                 income_bracket              = $9,
                 cinematic_context_completed = true,
                 updated_at                  = now()
             WHERE user_id = $10",
        )
        .bind(age_opt)
        .bind(gender_opt)
        .bind(&input.relationship_status)
        .bind(input.dependent_count)
        .bind(&input.living_situation)
        .bind(&input.industry)
        .bind(&input.career_stage)
        .bind(&input.net_worth_bracket)
        .bind(&input.income_bracket)
        .bind(user_id)
        .execute(&self.pool)
        .await?;
        if res.rows_affected() == 0 {
            return Err(anyhow!("no profile found for user {}", user_id));
        }
        Ok(())
    }

    pub async fn update_extracted_context(&self, story_id: Uuid, raw: &JsonValue) -> Result<()> {
        let res = sqlx::query(
            "UPDATE life_stories SET extracted_context = $1, updated_at = now() WHERE id = $2",
        )
        .bind(raw)
        .bind(story_id)
        .execute(&self.pool)
        .await?;
        if res.rows_affected() == 0 {
            return Err(anyhow!("life story {} not found", story_id));
        }
        Ok(())
    }

    pub async fn apply_assumption_calibration(
        &self,
        user_id: Uuid,
        patch: &UserCalibrationProfilePatch,
        ai_summary: &str,
        extracted: Option<&JsonValue>,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        self.apply_assumption_calibration_tx(&mut tx, user_id, patch, ai_summary, extracted)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn apply_assumption_calibration_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        user_id: Uuid,
        patch: &UserCalibrationProfilePatch,
        ai_summary: &str,
        extracted: Option<&JsonValue>,
    ) -> Result<()> {
        if patch.has_updates() {
            let res = sqlx::query(
                "UPDATE user_profiles SET
                   estimated_net_worth = COALESCE($1, estimated_net_worth),
                   estimated_yearly_salary = COALESCE($2, estimated_yearly_salary),
                   risk_tolerance = COALESCE($3, risk_tolerance),
                   follow_through = COALESCE($4, follow_through),
                   optimism_bias = COALESCE($5, optimism_bias),
                   stress_response = COALESCE($6, stress_response),
                   decision_style = COALESCE($7, decision_style),
                   saving_habits = COALESCE($8, saving_habits),
                   debt_comfort = COALESCE($9, debt_comfort),
                   housing_stability = COALESCE($10, housing_stability),
                   income_stability = COALESCE($11, income_stability),
                   liquid_net_worth_source = COALESCE($12, liquid_net_worth_source),
                   relationship_status = COALESCE($13, relationship_status),
                   household_income_structure = COALESCE($14, household_income_structure),
                   dependent_count = COALESCE($15, dependent_count),
                   life_stability = COALESCE($16, life_stability),
                   updated_at = now()
                 WHERE user_id = $17",
            )
            .bind(patch.estimated_net_worth)
            .bind(patch.estimated_yearly_salary)
            .bind(&patch.risk_tolerance)
            .bind(&patch.follow_through)
            .bind(&patch.optimism_bias)
            .bind(&patch.stress_response)
            .bind(&patch.decision_style)
            .bind(&patch.saving_habits)
            .bind(&patch.debt_comfort)
            .bind(&patch.housing_stability)
            .bind(&patch.income_stability)
            .bind(&patch.liquid_net_worth_source)
            .bind(&patch.relationship_status)
            .bind(&patch.household_income_structure)
            .bind(patch.dependent_count)
            .bind(&patch.life_stability)
            .bind(user_id)
            .execute(&mut **tx)
            .await?;
            if res.rows_affected() == 0 {
                return Err(anyhow!("no profile found for user {}", user_id));
            }
        }

        let res = sqlx::query(
            "UPDATE life_stories SET
               ai_summary = CASE WHEN $1 <> '' THEN $1 ELSE ai_summary END,
               extracted_context = COALESCE($2, extracted_context),
               updated_at = now()
             WHERE user_id = $3",
        )
        .bind(ai_summary)
        .bind(extracted)
        .bind(user_id)
        .execute(&mut **tx)
        .await?;
        if res.rows_affected() == 0 {
            return Err(anyhow!("no life story found for user {}", user_id));
        }
        Ok(())
    }

    pub async fn build_life_state(&self, user_id: Uuid) -> Result<LifeState> {
        let mut ls = LifeState::default_state();
        let profile = self.get_profile_by_user_id(user_id).await?;
        ls.income = profile.estimated_yearly_salary;
        ls.net_worth = profile.estimated_net_worth;

        let story = self.get_life_story_by_user_id(user_id).await.ok();

        if let Some(story) = &story {
            if !story.extracted_context.is_null() {
                if let Some(ec) = story.extracted_context.as_object() {
                    merge_extracted_context(&mut ls, ec);
                }
            }
        }

        if let Some(rs) = profile.relationship_status.as_deref().filter(|s| !s.is_empty()) {
            if ls.relationship_status == "unknown" {
                ls.relationship_status = rs.to_string();
            }
        }
        if let Some(dc) = profile.dependent_count {
            if ls.has_dependents.is_none() {
                ls.dependent_count = dc;
                ls.has_dependents = Some(dc > 0);
            }
        }
        if let Some(rt) = profile.risk_tolerance.as_deref().filter(|s| !s.is_empty()) {
            if ls.risk_tolerance == score_level::UNKNOWN {
                ls.risk_tolerance = rt.to_string();
            }
        }
        if let Some(ab) = profile.age_bracket.as_deref().filter(|s| !s.is_empty()) {
            if ls.age_range.is_empty() {
                ls.age_range = ab.to_string();
            }
        }
        if let Some(g) = profile.gender.as_deref().filter(|s| !s.is_empty()) {
            if ls.gender.is_empty() {
                ls.gender = g.to_string();
                if ls.gender_source.is_empty() {
                    ls.gender_source = "explicit".into();
                }
            }
        }
        if let Some(i) = profile.industry.as_deref().filter(|s| !s.is_empty()) {
            if ls.industry.is_empty() {
                ls.industry = i.to_string();
            }
        }
        if let Some(cs) = profile.career_stage.as_deref().filter(|s| !s.is_empty()) {
            if ls.career_stage.is_empty() {
                ls.career_stage = cs.to_string();
            }
        }
        ls.compute_completeness();
        Ok(ls)
    }

    pub async fn set_suggested_first_decision(&self, user_id: Uuid, raw: &JsonValue) -> Result<()> {
        let res = sqlx::query(
            "UPDATE user_profiles
             SET suggested_first_decision = $1,
                 suggested_first_decision_generated_at = now(),
                 updated_at = now()
             WHERE user_id = $2",
        )
        .bind(raw)
        .bind(user_id)
        .execute(&self.pool)
        .await?;
        if res.rows_affected() == 0 {
            return Err(anyhow!("no profile found for user {}", user_id));
        }
        Ok(())
    }

    pub async fn get_suggested_first_decision(&self, user_id: Uuid) -> Result<JsonValue> {
        let row: (Option<JsonValue>,) =
            sqlx::query_as("SELECT suggested_first_decision FROM user_profiles WHERE user_id = $1")
                .bind(user_id)
                .fetch_one(&self.pool)
                .await?;
        Ok(row.0.unwrap_or(JsonValue::Null))
    }

    pub async fn set_suggested_first_what_if(
        &self,
        user_id: Uuid,
        raw: &JsonValue,
    ) -> Result<()> {
        let res = sqlx::query(
            "UPDATE user_profiles
             SET suggested_first_what_if = $1,
                 suggested_first_what_if_generated_at = now(),
                 updated_at = now()
             WHERE user_id = $2",
        )
        .bind(raw)
        .bind(user_id)
        .execute(&self.pool)
        .await?;
        if res.rows_affected() == 0 {
            return Err(anyhow!("no profile found for user {}", user_id));
        }
        Ok(())
    }

    pub async fn get_suggested_first_what_if(&self, user_id: Uuid) -> Result<JsonValue> {
        let row: (Option<JsonValue>,) =
            sqlx::query_as("SELECT suggested_first_what_if FROM user_profiles WHERE user_id = $1")
                .bind(user_id)
                .fetch_one(&self.pool)
                .await?;
        Ok(row.0.unwrap_or(JsonValue::Null))
    }

    // --- Character Plates ---

    pub async fn insert_character_plate(&self, plate: &CharacterPlate) -> Result<()> {
        sqlx::query(
            "INSERT INTO character_plates
               (id, user_id, source_photo_id, storage_bucket, storage_url, storage_path, mime_type, prompt_used, status,
                attempt_count, last_error, last_attempt_at, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, now(), now())",
        )
        .bind(plate.id)
        .bind(plate.user_id)
        .bind(plate.source_photo_id)
        .bind(&plate.storage_bucket)
        .bind(&plate.storage_url)
        .bind(&plate.storage_path)
        .bind(&plate.mime_type)
        .bind(&plate.prompt_used)
        .bind(&plate.status)
        .bind(plate.attempt_count)
        .bind(&plate.last_error)
        .bind(plate.last_attempt_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update_character_plate_status(
        &self,
        plate_id: Uuid,
        status: &str,
        storage_bucket: Option<&str>,
        storage_url: Option<&str>,
        storage_path: Option<&str>,
        mime_type: Option<&str>,
        last_error: Option<&str>,
    ) -> Result<()> {
        let res = sqlx::query(
            "UPDATE character_plates
             SET status = $1,
                 storage_bucket = $2,
                 storage_url = $3,
                 storage_path = $4,
                 mime_type = $5,
                 last_error = $6,
                 last_attempt_at = CASE
                     WHEN $1 IN ('generating', 'failed', 'ready') THEN now()
                     ELSE last_attempt_at
                 END,
                 updated_at = now()
             WHERE id = $7",
        )
        .bind(status)
        .bind(storage_bucket)
        .bind(storage_url)
        .bind(storage_path)
        .bind(mime_type)
        .bind(last_error)
        .bind(plate_id)
        .execute(&self.pool)
        .await?;
        if res.rows_affected() == 0 {
            return Err(anyhow!("character plate {} not found", plate_id));
        }
        Ok(())
    }

    pub async fn get_ready_character_plate_by_user_id(
        &self,
        user_id: Uuid,
    ) -> Result<CharacterPlate> {
        let cp = sqlx::query_as::<_, CharacterPlate>(
            "SELECT id, user_id, source_photo_id, storage_bucket, storage_url, storage_path, mime_type,
                    prompt_used, status, attempt_count, last_error, last_attempt_at, created_at, updated_at
             FROM character_plates
             WHERE user_id = $1 AND status = 'ready'
             ORDER BY created_at DESC
             LIMIT 1",
        )
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(cp)
    }

    pub async fn claim_character_plate_generation(
        &self,
        user_id: Uuid,
        source_photo_id: Uuid,
        prompt: &str,
    ) -> Result<(CharacterPlate, bool)> {
        let mut tx = self.pool.begin().await?;

        let existing = sqlx::query_as::<_, CharacterPlate>(
            "SELECT id, user_id, source_photo_id, storage_bucket, storage_url, storage_path, mime_type,
                    prompt_used, status, attempt_count, last_error, last_attempt_at, created_at, updated_at
             FROM character_plates
             WHERE user_id = $1 AND source_photo_id = $2
             FOR UPDATE",
        )
        .bind(user_id)
        .bind(source_photo_id)
        .fetch_optional(&mut *tx)
        .await?;

        let now = Utc::now();
        match existing {
            None => {
                let plate = CharacterPlate {
                    id: Uuid::new_v4(),
                    user_id,
                    source_photo_id,
                    storage_bucket: None,
                    storage_url: None,
                    storage_path: None,
                    mime_type: None,
                    prompt_used: prompt.to_string(),
                    status: character_plate_status::GENERATING.into(),
                    attempt_count: 1,
                    last_error: None,
                    last_attempt_at: Some(now),
                    created_at: now,
                    updated_at: now,
                };
                sqlx::query(
                    "INSERT INTO character_plates
                       (id, user_id, source_photo_id, prompt_used, status, attempt_count, last_attempt_at, created_at, updated_at)
                     VALUES ($1, $2, $3, $4, $5, $6, now(), now(), now())",
                )
                .bind(plate.id)
                .bind(plate.user_id)
                .bind(plate.source_photo_id)
                .bind(&plate.prompt_used)
                .bind(&plate.status)
                .bind(plate.attempt_count)
                .execute(&mut *tx)
                .await?;
                tx.commit().await?;
                Ok((plate, true))
            }
            Some(mut plate) => {
                match plate.status.as_str() {
                    "ready" => {
                        if plate.storage_bucket.is_none()
                            || plate.storage_path.is_none()
                            || plate.storage_url.is_none()
                            || plate.mime_type.is_none()
                        {
                            sqlx::query(
                                "UPDATE character_plates SET status = $1, prompt_used = $2, storage_bucket = NULL, storage_url = NULL, storage_path = NULL, mime_type = NULL, attempt_count = attempt_count + 1, last_error = NULL, last_attempt_at = now(), updated_at = now() WHERE id = $3",
                            )
                            .bind(character_plate_status::GENERATING)
                            .bind(prompt)
                            .bind(plate.id)
                            .execute(&mut *tx)
                            .await?;
                            plate.status = character_plate_status::GENERATING.into();
                            plate.prompt_used = prompt.to_string();
                            plate.attempt_count += 1;
                            plate.storage_bucket = None;
                            plate.storage_url = None;
                            plate.storage_path = None;
                            plate.mime_type = None;
                            plate.last_error = None;
                            tx.commit().await?;
                            return Ok((plate, true));
                        }
                        tx.commit().await?;
                        Ok((plate, false))
                    }
                    "pending" | "generating" => {
                        tx.commit().await?;
                        Ok((plate, false))
                    }
                    "failed" => {
                        sqlx::query(
                            "UPDATE character_plates SET status = $1, prompt_used = $2, attempt_count = attempt_count + 1, last_error = NULL, last_attempt_at = now(), updated_at = now() WHERE id = $3",
                        )
                        .bind(character_plate_status::GENERATING)
                        .bind(prompt)
                        .bind(plate.id)
                        .execute(&mut *tx)
                        .await?;
                        plate.status = character_plate_status::GENERATING.into();
                        plate.prompt_used = prompt.to_string();
                        plate.attempt_count += 1;
                        plate.last_error = None;
                        tx.commit().await?;
                        Ok((plate, true))
                    }
                    _ => {
                        tx.commit().await?;
                        Ok((plate, false))
                    }
                }
            }
        }
    }

    pub async fn save_life_context(
        &self,
        user_id: Uuid,
        relationship: &str,
        household: &str,
        dependents: i32,
        stability: &str,
    ) -> Result<()> {
        let res = sqlx::query(
            "UPDATE user_profiles SET
               relationship_status = $1,
               household_income_structure = $2,
               dependent_count = $3,
               life_stability = $4,
               onboarding_status = 'life_context_completed',
               updated_at = now()
             WHERE user_id = $5",
        )
        .bind(relationship)
        .bind(household)
        .bind(dependents)
        .bind(stability)
        .bind(user_id)
        .execute(&self.pool)
        .await?;
        if res.rows_affected() == 0 {
            return Err(anyhow!("no profile found for user {}", user_id));
        }
        Ok(())
    }
}

fn merge_extracted_context(ls: &mut LifeState, ec: &serde_json::Map<String, JsonValue>) {
    let s = |v: &JsonValue| v.as_str().map(|s| s.to_string()).filter(|s| !s.is_empty());
    let f = |v: &JsonValue| v.as_f64();
    let b = |v: &JsonValue| v.as_bool();

    if let Some(v) = ec.get("age").and_then(f) {
        if v > 0.0 {
            ls.age = v as i32;
        }
    }
    if let Some(v) = ec.get("location").and_then(s) {
        ls.location = v;
    }
    if let Some(v) = ec.get("gender").and_then(s) {
        ls.gender = v;
    }
    if let Some(v) = ec.get("education_level").and_then(s) {
        ls.education_level = v;
    }
    if let Some(v) = ec.get("industry").and_then(s) {
        ls.industry = v;
    }
    if let Some(v) = ec.get("role").and_then(s) {
        ls.role = v;
    }
    if let Some(v) = ec.get("profession").and_then(s) {
        ls.profession = v;
    }
    if let Some(v) = ec.get("career_experience_yr").and_then(f) {
        if v > 0.0 {
            ls.career_experience_yr = v as i32;
        }
    }
    if let Some(v) = ec.get("debt").and_then(f) {
        ls.debt = v;
    }
    if let Some(v) = ec.get("risk_tolerance").and_then(s) {
        ls.risk_tolerance = v;
    }
    if let Some(v) = ec.get("health_score").and_then(f) {
        if v > 0.0 {
            ls.health_score = v;
            ls.health_provided = true;
        }
    }
    if let Some(v) = ec.get("network_strength").and_then(s) {
        ls.network_strength = v;
    }
    if let Some(v) = ec.get("ambition").and_then(s) {
        ls.ambition = v;
    }
    if let Some(v) = ec.get("stress_level").and_then(s) {
        ls.stress_level = v;
    }
    if let Some(v) = ec.get("relationship_status").and_then(s) {
        ls.relationship_status = v;
    }
    if let Some(v) = ec.get("has_dependents").and_then(b) {
        ls.has_dependents = Some(v);
    }
    if let Some(v) = ec.get("dependent_count").and_then(f) {
        ls.dependent_count = v as i32;
    }
    if let Some(arr) = ec.get("goals").and_then(|v| v.as_array()) {
        let goals: Vec<String> = arr
            .iter()
            .filter_map(|i| i.as_str().map(|s| s.to_string()))
            .collect();
        if !goals.is_empty() {
            ls.goals = goals;
        }
    }
    if let Some(v) = ec.get("geographic_mobility").and_then(s) {
        ls.geographic_mobility = v;
    }
    if let Some(v) = ec.get("age_range").and_then(s) {
        ls.age_range = v;
    }
    if let Some(v) = ec.get("age_source").and_then(s) {
        ls.age_source = v;
    }
    if let Some(v) = ec.get("career_stage").and_then(s) {
        ls.career_stage = v;
    }
    if let Some(v) = ec.get("career_experience_source").and_then(s) {
        ls.career_experience_source = v;
    }
    if let Some(v) = ec.get("gender_source").and_then(s) {
        ls.gender_source = v;
    }
    if let Some(v) = ec.get("monthly_spending").and_then(f) {
        if v > 0.0 {
            ls.monthly_spending = v;
        }
    }
    if let Some(v) = ec.get("monthly_savings").and_then(f) {
        if v > 0.0 {
            ls.monthly_savings = v;
        }
    }
    if let Some(v) = ec.get("housing_cost").and_then(f) {
        if v > 0.0 {
            ls.housing_cost = v;
        }
    }
}

// needed for extracted_context updates
pub type _ChronoTs = DateTime<Utc>;
