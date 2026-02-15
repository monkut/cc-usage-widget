use chrono::Utc;
use reqwest::header::{HeaderMap, HeaderValue};
use serde::Deserialize;
use std::collections::HashMap;

use crate::usage::{
    build_active_sessions, collect_daily_activity, collect_jsonl_files, compute_weekly_usage,
    count_user_prompts_in_window, count_weighted_usage_in_window, get_claude_data_dirs,
    get_model_display_name, parse_usage_from_file, ActiveSession, DailyActivity, ModelUsage,
    QuotaInfo, TokenUsage, UsageStats, WeeklyUsage,
};

const BASE_URL: &str = "https://api.anthropic.com";

// --- Usage Report types ---

#[derive(Debug, Deserialize)]
pub struct CacheCreation {
    #[serde(default)]
    pub ephemeral_1h_input_tokens: u64,
    #[serde(default)]
    pub ephemeral_5m_input_tokens: u64,
}

#[derive(Debug, Deserialize)]
pub struct UsageResult {
    pub model: Option<String>,
    #[serde(default)]
    pub uncached_input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub cache_read_input_tokens: u64,
    #[serde(default)]
    pub cache_creation: Option<CacheCreation>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct UsageBucket {
    pub starting_at: String,
    pub ending_at: String,
    pub results: Vec<UsageResult>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct UsageReportResponse {
    pub data: Vec<UsageBucket>,
    pub has_more: bool,
    pub next_page: Option<String>,
}

// --- Cost Report types ---

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct CostResult {
    pub amount: Option<String>,
    pub currency: Option<String>,
    pub model: Option<String>,
    pub cost_type: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct CostBucket {
    pub starting_at: String,
    pub ending_at: String,
    pub results: Vec<CostResult>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct CostReportResponse {
    pub data: Vec<CostBucket>,
    pub has_more: bool,
    pub next_page: Option<String>,
}

pub struct AdminApiClient {
    client: reqwest::Client,
}

impl AdminApiClient {
    pub fn new(api_key: &str) -> Result<Self, String> {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-api-key",
            HeaderValue::from_str(api_key).map_err(|e| format!("Invalid API key: {e}"))?,
        );
        headers.insert(
            "anthropic-version",
            HeaderValue::from_static("2023-06-01"),
        );

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .map_err(|e| format!("Failed to build HTTP client: {e}"))?;

        Ok(Self { client })
    }

    pub async fn fetch_usage_report(
        &self,
        starting_at: &str,
        ending_at: Option<&str>,
        bucket_width: &str,
        group_by: &[&str],
    ) -> Result<UsageReportResponse, String> {
        let mut url = format!(
            "{BASE_URL}/v1/organizations/usage_report/messages?starting_at={starting_at}&bucket_width={bucket_width}"
        );
        if let Some(end) = ending_at {
            url.push_str(&format!("&ending_at={end}"));
        }
        for g in group_by {
            url.push_str(&format!("&group_by[]={g}"));
        }

        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("Usage report request failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Usage report API error {status}: {body}"));
        }

        resp.json::<UsageReportResponse>()
            .await
            .map_err(|e| format!("Failed to parse usage report: {e}"))
    }

    pub async fn fetch_cost_report(
        &self,
        starting_at: &str,
        ending_at: Option<&str>,
    ) -> Result<CostReportResponse, String> {
        let mut url = format!(
            "{BASE_URL}/v1/organizations/cost_report?starting_at={starting_at}&bucket_width=1d&group_by[]=description"
        );
        if let Some(end) = ending_at {
            url.push_str(&format!("&ending_at={end}"));
        }

        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("Cost report request failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Cost report API error {status}: {body}"));
        }

        resp.json::<CostReportResponse>()
            .await
            .map_err(|e| format!("Failed to parse cost report: {e}"))
    }

    /// Validate the API key by making a minimal usage report request
    pub async fn validate(&self) -> Result<(), String> {
        let now = Utc::now();
        let starting_at = (now - chrono::Duration::hours(1))
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string();

        let url = format!(
            "{BASE_URL}/v1/organizations/usage_report/messages?starting_at={starting_at}&bucket_width=1h&limit=1"
        );

        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("Validation request failed: {e}"))?;

        if resp.status().is_success() {
            Ok(())
        } else {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            Err(format!("API key validation failed ({status}): {body}"))
        }
    }
}

/// Supplemental data from local JSONL files (sessions, quota, activity)
struct LocalSupplementalData {
    active_sessions: Vec<ActiveSession>,
    quota: QuotaInfo,
    daily_activity: Vec<DailyActivity>,
    weekly_usage: WeeklyUsage,
    last_updated: String,
}

