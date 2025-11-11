use std::env;

pub const RESUME_DISABLED_ENV: &str = "CODEX_RESUME_DISABLED";

pub fn resume_disabled() -> bool {
    match env::var(RESUME_DISABLED_ENV) {
        Ok(value) => parse_truthy(&value),
        Err(env::VarError::NotPresent) => false,
        Err(env::VarError::NotUnicode(_)) => true,
    }
}

fn parse_truthy(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return true;
    }
    !matches!(
        trimmed.to_ascii_lowercase().as_str(),
        "0" | "false" | "off" | "no"
    )
}
