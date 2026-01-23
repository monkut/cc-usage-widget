use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_input_tokens: u64,
    pub cache_read_input_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelUsage {
    pub model: String,
    pub display_name: String,
    pub tokens: TokenUsage,
    pub cost_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaInfo {
    pub messages_in_window: u32,
    pub window_hours: u32,
    pub estimated_limit: u32,
    pub usage_percent: f64,
    pub plan: String,
    pub week_usage_percent: f64,
    pub week_limit_hours: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveSession {
    pub session_id: String,
    pub project: String,
    pub directory: String,
    pub first_activity: String,
    pub last_activity: String,
    pub duration_minutes: u32,
    pub message_count: u32,
    pub total_tokens: u64,
    pub cost_usd: f64,
    pub model: String,
    pub model_display_name: String,
    pub context_remaining_percent: f64,
    pub todo_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyActivity {
    pub date: String,      // YYYY-MM-DD format
    pub prompt_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageStats {
    pub total_tokens: TokenUsage,
    pub total_cost_usd: f64,
    pub by_model: Vec<ModelUsage>,
    pub session_count: u32,
    pub last_updated: String,
    pub quota: QuotaInfo,
    pub active_sessions: Vec<ActiveSession>,
    pub daily_activity: Vec<DailyActivity>,
}

#[derive(Debug, Deserialize)]
struct MessageUsage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    cache_creation_input_tokens: Option<u64>,
    cache_read_input_tokens: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct Message {
    model: Option<String>,
    usage: Option<MessageUsage>,
}

#[derive(Debug, Deserialize)]
struct JournalEntry {
    #[serde(rename = "type")]
    entry_type: Option<String>,
    message: Option<Message>,
    timestamp: Option<String>,
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
    cwd: Option<String>,
}

/// Parse a JSON line and return timestamp if it's an actual user prompt (not just tool results)
/// Returns None if not a user prompt or parsing fails
fn parse_user_prompt_timestamp(line: &str) -> Option<String> {
    let entry: serde_json::Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => return None,
    };

    // Must be a "user" type entry
    if entry.get("type").and_then(|t| t.as_str()) != Some("user") {
        return None;
    }

    // Get the message content
    let content = match entry.get("message").and_then(|m| m.get("content")) {
        Some(c) => c,
        None => return None,
    };

    // Check if this is an actual user prompt (has text content)
    let is_real_prompt = if content.is_string() {
        true
    } else if let Some(arr) = content.as_array() {
        // Check for any text block (not just tool_result)
        arr.iter().any(|block| {
            block.get("type").and_then(|t| t.as_str()) == Some("text")
        })
    } else {
        false
    };

    if !is_real_prompt {
        return None;
    }

    // Return the timestamp if present
    entry.get("timestamp").and_then(|t| t.as_str()).map(|s| s.to_string())
}

fn get_model_display_name(model: &str) -> String {
    // Extract meaningful parts from model ID like "claude-opus-4-5-20251101"
    if model.contains("opus-4-5") || model.contains("opus-4.5") {
        "Opus 4.5".to_string()
    } else if model.contains("opus-4") {
        "Opus 4".to_string()
    } else if model.contains("opus") {
        "Opus".to_string()
    } else if model.contains("sonnet-4") {
        "Sonnet 4".to_string()
    } else if model.contains("sonnet-3-5") || model.contains("sonnet-3.5") {
        "Sonnet 3.5".to_string()
    } else if model.contains("sonnet") {
        "Sonnet".to_string()
    } else if model.contains("haiku-3-5") || model.contains("haiku-3.5") {
        "Haiku 3.5".to_string()
    } else if model.contains("haiku") {
        "Haiku".to_string()
    } else {
        model.to_string()
    }
}

// Pricing per million tokens (as of 2025)
fn get_model_pricing(model: &str) -> (f64, f64, f64, f64) {
    // (input, output, cache_write, cache_read) per million tokens
    match model {
        m if m.contains("opus") => (15.0, 75.0, 18.75, 1.50),
        m if m.contains("sonnet") => (3.0, 15.0, 3.75, 0.30),
        m if m.contains("haiku") => (0.25, 1.25, 0.30, 0.03),
        _ => (3.0, 15.0, 3.75, 0.30), // default to sonnet pricing
    }
}

fn calculate_cost(model: &str, tokens: &TokenUsage) -> f64 {
    let (input_price, output_price, cache_write_price, cache_read_price) = get_model_pricing(model);
    let million = 1_000_000.0;

    (tokens.input_tokens as f64 / million * input_price)
        + (tokens.output_tokens as f64 / million * output_price)
        + (tokens.cache_creation_input_tokens as f64 / million * cache_write_price)
        + (tokens.cache_read_input_tokens as f64 / million * cache_read_price)
}

/// Get context window size for a model (in tokens)
fn get_model_context_limit(_model: &str) -> u64 {
    // All Claude 3.5/4 models have 200K context windows
    200_000
}

/// Calculate context remaining percentage
fn calculate_context_remaining(total_tokens: u64, model: &str) -> f64 {
    let limit = get_model_context_limit(model);
    let used_percent = (total_tokens as f64 / limit as f64) * 100.0;
    (100.0 - used_percent).max(0.0)
}

#[derive(Debug, Deserialize)]
struct TodoItem {
    status: Option<String>,
}

/// Read todo file and count pending todos for a session
fn get_pending_todo_count(session_id: &str) -> u32 {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return 0,
    };

    // Check both possible locations
    let paths = [
        home.join(".claude").join("todos"),
        home.join(".config").join("claude").join("todos"),
    ];

    for todos_dir in &paths {
        if !todos_dir.exists() {
            continue;
        }

        // Look for files matching the session ID pattern
        if let Ok(entries) = std::fs::read_dir(todos_dir) {
            for entry in entries.flatten() {
                let filename = entry.file_name().to_string_lossy().to_string();
                if filename.starts_with(session_id) && filename.ends_with(".json") {
                    if let Ok(content) = std::fs::read_to_string(entry.path()) {
                        if let Ok(todos) = serde_json::from_str::<Vec<TodoItem>>(&content) {
                            return todos
                                .iter()
                                .filter(|t| {
                                    t.status.as_deref() != Some("completed")
                                })
                                .count() as u32;
                        }
                    }
                }
            }
        }
    }

    0
}

pub fn get_claude_data_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    if let Some(home) = dirs::home_dir() {
        // Current default location
        let config_claude = home.join(".config").join("claude").join("projects");
        if config_claude.exists() {
            dirs.push(config_claude);
        }

        // Legacy location
        let dot_claude = home.join(".claude").join("projects");
        if dot_claude.exists() {
            dirs.push(dot_claude);
        }
    }

    dirs
}

/// Collect JSONL files, optionally filtering by modification time
/// If max_age_hours is None, returns all files; otherwise only files modified within that window
pub fn collect_jsonl_files(data_dirs: &[PathBuf], max_age_hours: Option<i64>) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let cutoff = max_age_hours.map(|hours| {
        std::time::SystemTime::now() - std::time::Duration::from_secs((hours * 3600) as u64)
    });

    for dir in data_dirs {
        if let Ok(entries) = glob::glob(&format!("{}/**/*.jsonl", dir.display())) {
            for entry in entries.flatten() {
                // If we have a cutoff, filter by modification time
                if let Some(cutoff_time) = cutoff {
                    if let Ok(metadata) = entry.metadata() {
                        if let Ok(modified) = metadata.modified() {
                            if modified < cutoff_time {
                                continue; // Skip files older than cutoff
                            }
                        }
                    }
                }
                files.push(entry);
            }
        }
    }

    files
}

