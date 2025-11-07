use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DefaultsConfig {
    pub engine: Option<String>,
    pub mock: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EnginesConfig {
    #[serde(default)]
    pub codex: Option<EngineDetail>,
    #[serde(default)]
    pub codemachine: Option<EngineDetail>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EngineDetail {
    pub bin: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentSpec {
    pub engine: Option<String>,
    pub model: Option<String>,
    pub prompt: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StepInput {
    pub template: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StepOutput {
    pub kind: String, // "stdout" | "file"
    pub path: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StepSpec {
    #[serde(rename = "agent", alias = "use")]
    pub agent: String,
    #[serde(default)]
    pub description: Option<String>,
    // Optional per-step overrides for the referenced agent
    #[serde(default)]
    pub engine: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub input: StepInput,
    #[serde(default)]
    pub output: StepOutput,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkflowSpec {
    pub description: Option<String>,
    #[serde(default)]
    pub steps: Vec<StepSpec>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FlowConfig {
    pub name: Option<String>,
    pub version: Option<String>,
    #[serde(default)]
    pub defaults: DefaultsConfig,
    #[serde(default)]
    pub engines: EnginesConfig,
    #[serde(default)]
    pub agents: HashMap<String, AgentSpec>,
    #[serde(default)]
    pub workflows: HashMap<String, WorkflowSpec>,
    #[serde(default)]
    pub vars: HashMap<String, String>,
}

impl FlowConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("failed to read config file {}", path.display()))?;
        let cfg: Self = toml::from_str(&content)
            .with_context(|| format!("failed to parse TOML at {}", path.display()))?;
        Ok(cfg)
    }

    pub fn merge_cli_vars(&mut self, cli_vars: HashMap<String, String>) {
        for (k, v) in cli_vars {
            self.vars.insert(k, v);
        }
    }
}

// A standalone workflow file schema: contains a single [workflow] table
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkflowFile {
    pub name: Option<String>,
    pub version: Option<String>,
    #[serde(default)]
    pub defaults: DefaultsConfig,
    #[serde(default)]
    pub engines: EnginesConfig,
    #[serde(default)]
    pub agents: HashMap<String, AgentSpec>,
    pub workflow: WorkflowSpec,
    #[serde(default)]
    pub vars: HashMap<String, String>,
}

impl WorkflowFile {
    pub fn load(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("failed to read workflow file {}", path.display()))?;
        let cfg: Self = toml::from_str(&content)
            .with_context(|| format!("failed to parse TOML at {}", path.display()))?;
        Ok(cfg)
    }

    pub fn into_flow_config(self) -> FlowConfig {
        let mut workflows = HashMap::new();
        workflows.insert(
            self.name.clone().unwrap_or_else(|| "main".to_string()),
            self.workflow,
        );
        FlowConfig {
            name: self.name,
            version: self.version,
            defaults: self.defaults,
            engines: self.engines,
            agents: self.agents,
            workflows,
            vars: self.vars,
        }
    }
}
