use crate::exec_command::relativize_to_home;
use crate::text_formatting;
use codex_core::auth::get_auth_file;
use codex_core::auth::try_read_auth_json;
use codex_core::config::Config;
use codex_core::project_doc::discover_project_doc_paths;
use std::path::Path;
use unicode_width::UnicodeWidthStr;

use super::account::StatusAccountDisplay;

fn normalize_agents_display_path(path: &Path) -> String {
    dunce::simplified(path).display().to_string()
}

pub(crate) fn compose_model_display(
    config: &Config,
    entries: &[(&str, String)],
) -> (String, Vec<String>) {
    let mut details: Vec<String> = Vec::new();
    if let Some((_, effort)) = entries.iter().find(|(k, _)| *k == "reasoning effort") {
        details.push(format!("reasoning {}", effort.to_ascii_lowercase()));
    }
    if let Some((_, summary)) = entries.iter().find(|(k, _)| *k == "reasoning summaries") {
        let summary = summary.trim();
        if summary.eq_ignore_ascii_case("none") || summary.eq_ignore_ascii_case("off") {
            details.push("summaries off".to_string());
        } else if !summary.is_empty() {
            details.push(format!("summaries {}", summary.to_ascii_lowercase()));
        }
    }

    (config.model.clone(), details)
}

pub(crate) fn compose_agents_summary(config: &Config) -> String {
    match discover_project_doc_paths(config) {
        Ok(paths) => {
            let mut rels: Vec<String> = Vec::new();
            for p in paths {
                let file_name = p
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_else(|| "<unknown>".to_string());
                let display = if let Some(parent) = p.parent() {
                    if parent == config.cwd {
                        file_name.clone()
                    } else {
                        let mut cur = config.cwd.as_path();
                        let mut ups = 0usize;
                        let mut reached = false;
                        while let Some(c) = cur.parent() {
                            if cur == parent {
                                reached = true;
                                break;
                            }
                            cur = c;
                            ups += 1;
                        }
                        if reached {
                            let up = format!("..{}", std::path::MAIN_SEPARATOR);
                            format!("{}{}", up.repeat(ups), file_name)
                        } else if let Ok(stripped) = p.strip_prefix(&config.cwd) {
                            normalize_agents_display_path(stripped)
                        } else {
                            normalize_agents_display_path(&p)
                        }
                    }
                } else {
                    normalize_agents_display_path(&p)
                };
                rels.push(display);
            }
            if rels.is_empty() {
                "<none>".to_string()
            } else {
                rels.join(", ")
            }
        }
        Err(_) => "<none>".to_string(),
    }
}

pub(crate) fn compose_account_display(config: &Config) -> Option<StatusAccountDisplay> {
    let auth_file = get_auth_file(&config.codex_home);
    let auth = try_read_auth_json(&auth_file).ok()?;

    if let Some(tokens) = auth.tokens.as_ref() {
        let info = &tokens.id_token;
        let email = info.email.clone();
        let plan = info.get_chatgpt_plan_type().map(|plan| title_case(&plan));
        return Some(StatusAccountDisplay::ChatGpt { email, plan });
    }

    if let Some(key) = auth.openai_api_key
        && !key.is_empty()
    {
        return Some(StatusAccountDisplay::ApiKey);
    }

    None
}

pub(crate) fn format_tokens_compact(value: u64) -> String {
    if value == 0 {
        return "0".to_string();
    }
    if value < 1_000 {
        return value.to_string();
    }

    let (scaled, suffix) = if value >= 1_000_000_000_000 {
        (value as f64 / 1_000_000_000_000.0, "T")
    } else if value >= 1_000_000_000 {
        (value as f64 / 1_000_000_000.0, "B")
    } else if value >= 1_000_000 {
        (value as f64 / 1_000_000.0, "M")
    } else {
        (value as f64 / 1_000.0, "K")
    };

    let decimals = if scaled < 10.0 {
        2
    } else if scaled < 100.0 {
        1
    } else {
        0
    };

    let mut formatted = format!("{scaled:.decimals$}");
    if formatted.contains('.') {
        while formatted.ends_with('0') {
            formatted.pop();
        }
        if formatted.ends_with('.') {
            formatted.pop();
        }
    }

    format!("{formatted}{suffix}")
}

pub(crate) fn format_directory_display(directory: &Path, max_width: Option<usize>) -> String {
    let formatted = if let Some(rel) = relativize_to_home(directory) {
        if rel.as_os_str().is_empty() {
            "~".to_string()
        } else {
            format!("~{}{}", std::path::MAIN_SEPARATOR, rel.display())
        }
    } else {
        directory.display().to_string()
    };

    if let Some(max_width) = max_width {
        if max_width == 0 {
            return String::new();
        }
        if UnicodeWidthStr::width(formatted.as_str()) > max_width {
            return text_formatting::center_truncate_path(&formatted, max_width);
        }
    }

    formatted
}

pub(crate) fn format_reset_duration(resets_in_seconds: u64) -> String {
    let minutes = resets_in_seconds / 60;
    let hours = minutes / 60;
    let days = hours / 24;

    if days > 0 {
        format!("in {days}d")
    } else if hours > 0 {
        format!("in {hours}h")
    } else if minutes > 0 {
        format!("in {minutes}m")
    } else {
        "in <1m".to_string()
    }
}

pub(crate) fn title_case(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    let mut chars = s.chars();
    let first = match chars.next() {
        Some(c) => c,
        None => return String::new(),
    };
    let rest: String = chars.as_str().to_ascii_lowercase();
    first.to_uppercase().collect::<String>() + &rest
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_reset_duration_days() {
        assert_eq!(format_reset_duration(86400), "in 1d");
        assert_eq!(format_reset_duration(172800), "in 2d");
    }

    #[test]
    fn test_format_reset_duration_hours() {
        assert_eq!(format_reset_duration(3600), "in 1h");
        assert_eq!(format_reset_duration(7200), "in 2h");
        assert_eq!(format_reset_duration(10800), "in 3h");
    }

    #[test]
    fn test_format_reset_duration_minutes() {
        assert_eq!(format_reset_duration(60), "in 1m");
        assert_eq!(format_reset_duration(1800), "in 30m");
        assert_eq!(format_reset_duration(3540), "in 59m");
    }

    #[test]
    fn test_format_reset_duration_less_than_minute() {
        assert_eq!(format_reset_duration(0), "in <1m");
        assert_eq!(format_reset_duration(30), "in <1m");
        assert_eq!(format_reset_duration(59), "in <1m");
    }
}