#[derive(Debug)]
pub struct ParsedEntry {
    pub model: String,
    pub tokens: TokenUsage,
    pub timestamp: String,
    pub session_id: String,
    pub cwd: String,
}

pub fn parse_usage_from_file(path: &PathBuf) -> Result<Vec<ParsedEntry>, String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    let reader = BufReader::new(file);
    let mut usages = Vec::new();

    // Track the cwd from the most recent entry (for entries that don't have cwd)
    let mut last_cwd = String::new();

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };

        if line.trim().is_empty() {
            continue;
        }

        let entry: JournalEntry = match serde_json::from_str(&line) {
            Ok(e) => e,
            Err(_) => continue,
        };

        // Update last_cwd if this entry has a cwd
        if let Some(ref cwd) = entry.cwd {
            last_cwd = cwd.clone();
        }

        // Only process assistant messages with usage data
        if entry.entry_type.as_deref() != Some("assistant") {
            continue;
        }

        if let Some(message) = entry.message {
            if let (Some(model), Some(usage)) = (message.model, message.usage) {
                let timestamp = entry.timestamp.unwrap_or_default();
                let session_id = entry.session_id.unwrap_or_default();
                let cwd = entry.cwd.unwrap_or_else(|| last_cwd.clone());
                let tokens = TokenUsage {
                    input_tokens: usage.input_tokens.unwrap_or(0),
                    output_tokens: usage.output_tokens.unwrap_or(0),
                    cache_creation_input_tokens: usage.cache_creation_input_tokens.unwrap_or(0),
                    cache_read_input_tokens: usage.cache_read_input_tokens.unwrap_or(0),
                };
                usages.push(ParsedEntry {
                    model,
                    tokens,
                    timestamp,
                    session_id,
                    cwd,
                });
            }
        }
    }

    Ok(usages)
}