/// Gather local JSONL data for sessions, quota calculations, and activity heatmap
fn get_local_supplemental_data() -> LocalSupplementalData {
    let data_dirs = get_claude_data_dirs();

    // Sessions from last 25 hours of files
    let session_files = collect_jsonl_files(&data_dirs, Some(25));
    let mut session_entries = Vec::new();
    for file in &session_files {
        if let Ok(entries) = parse_usage_from_file(file) {
            session_entries.extend(entries);
        }
    }

    // Find latest timestamp from session entries
    let last_updated = session_entries
        .iter()
        .map(|e| e.timestamp.as_str())
        .max()
        .unwrap_or("")
        .to_string();

    let active_sessions = build_active_sessions(session_entries);

    // Quota: 5hr rolling window
    let five_hr_files = collect_jsonl_files(&data_dirs, Some(6));
    let quota_window_prompts = count_user_prompts_in_window(&five_hr_files, 5);
    let quota_window_weighted = count_weighted_usage_in_window(&five_hr_files, 5);

    // Weekly quota
    let week_files = collect_jsonl_files(&data_dirs, Some(24 * 8));
    let week_weighted = count_weighted_usage_in_window(&week_files, 24 * 7);

    let estimated_limit: u32 = 500;
    let usage_percent = (quota_window_weighted / estimated_limit as f64 * 100.0).min(100.0);
    let week_estimated_prompts: u32 = 2590;
    let week_usage_percent = (week_weighted / week_estimated_prompts as f64 * 100.0).min(100.0);

    let quota = QuotaInfo {
        messages_in_window: quota_window_prompts,
        window_hours: 5,
        estimated_limit,
        usage_percent,
        plan: "Max 5x".to_string(),
        week_usage_percent,
        week_limit_hours: 210,
    };

    // Daily activity heatmap
    let activity_files = collect_jsonl_files(&data_dirs, Some(24 * 85));
    let daily_activity = collect_daily_activity(&activity_files);
    let weekly_usage = compute_weekly_usage(&daily_activity);

    LocalSupplementalData {
        active_sessions,
        quota,
        daily_activity,
        weekly_usage,
        last_updated,
    }
}

/// Build UsageStats by combining API token/cost data with local session/quota data
pub async fn build_usage_stats_from_api(client: &AdminApiClient) -> Result<UsageStats, String> {
    let now = Utc::now();
    let today_start = now
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc()
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    let ending_at = now.format("%Y-%m-%dT%H:%M:%SZ").to_string();

    // Fetch usage grouped by model for today
    let usage_report = client
        .fetch_usage_report(&today_start, Some(&ending_at), "1d", &["model"])
        .await?;

    // Fetch cost report for today
    let cost_report = client
        .fetch_cost_report(&today_start, Some(&ending_at))
        .await?;

    // Aggregate usage by model from API data
    let mut model_tokens: HashMap<String, TokenUsage> = HashMap::new();
    for bucket in &usage_report.data {
        for result in &bucket.results {
            let model_name = result.model.as_deref().unwrap_or("unknown").to_string();
            let entry = model_tokens.entry(model_name).or_default();
            entry.input_tokens += result.uncached_input_tokens;
            entry.output_tokens += result.output_tokens;
            entry.cache_read_input_tokens += result.cache_read_input_tokens;
            if let Some(ref cache) = result.cache_creation {
                entry.cache_creation_input_tokens +=
                    cache.ephemeral_5m_input_tokens + cache.ephemeral_1h_input_tokens;
            }
        }
    }

    // Aggregate cost by model from API data
    let mut model_costs: HashMap<String, f64> = HashMap::new();
    for bucket in &cost_report.data {
        for result in &bucket.results {
            if let (Some(ref model), Some(ref amount_str)) = (&result.model, &result.amount) {
                // Amount is in cents as decimal string
                if let Ok(cents) = amount_str.parse::<f64>() {
                    *model_costs.entry(model.clone()).or_default() += cents / 100.0;
                }
            }
        }
    }

    // Build model usage list
    let mut total = TokenUsage::default();
    let mut total_cost: f64 = 0.0;

    let mut by_model: Vec<ModelUsage> = model_tokens
        .into_iter()
        .map(|(model, tokens)| {
            total.input_tokens += tokens.input_tokens;
            total.output_tokens += tokens.output_tokens;
            total.cache_creation_input_tokens += tokens.cache_creation_input_tokens;
            total.cache_read_input_tokens += tokens.cache_read_input_tokens;

            let cost = model_costs.get(&model).copied().unwrap_or(0.0);
            total_cost += cost;
            let display_name = get_model_display_name(&model);

            ModelUsage {
                model,
                display_name,
                tokens,
                cost_usd: cost,
            }
        })
        .collect();

    by_model.sort_by(|a, b| {
        let a_total = a.tokens.input_tokens + a.tokens.output_tokens;
        let b_total = b.tokens.input_tokens + b.tokens.output_tokens;
        b_total.cmp(&a_total)
    });

    // Get supplemental data from local JSONL (sessions, quota, activity)
    let local = tokio::task::spawn_blocking(get_local_supplemental_data)
        .await
        .map_err(|e| format!("Failed to get local data: {e}"))?;

    let session_count = by_model
        .iter()
        .map(|m| {
            (m.tokens.input_tokens + m.tokens.output_tokens + m.tokens.cache_creation_input_tokens + m.tokens.cache_read_input_tokens) as u32
        })
        .sum::<u32>();

    Ok(UsageStats {
        total_tokens: total,
        total_cost_usd: total_cost,
        by_model,
        session_count,
        last_updated: local.last_updated,
        quota: local.quota,
        active_sessions: local.active_sessions,
        daily_activity: local.daily_activity,
        weekly_usage: local.weekly_usage,
    })
}
