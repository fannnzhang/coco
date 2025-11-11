use anyhow::Result;
use anyhow::bail;
use serde_json::Value;

use crate::runner::state_store::TokenUsage;
use crate::runner::state_store::WORKFLOW_STATE_SCHEMA_VERSION;

pub fn upgrade(raw: &str) -> Result<(Value, bool)> {
    let mut value: Value = serde_json::from_str(raw)?;
    let mut version = value
        .get("schema_version")
        .and_then(Value::as_u64)
        .unwrap_or(1) as u32;
    if version > WORKFLOW_STATE_SCHEMA_VERSION {
        bail!(
            "workflow state schema version {version} is newer than supported {WORKFLOW_STATE_SCHEMA_VERSION}"
        );
    }
    if version == WORKFLOW_STATE_SCHEMA_VERSION {
        return Ok((value, false));
    }

    let mut migrated = false;
    while version < WORKFLOW_STATE_SCHEMA_VERSION {
        match version {
            1 => {
                migrate_v1_to_v2(&mut value)?;
                version = 2;
            }
            other => bail!("no migration path for workflow state schema version {other}"),
        }
        migrated = true;
    }

    value["schema_version"] = Value::from(WORKFLOW_STATE_SCHEMA_VERSION);
    Ok((value, migrated))
}

fn migrate_v1_to_v2(doc: &mut Value) -> Result<()> {
    let mut accumulated = TokenUsage::default();
    let mut saw_usage = false;

    if let Some(steps) = doc.get_mut("steps").and_then(Value::as_array_mut) {
        for step in steps {
            if let Some(delta) = step.get("token_delta").and_then(parse_usage) {
                accumulated.add_assign(&delta);
                saw_usage = true;
            }
        }
    }

    doc["token_usage"] = if saw_usage {
        serde_json::to_value(accumulated)?
    } else {
        Value::Null
    };
    Ok(())
}

fn parse_usage(value: &Value) -> Option<TokenUsage> {
    Some(TokenUsage {
        prompt_tokens: value.get("prompt_tokens")?.as_i64()?,
        completion_tokens: value.get("completion_tokens")?.as_i64()?,
        total_tokens: value.get("total_tokens")?.as_i64()?,
        total_cost: value.get("total_cost")?.as_f64()?,
    })
}