pub fn aggregate_usage(
    entries: Vec<ParsedEntry>,
    since: Option<DateTime<Utc>>,
    quota_window_prompts: u32,
    week_prompts: u32,
    daily_activity: Vec<DailyActivity>,
) -> UsageStats {
    let mut by_model: HashMap<String, TokenUsage> = HashMap::new();
    let mut total = TokenUsage::default();
    let mut latest_timestamp = String::new();
    let mut message_count: u32 = 0;

    // Track active sessions (last 24 hours)
    // session_id -> (cwd, first_activity, last_activity, count, total_tokens, cost, last_model, current_context_tokens)
    let day_ago = Utc::now() - chrono::Duration::hours(24);
    let mut session_data: HashMap<String, (String, String, String, u32, u64, f64, String, u64)> = HashMap::new();

    for entry in entries {
        // Track sessions active in last 24 hours
        if let Ok(ts) = DateTime::parse_from_rfc3339(&entry.timestamp) {
            if ts >= day_ago && !entry.session_id.is_empty() {
                let entry_tokens = entry.tokens.input_tokens
                    + entry.tokens.output_tokens
                    + entry.tokens.cache_creation_input_tokens
                    + entry.tokens.cache_read_input_tokens;
                let entry_cost = calculate_cost(&entry.model, &entry.tokens);
                // Current context = cached context + new tokens being added
                let context_tokens = entry.tokens.cache_read_input_tokens
                    + entry.tokens.cache_creation_input_tokens
                    + entry.tokens.input_tokens;

                let session = session_data
                    .entry(entry.session_id.clone())
                    .or_insert((
                        entry.cwd.clone(),
                        entry.timestamp.clone(),
                        entry.timestamp.clone(),
                        0,
                        0,
                        0.0,
                        entry.model.clone(),
                        context_tokens,
                    ));
                // Update first_activity if earlier
                if entry.timestamp < session.1 {
                    session.1 = entry.timestamp.clone();
                }
                // Update last_activity, model, and current context if later
                if entry.timestamp > session.2 {
                    session.2 = entry.timestamp.clone();
                    session.6 = entry.model.clone(); // update to most recent model
                    session.7 = context_tokens; // update current context from most recent message
                }
                session.3 += 1; // message count
                session.4 += entry_tokens; // total tokens
                session.5 += entry_cost; // cost
            }
        }

        // Filter by date if specified for totals
        if let Some(since_dt) = since {
            if let Ok(ts) = DateTime::parse_from_rfc3339(&entry.timestamp) {
                if ts < since_dt {
                    continue;
                }
            }
        }

        message_count += 1;

        if entry.timestamp > latest_timestamp {
            latest_timestamp = entry.timestamp.clone();
        }

        // Aggregate by model
        let model_entry = by_model.entry(entry.model).or_default();
        model_entry.input_tokens += entry.tokens.input_tokens;
        model_entry.output_tokens += entry.tokens.output_tokens;
        model_entry.cache_creation_input_tokens += entry.tokens.cache_creation_input_tokens;
        model_entry.cache_read_input_tokens += entry.tokens.cache_read_input_tokens;

        // Total
        total.input_tokens += entry.tokens.input_tokens;
        total.output_tokens += entry.tokens.output_tokens;
        total.cache_creation_input_tokens += entry.tokens.cache_creation_input_tokens;
        total.cache_read_input_tokens += entry.tokens.cache_read_input_tokens;
    }

    let mut model_usages: Vec<ModelUsage> = by_model
        .into_iter()
        .map(|(model, tokens)| {
            let cost = calculate_cost(&model, &tokens);
            let display_name = get_model_display_name(&model);
            ModelUsage {
                model,
                display_name,
                tokens,
                cost_usd: cost,
            }
        })
        .collect();

    // Sort by total tokens (highest first)
    model_usages.sort_by(|a, b| {
        let a_total = a.tokens.input_tokens + a.tokens.output_tokens;
        let b_total = b.tokens.input_tokens + b.tokens.output_tokens;
        b_total.cmp(&a_total)
    });

    let total_cost: f64 = model_usages.iter().map(|m| m.cost_usd).sum();

    // Estimate quota - Max 5x plan defaults (based on Anthropic docs: 50-200 prompts/5hr)
    // Using 125 as midpoint estimate
    let estimated_limit: u32 = 125;
    let usage_percent = (quota_window_prompts as f64 / estimated_limit as f64 * 100.0).min(100.0);

    // Weekly limit estimation (assume ~210 hours/week for Max 5x based on 140-280 range)
    let week_limit_hours: u32 = 210;
    let week_estimated_prompts: u32 = 125 * 7 * 24 / 5; // ~4200 prompts/week
    let week_usage_percent = (week_prompts as f64 / week_estimated_prompts as f64 * 100.0).min(100.0);

    let quota = QuotaInfo {
        messages_in_window: quota_window_prompts,
        window_hours: 5,
        estimated_limit,
        usage_percent,
        plan: "Max 5x".to_string(),
        week_usage_percent,
        week_limit_hours,
    };

    // Build active sessions list
    let mut active_sessions: Vec<ActiveSession> = session_data
        .into_iter()
        .map(|(session_id, (cwd, first_activity, last_activity, msg_count, total_tokens, cost, model, current_context_tokens))| {
            // Use cwd directly as directory (it's the actual working directory from JSONL)
            let directory = cwd.clone();

            // Shorten for display - get last path component
            let short_project = directory
                .split('/')
                .last()
                .unwrap_or(&cwd)
                .to_string();

            // Calculate duration in minutes
            let duration_minutes = if let (Ok(first), Ok(last)) = (
                DateTime::parse_from_rfc3339(&first_activity),
                DateTime::parse_from_rfc3339(&last_activity),
            ) {
                ((last - first).num_minutes().max(0)) as u32
            } else {
                0
            };

            let model_display_name = get_model_display_name(&model);
            // Use current context tokens (from most recent message) for context remaining calculation
            let context_remaining_percent = calculate_context_remaining(current_context_tokens, &model);
            let todo_count = get_pending_todo_count(&session_id);

            ActiveSession {
                session_id: session_id.chars().take(8).collect(),
                project: short_project,
                directory,
                first_activity,
                last_activity,
                duration_minutes,
                message_count: msg_count,
                total_tokens,
                cost_usd: cost,
                model,
                model_display_name,
                context_remaining_percent,
                todo_count,
            }
        })
        .collect();

    // Sort by last activity (most recent first)
    active_sessions.sort_by(|a, b| b.last_activity.cmp(&a.last_activity));

    UsageStats {
        total_tokens: total,
        total_cost_usd: total_cost,
        by_model: model_usages,
        session_count: message_count,
        last_updated: latest_timestamp,
        quota,
        active_sessions,
        daily_activity,
    }
}

