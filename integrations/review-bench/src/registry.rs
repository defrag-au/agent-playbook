//! The model registry: a model is a configuration entry, not code.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

/// A provider this runner can drive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Ollama,
    DeepSeek,
}

impl Provider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ollama => "ollama",
            Self::DeepSeek => "deepseek",
        }
    }
}

/// One registry entry: what to call, how, and the budgets it must fit.
#[derive(Debug, Clone, Deserialize)]
pub struct ModelSpec {
    pub provider: Provider,
    /// The provider's wire model string.
    pub model: String,
    /// The context window requested from a local daemon.
    #[serde(default = "default_context")]
    pub context: u32,
    #[serde(default = "default_temperature")]
    pub temperature: f64,
    #[serde(default = "default_seed")]
    pub seed: u64,
    /// Ask a reasoning model to think before answering.
    #[serde(default)]
    pub think: bool,
    /// The model's measured resident size, in GB.
    #[serde(default)]
    pub resident_gb: Option<f64>,
    /// The largest resident size this machine may give it, in GB.
    #[serde(default)]
    pub max_resident_gb: Option<f64>,
    /// A run this heavy needs `--allow-heavy`.
    #[serde(default)]
    pub heavy: bool,
    /// The output-token limit for providers that take one.
    #[serde(default)]
    pub max_output_tokens: Option<u64>,
    /// The per-run cost ceiling, when the provider is billed.
    #[serde(default)]
    pub max_cost_usd: Option<f64>,
}

fn default_context() -> u32 {
    65_536
}

fn default_temperature() -> f64 {
    0.1
}

fn default_seed() -> u64 {
    42
}

#[derive(Debug, Clone, Deserialize)]
pub struct Registry {
    pub models: BTreeMap<String, ModelSpec>,
}

impl Registry {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("read the model registry at {}", path.display()))?;
        toml::from_str(&text)
            .with_context(|| format!("parse the model registry at {}", path.display()))
    }

    pub fn get(&self, name: &str) -> Result<&ModelSpec> {
        self.models.get(name).with_context(|| {
            let known: Vec<&str> = self.models.keys().map(String::as_str).collect();
            format!("no model {name:?}; known: {}", known.join(", "))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_registry_entry_carries_its_budgets() {
        let registry: Registry = toml::from_str(
            r#"
            [models.local]
            provider = "ollama"
            model = "qwen3-vl:30b-a3b-instruct"
            resident_gb = 20
            max_resident_gb = 24
            "#,
        )
        .expect("the registry parses");
        let spec = registry.get("local").expect("the entry exists");
        assert_eq!(spec.provider, Provider::Ollama);
        assert_eq!(spec.context, 65_536);
        assert_eq!(spec.temperature, 0.1);
        assert_eq!(spec.seed, 42);
        assert_eq!(spec.resident_gb, Some(20.0));
        assert!(!spec.heavy);
    }

    #[test]
    fn an_unknown_model_names_the_known_ones() {
        let registry: Registry = toml::from_str(
            r#"
            [models.local]
            provider = "ollama"
            model = "qwen3-vl:30b-a3b-instruct"
            "#,
        )
        .expect("the registry parses");
        let error = registry.get("missing").expect_err("missing is not a model");
        assert!(error.to_string().contains("local"), "{error}");
    }
}
