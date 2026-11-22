//! One completion per case, through rig-core.

use anyhow::{bail, Context, Result};
use rig_core::completion::{CompletionRequest, CompletionResponse, Effort, Reasoning, Usage};
use rig_core::message::{ImageDetail, ImageMediaType, Message, UserContent};
use rig_core::providers::deepseek;
use rig_core::providers::ollama::extension::{KeepAlive, OllamaOptions};
use rig_core::providers::ollama::Ollama;

use crate::corpus::{prompt, LoadedCase};
use crate::registry::{ModelSpec, Provider};

pub struct Completion {
    pub text: String,
    pub usage: Usage,
}

/// Send one case and return the model's text and token usage.
pub async fn complete(spec: &ModelSpec, case: &LoadedCase, rubric: &str) -> Result<Completion> {
    let mut content = vec![UserContent::text(prompt(rubric, &case.case))];
    let full = std::fs::read(case.dir.join(&case.case.image))
        .with_context(|| format!("read the full image for {}", case.slug))?;
    content.push(UserContent::image_raw(
        full,
        Some(ImageMediaType::PNG),
        Some(ImageDetail::High),
    ));
    if let Some(crop) = &case.case.crop {
        let crop = std::fs::read(case.dir.join(crop))
            .with_context(|| format!("read the crop for {}", case.slug))?;
        content.push(UserContent::image_raw(
            crop,
            Some(ImageMediaType::PNG),
            Some(ImageDetail::High),
        ));
    }

    let reasoning = if spec.think {
        Reasoning::Effort(Effort::Medium)
    } else {
        Reasoning::Off
    };
    let mut request = CompletionRequest::new(Message::User { content })
        .temperature(spec.temperature)
        .seed(spec.seed)
        .reasoning(reasoning);
    if let Some(max_tokens) = spec.max_output_tokens {
        request = request.max_tokens(max_tokens);
    }

    match spec.provider {
        Provider::Ollama => {
            let options = OllamaOptions::default()
                .keep_alive(KeepAlive::duration("30m"))
                .num_ctx(spec.context);
            let model = Ollama::new().native_completion(spec.model.clone());
            let response = model
                .call(request.provider_option(options))
                .await
                .with_context(|| format!("ollama completion for {}", case.slug))?;
            finish(response, &case.slug)
        }
        Provider::DeepSeek => {
            let client = deepseek::from_env().context("DeepSeek credentials are not configured")?;
            let model = client.chat(spec.model.clone());
            let response = model
                .call(request)
                .await
                .with_context(|| format!("deepseek completion for {}", case.slug))?;
            finish(response, &case.slug)
        }
    }
}

fn finish(response: CompletionResponse, slug: &str) -> Result<Completion> {
    if let Some(error) = &response.error {
        bail!("{slug}: provider error: {error}");
    }
    let text = response
        .message()
        .and_then(|message| message.final_answer())
        .with_context(|| format!("{slug}: the reply has no final text"))?;
    Ok(Completion {
        text,
        usage: response.usage,
    })
}