/// Count actual user prompts (excluding tool_result-only messages) in a time window
fn count_user_prompts_in_window(files: &[PathBuf], hours: i64) -> u32 {
    let window_start = Utc::now() - chrono::Duration::hours(hours);
    let mut count: u32 = 0;

    for path in files {
        let file = match File::open(path) {
            Ok(f) => f,
            Err(_) => continue,
        };
        let reader = BufReader::new(file);

        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => continue,
            };

            if line.trim().is_empty() {
                continue;
            }

            // Parse once: get timestamp only if this is an actual user prompt
            if let Some(ts_str) = parse_user_prompt_timestamp(&line) {
                if let Ok(ts) = DateTime::parse_from_rfc3339(&ts_str) {
                    if ts >= window_start {
                        count += 1;
                    }
                }
            }
        }
    }

    count
}

/// Collect daily user prompt counts for the last 12 weeks (84 days)
fn collect_daily_activity(files: &[PathBuf]) -> Vec<DailyActivity> {
    let mut daily_counts: HashMap<String, u32> = HashMap::new();
    let twelve_weeks_ago = Utc::now() - chrono::Duration::days(84);

    for path in files {
        let file = match File::open(path) {
            Ok(f) => f,
            Err(_) => continue,
        };
        let reader = BufReader::new(file);

        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => continue,
            };

            if line.trim().is_empty() {
                continue;
            }

            // Parse once: get timestamp only if this is an actual user prompt
            if let Some(ts_str) = parse_user_prompt_timestamp(&line) {
                if let Ok(ts) = DateTime::parse_from_rfc3339(&ts_str) {
                    if ts >= twelve_weeks_ago {
                        let date = ts.format("%Y-%m-%d").to_string();
                        *daily_counts.entry(date).or_insert(0) += 1;
                    }
                }
            }
        }
    }

    // Convert to sorted vec of DailyActivity
    let mut activities: Vec<DailyActivity> = daily_counts
        .into_iter()
        .map(|(date, prompt_count)| DailyActivity { date, prompt_count })
        .collect();

    activities.sort_by(|a, b| a.date.cmp(&b.date));
    activities
}

pub fn get_current_usage(period: &str) -> Result<UsageStats, String> {
    let data_dirs = get_claude_data_dirs();
    if data_dirs.is_empty() {
        return Err("No Claude data directories found".to_string());
    }

    // Determine file age filter based on period (add buffer for safety)
    let period_hours = match period {
        "today" => Some(25),      // 24hr + 1hr buffer
        "week" => Some(24 * 8),   // 7 days + 1 day buffer
        "month" => Some(24 * 32), // 30 days + 2 days buffer
        _ => None,                // "all" - no filter
    };

    // Collect files filtered by modification time for token usage
    let usage_files = collect_jsonl_files(&data_dirs, period_hours);
    let mut all_entries = Vec::new();

    for file in &usage_files {
        if let Ok(entries) = parse_usage_from_file(file) {
            all_entries.extend(entries);
        }
    }

    // Use separate filtered file lists for quota calculations
    // 5hr window: files modified in last 6 hours
    let five_hr_files = collect_jsonl_files(&data_dirs, Some(6));
    let quota_window_prompts = count_user_prompts_in_window(&five_hr_files, 5);

    // Week window: files modified in last 8 days
    let week_files = collect_jsonl_files(&data_dirs, Some(24 * 8));
    let week_prompts = count_user_prompts_in_window(&week_files, 24 * 7);

    // Daily activity: files modified in last 85 days (84 + 1 buffer)
    let activity_files = collect_jsonl_files(&data_dirs, Some(24 * 85));
    let daily_activity = collect_daily_activity(&activity_files);

    let since = match period {
        "today" => Some(Utc::now().date_naive().and_hms_opt(0, 0, 0).unwrap().and_utc()),
        "week" => Some(Utc::now() - chrono::Duration::days(7)),
        "month" => Some(Utc::now() - chrono::Duration::days(30)),
        _ => None, // "all"
    };

    Ok(aggregate_usage(all_entries, since, quota_window_prompts, week_prompts, daily_activity))
}
