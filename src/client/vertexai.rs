use super::access_token::*;
use super::claude::*;
use super::openai::*;
use super::*;

use anyhow::{anyhow, bail, Context, Result};
use chrono::{Duration, Utc};
use reqwest::{Client as ReqwestClient, RequestBuilder};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{path::PathBuf, str::FromStr};

#[derive(Debug, Clone, Deserialize, Default)]
pub struct VertexAIConfig {
    pub name: Option<String>,
    pub project_id: Option<String>,
    pub location: Option<String>,
    pub adc_file: Option<String>,
    #[serde(default)]
    pub models: Vec<ModelData>,
    pub patch: Option<RequestPatch>,
    pub extra: Option<ExtraConfig>,
}

impl VertexAIClient {
    config_get_fn!(project_id, get_project_id);
    config_get_fn!(location, get_location);

    pub const PROMPTS: [PromptAction<'static>; 2] = [
        ("project_id", "Project ID", None),
        ("location", "Location", None),
    ];
}

#[async_trait::async_trait]
impl Client for VertexAIClient {
    client_common_fns!();

    async fn chat_completions_inner(
        &self,
        client: &ReqwestClient,
        data: ChatCompletionsData,
    ) -> Result<ChatCompletionsOutput> {
        prepare_gcloud_access_token(client, self.name(), &self.config.adc_file).await?;
        let model = self.model();
        let model_category = ModelCategory::from_str(model.real_name())?;
        let request_data = prepare_chat_completions(self, data, &model_category)?;
        let builder = self.request_builder(client, request_data);
        match model_category {
            ModelCategory::Gemini => gemini_chat_completions(builder, model).await,
            ModelCategory::Claude => claude_chat_completions(builder, model).await,
            ModelCategory::Mistral => openai_chat_completions(builder, model).await,
        }
    }

    async fn chat_events_inner(
        &self,
        client: &ReqwestClient,
        data: ChatCompletionsData,
    ) -> Result<ChatEventStream> {
        prepare_gcloud_access_token(client, self.name(), &self.config.adc_file).await?;
        let model = self.model();
        let model_category = ModelCategory::from_str(model.real_name())?;
        let request_data = prepare_chat_completions(self, data, &model_category)?;
        let builder = self.request_builder(client, request_data);
        match model_category {
            ModelCategory::Gemini => gemini_chat_events(builder, model).await,
            ModelCategory::Claude => claude_chat_events(builder, model).await,
            ModelCategory::Mistral => openai_chat_events(builder, model).await,
        }
    }

    async fn embeddings_inner(
        &self,
        client: &ReqwestClient,
        data: &EmbeddingsData,
    ) -> Result<Vec<Vec<f32>>> {
        prepare_gcloud_access_token(client, self.name(), &self.config.adc_file).await?;
        let request_data = prepare_embeddings(self, data)?;
        let builder = self.request_builder(client, request_data);
        embeddings(builder, self.model()).await
    }
}

fn prepare_chat_completions(
    self_: &VertexAIClient,
    data: ChatCompletionsData,
    model_category: &ModelCategory,
) -> Result<RequestData> {
    let project_id = self_.get_project_id()?;
    let location = self_.get_location()?;
    let access_token = get_access_token(self_.name())?;

    let base_url = prediction_base_url(&project_id, &location, *model_category);

    let model_name = self_.model.real_name();

    let url = match model_category {
        ModelCategory::Gemini => {
            let func = match data.stream {
                true => "streamGenerateContent",
                false => "generateContent",
            };
            format!("{base_url}/google/models/{model_name}:{func}")
        }
        ModelCategory::Claude => {
            let func = match data.stream {
                true => "streamRawPredict",
                false => "rawPredict",
            };
            format!("{base_url}/anthropic/models/{model_name}:{func}")
        }
        ModelCategory::Mistral => {
            let func = match data.stream {
                true => "streamRawPredict",
                false => "rawPredict",
            };
            format!("{base_url}/mistralai/models/{model_name}:{func}")
        }
    };

    let body = match model_category {
        ModelCategory::Gemini => gemini_build_chat_completions_body(data, &self_.model)?,
        ModelCategory::Claude => {
            let mut body = claude_build_chat_completions_body(data, &self_.model)?;
            if let Some(body_obj) = body.as_object_mut() {
                body_obj.remove("model");
            }
            body["anthropic_version"] = "vertex-2023-10-16".into();
            body
        }
        ModelCategory::Mistral => {
            let mut body = openai_build_chat_completions_body(data, &self_.model);
            if let Some(body_obj) = body.as_object_mut() {
                body_obj["model"] = strip_model_version(self_.model.real_name()).into();
            }
            body
        }
    };

    let mut request_data = RequestData::new(url, body);

    request_data.bearer_auth(access_token);

    Ok(request_data)
}

fn prediction_base_url(project_id: &str, location: &str, model_category: ModelCategory) -> String {
    let host = match (model_category, location) {
        (_, "global") => "aiplatform.googleapis.com".to_string(),
        (ModelCategory::Claude, "us") => "aiplatform.us.rep.googleapis.com".to_string(),
        (ModelCategory::Claude, "eu") => "aiplatform.eu.rep.googleapis.com".to_string(),
        _ => format!("{location}-aiplatform.googleapis.com"),
    };
    format!("https://{host}/v1/projects/{project_id}/locations/{location}/publishers")
}

fn prepare_embeddings(self_: &VertexAIClient, data: &EmbeddingsData) -> Result<RequestData> {
    let project_id = self_.get_project_id()?;
    let location = self_.get_location()?;
    let access_token = get_access_token(self_.name())?;

    let base_url = if location == "global" {
        format!("https://aiplatform.googleapis.com/v1/projects/{project_id}/locations/global/publishers")
    } else {
        format!("https://{location}-aiplatform.googleapis.com/v1/projects/{project_id}/locations/{location}/publishers")
    };
    let url = format!(
        "{base_url}/google/models/{}:predict",
        self_.model.real_name()
    );

    let instances: Vec<_> = data.texts.iter().map(|v| json!({"content": v})).collect();

    let body = json!({
        "instances": instances,
    });

    let mut request_data = RequestData::new(url, body);

    request_data.bearer_auth(access_token);

    Ok(request_data)
}

pub async fn gemini_chat_completions(
    builder: RequestBuilder,
    _model: &Model,
) -> Result<ChatCompletionsOutput> {
    let res = builder.send().await?;
    let status = res.status();
    let data: Value = res.json().await?;
    if !status.is_success() {
        catch_error(&data, status.as_u16())?;
    }
    debug!("non-stream-data: {data}");
    gemini_extract_chat_completions_text(&data)
}

pub async fn gemini_chat_events(
    builder: RequestBuilder,
    _model: &Model,
) -> Result<ChatEventStream> {
    let res = builder.send().await?;
    let status = res.status();
    if !status.is_success() {
        let data: Value = res.json().await?;
        catch_error(&data, status.as_u16())?;
        return Ok(Box::pin(futures_util::stream::empty::<Result<ChatEvent>>()));
    }
    let handle = |value: &str, events: &mut Vec<ChatEvent>| -> Result<()> {
        let data: Value = serde_json::from_str(value)?;
        debug!("stream-data: {data}");
        if data.get("usageMetadata").is_some() {
            events.push(ChatEvent::Usage(TokenUsage::new(
                data["usageMetadata"]["promptTokenCount"].as_u64(),
                data["usageMetadata"]["candidatesTokenCount"].as_u64(),
            )));
        }
        if let Some(parts) = data["candidates"][0]["content"]["parts"].as_array() {
            for (i, part) in parts.iter().enumerate() {
                if let Some(text) = part["text"].as_str() {
                    if i > 0 {
                        events.push(ChatEvent::Text("\n\n".to_string()));
                    }
                    events.push(ChatEvent::Text(text.to_string()));
                } else if let (Some(name), Some(args)) = (
                    part["functionCall"]["name"].as_str(),
                    part["functionCall"]["args"].as_object(),
                ) {
                    events.push(ChatEvent::ToolCall(ToolCall::new(
                        name.to_string(),
                        json!(args),
                        None,
                    )));
                }
            }
        } else if let Some(block_reason) = data["promptFeedback"]["blockReason"].as_str() {
            bail!("Blocked by provider: {block_reason}")
        } else if let Some(finish_reason) = data["candidates"][0]["finishReason"].as_str() {
            match finish_reason {
                "STOP" => {
                    debug!("Provider completed with STOP but no content parts");
                }
                "SAFETY" => bail!("Blocked due to safety"),
                "RECITATION" => bail!("Blocked due to recitation"),
                "MALFORMED_FUNCTION_CALL" => {
                    if let Some(msg) = data["candidates"][0]["finishMessage"].as_str() {
                        if let Some(tool_call) = recover_malformed_function_call(msg) {
                            debug!("Recovered malformed function call in stream: {:?}", tool_call);
                            events.push(ChatEvent::ToolCall(tool_call));
                            return Ok(());
                        }
                    }
                    bail!("Provider ended generation without content (finishReason: MALFORMED_FUNCTION_CALL)");
                }
                other => bail!("Provider ended generation without content (finishReason: {other})"),
            }
        }

        Ok(())
    };
    Ok(json_chat_event_stream(res.bytes_stream(), handle))
}

async fn embeddings(builder: RequestBuilder, _model: &Model) -> Result<EmbeddingsOutput> {
    let res = builder.send().await?;
    let status = res.status();
    let data: Value = res.json().await?;
    if !status.is_success() {
        catch_error(&data, status.as_u16())?;
    }
    let res_body: EmbeddingsResBody =
        serde_json::from_value(data).context("Invalid embeddings data")?;
    let output = res_body
        .predictions
        .into_iter()
        .map(|v| v.embeddings.values)
        .collect();
    Ok(output)
}

#[derive(Deserialize)]
struct EmbeddingsResBody {
    predictions: Vec<EmbeddingsResBodyPrediction>,
}

#[derive(Deserialize)]
struct EmbeddingsResBodyPrediction {
    embeddings: EmbeddingsResBodyPredictionEmbeddings,
}

#[derive(Deserialize)]
struct EmbeddingsResBodyPredictionEmbeddings {
    values: Vec<f32>,
}

fn gemini_extract_chat_completions_text(data: &Value) -> Result<ChatCompletionsOutput> {
    let mut text_parts = vec![];
    let mut tool_calls = vec![];
    if let Some(parts) = data["candidates"][0]["content"]["parts"].as_array() {
        for part in parts {
            if let Some(text) = part["text"].as_str() {
                text_parts.push(text);
            }
            if let (Some(name), Some(args)) = (
                part["functionCall"]["name"].as_str(),
                part["functionCall"]["args"].as_object(),
            ) {
                tool_calls.push(ToolCall::new(name.to_string(), json!(args), None));
            }
        }
    }

    let text = text_parts.join("\n\n");
    if text.is_empty() && tool_calls.is_empty() {
        if let Some("SAFETY") = data["promptFeedback"]["blockReason"]
            .as_str()
            .or_else(|| data["candidates"][0]["finishReason"].as_str())
        {
            bail!("Blocked due to safety")
        } else if data["candidates"][0]["finishReason"].as_str() == Some("MALFORMED_FUNCTION_CALL") {
            if let Some(msg) = data["candidates"][0]["finishMessage"].as_str() {
                if let Some(tool_call) = recover_malformed_function_call(msg) {
                    debug!("Recovered malformed function call in non-streaming: {:?}", tool_call);
                    tool_calls.push(tool_call);
                    let output = ChatCompletionsOutput {
                        text: String::new(),
                        tool_calls,
                        id: None,
                        input_tokens: data["usageMetadata"]["promptTokenCount"].as_u64(),
                        output_tokens: data["usageMetadata"]["candidatesTokenCount"].as_u64(),
                    };
                    return Ok(output);
                }
            }
            bail!("Invalid response data: {data}");
        } else {
            bail!("Invalid response data: {data}");
        }
    }
    let output = ChatCompletionsOutput {
        text,
        tool_calls,
        id: None,
        input_tokens: data["usageMetadata"]["promptTokenCount"].as_u64(),
        output_tokens: data["usageMetadata"]["candidatesTokenCount"].as_u64(),
    };
    Ok(output)
}

pub fn gemini_build_chat_completions_body(
    data: ChatCompletionsData,
    model: &Model,
) -> Result<Value> {
    let ChatCompletionsData {
        mut messages,
        temperature,
        top_p,
        functions,
        stream: _,
        include_usage: _,
    } = data;

    let system_message = extract_system_message(&mut messages);

    let mut network_image_urls = vec![];
    let contents: Vec<Value> = messages
        .into_iter()
        .flat_map(|message| {
            let Message { role, content } = message;
            let role = match role {
                MessageRole::User => "user",
                _ => "model",
            };
               match content {
                    MessageContent::Text(text) => vec![json!({
                        "role": role,
                        "parts": [{ "text": text }]
                    })],
                    MessageContent::Array(list) => {
                        let parts: Vec<Value> = list
                            .into_iter()
                            .map(|item| match item {
                                MessageContentPart::Text { text } => json!({"text": text}),
                                MessageContentPart::ImageUrl { image_url: ImageUrl { url } } => {
                                    if let Some((mime_type, data)) = url.strip_prefix("data:").and_then(|v| v.split_once(";base64,")) {
                                        json!({ "inline_data": { "mime_type": mime_type, "data": data } })
                                    } else {
                                        network_image_urls.push(url.clone());
                                        json!({ "url": url })
                                    }
                                },
                            })
                            .collect();
                        vec![json!({ "role": role, "parts": parts })]
                    },
                    MessageContent::ToolCalls(MessageContentToolCalls { tool_results, .. }) => {
                        let model_parts: Vec<Value> = tool_results.iter().map(|tool_result| {
                            json!({
                                "functionCall": {
                                    "name": tool_result.call.name,
                                    "args": tool_result.call.arguments,
                                }
                            })
                        }).collect();
                        let function_parts: Vec<Value> = tool_results.into_iter().map(|tool_result| {
                            json!({
                                "functionResponse": {
                                    "name": tool_result.call.name,
                                    "response": {
                                        "name": tool_result.call.name,
                                        "content": tool_result.output,
                                    }
                                }
                            })
                        }).collect();
                        vec![
                            json!({ "role": "model", "parts": model_parts }),
                            json!({ "role": "function", "parts": function_parts }),
                        ]
                    }
                }
        })
        .collect();

    if !network_image_urls.is_empty() {
        bail!(
            "The model does not support network images: {:?}",
            network_image_urls
        );
    }

    let mut body = json!({ "contents": contents, "generationConfig": {} });

    let mut system_text = system_message.map(|v| v.to_string());
    if functions.is_some() {
        const TOOL_INSTRUCTION: &str = "\nWhen invoking tools, call the tool directly using the exact declared function name. Do NOT output code, python calls, or prepend namespaces (e.g. do NOT write `print(default_api....)`).";
        match system_text.as_mut() {
            Some(s) => s.push_str(TOOL_INSTRUCTION),
            None => system_text = Some(TOOL_INSTRUCTION.to_string()),
        }
    }

    if let Some(v) = system_text {
        body["systemInstruction"] = json!({ "parts": [{"text": v }] });
    }

    if let Some(v) = model.max_tokens_param() {
        body["generationConfig"]["maxOutputTokens"] = v.into();
    }
    if let Some(v) = temperature {
        body["generationConfig"]["temperature"] = v.into();
    }
    if let Some(v) = top_p {
        body["generationConfig"]["topP"] = v.into();
    }

    if let Some(functions) = functions {
        // Gemini doesn't support functions with parameters that have empty properties, so we need to patch it.
        let function_declarations: Vec<_> = functions
            .into_iter()
            .map(|function| {
                if function.parameters.is_empty_properties() {
                    json!({
                        "name": function.name,
                        "description": function.description,
                    })
                } else {
                    json!(function)
                }
            })
            .collect();
        body["tools"] = json!([{ "functionDeclarations": function_declarations }]);
    }

    Ok(body)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModelCategory {
    Gemini,
    Claude,
    Mistral,
}

impl FromStr for ModelCategory {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        if s.starts_with("gemini") {
            Ok(ModelCategory::Gemini)
        } else if s.starts_with("claude") {
            Ok(ModelCategory::Claude)
        } else if s.starts_with("mistral") || s.starts_with("codestral") {
            Ok(ModelCategory::Mistral)
        } else {
            unsupported_model!(s)
        }
    }
}

pub async fn prepare_gcloud_access_token(
    client: &reqwest::Client,
    client_name: &str,
    adc_file: &Option<String>,
) -> Result<()> {
    if !is_valid_access_token(client_name) {
        let (token, expires_in) = fetch_access_token(client, adc_file)
            .await
            .with_context(|| "Failed to fetch access token")?;
        let expires_at = Utc::now()
            + Duration::try_seconds(expires_in)
                .ok_or_else(|| anyhow!("Failed to parse expires_in of access_token"))?;
        set_access_token(client_name, token, expires_at.timestamp())
    }
    Ok(())
}

async fn fetch_access_token(
    client: &reqwest::Client,
    file: &Option<String>,
) -> Result<(String, i64)> {
    let credentials = load_adc(file).await?;
    let value: Value = client
        .post("https://oauth2.googleapis.com/token")
        .json(&credentials)
        .send()
        .await?
        .json()
        .await?;

    if let (Some(access_token), Some(expires_in)) =
        (value["access_token"].as_str(), value["expires_in"].as_i64())
    {
        Ok((access_token.to_string(), expires_in))
    } else if let Some(err_msg) = value["error_description"].as_str() {
        bail!("{err_msg}")
    } else {
        bail!("Invalid response data: {value}")
    }
}

async fn load_adc(file: &Option<String>) -> Result<Value> {
    let adc_file = file
        .as_ref()
        .map(PathBuf::from)
        .or_else(default_adc_file)
        .ok_or_else(|| anyhow!("No application_default_credentials.json"))?;
    let data = tokio::fs::read_to_string(adc_file).await?;
    let data: Value = serde_json::from_str(&data)?;
    if let (Some(client_id), Some(client_secret), Some(refresh_token)) = (
        data["client_id"].as_str(),
        data["client_secret"].as_str(),
        data["refresh_token"].as_str(),
    ) {
        Ok(json!({
            "client_id": client_id,
            "client_secret": client_secret,
            "refresh_token": refresh_token,
            "grant_type": "refresh_token",
        }))
    } else {
        bail!("Invalid application_default_credentials.json")
    }
}

#[cfg(not(windows))]
fn default_adc_file() -> Option<PathBuf> {
    let mut path = dirs::home_dir()?;
    path.push(".config");
    path.push("gcloud");
    path.push("application_default_credentials.json");
    Some(path)
}

#[cfg(windows)]
fn default_adc_file() -> Option<PathBuf> {
    let mut path = dirs::config_dir()?;
    path.push("gcloud");
    path.push("application_default_credentials.json");
    Some(path)
}

fn strip_model_version(name: &str) -> &str {
    match name.split_once('@') {
        Some((v, _)) => v,
        None => name,
    }
}

pub(crate) fn recover_malformed_function_call(msg: &str) -> Option<ToolCall> {
    let text = msg.strip_prefix("Malformed function call:")?.trim();
    let text = if let Some(stripped) = text.strip_prefix("print(") {
        stripped.strip_suffix(')')?.trim()
    } else {
        text
    };
    let open_paren = text.find('(')?;
    let close_paren = text.rfind(')')?;
    if close_paren <= open_paren {
        return None;
    }
    let full_name = text[..open_paren].trim();
    let name = full_name.rsplit('.').next()?.trim();
    let args_str = text[open_paren + 1..close_paren].trim();
    let args = parse_python_kwargs(args_str)?;
    Some(ToolCall::new(name.to_string(), Value::Object(args), None))
}

fn parse_python_kwargs(args_str: &str) -> Option<serde_json::Map<String, Value>> {
    let mut map = serde_json::Map::new();
    if args_str.is_empty() {
        return Some(map);
    }

    let mut pairs = Vec::new();
    let mut current = String::new();
    let mut in_quote: Option<char> = None;
    let mut escape = false;
    let mut depth = 0usize;

    for ch in args_str.chars() {
        if escape {
            current.push(ch);
            escape = false;
            continue;
        }
        if ch == '\\' {
            escape = true;
            current.push(ch);
            continue;
        }
        if let Some(q) = in_quote {
            if ch == q {
                in_quote = None;
            }
            current.push(ch);
            continue;
        }
        if ch == '"' || ch == '\'' {
            in_quote = Some(ch);
            current.push(ch);
            continue;
        }
        if ch == '(' || ch == '[' || ch == '{' {
            depth += 1;
            current.push(ch);
            continue;
        }
        if ch == ')' || ch == ']' || ch == '}' {
            depth = depth.saturating_sub(1);
            current.push(ch);
            continue;
        }
        if ch == ',' && depth == 0 {
            pairs.push(current.trim().to_string());
            current.clear();
            continue;
        }
        current.push(ch);
    }
    if !current.trim().is_empty() {
        pairs.push(current.trim().to_string());
    }

    for pair in pairs {
        let eq_idx = pair.find('=')?;
        let key = pair[..eq_idx].trim().to_string();
        let val_str = pair[eq_idx + 1..].trim();
        let val = parse_python_val(val_str);
        map.insert(key, val);
    }

    Some(map)
}

fn parse_python_val(val_str: &str) -> Value {
    if (val_str.starts_with('"') && val_str.ends_with('"'))
        || (val_str.starts_with('\'') && val_str.ends_with('\''))
    {
        if val_str.len() >= 2 {
            let inner = &val_str[1..val_str.len() - 1];
            let unescaped = inner.replace("\\\"", "\"").replace("\\'", "'");
            return Value::String(unescaped);
        }
        return Value::String(String::new());
    }
    if val_str.eq_ignore_ascii_case("true") {
        return Value::Bool(true);
    }
    if val_str.eq_ignore_ascii_case("false") {
        return Value::Bool(false);
    }
    if val_str.eq_ignore_ascii_case("none") || val_str.eq_ignore_ascii_case("null") {
        return Value::Null;
    }
    if let Ok(num) = val_str.parse::<i64>() {
        return json!(num);
    }
    if let Ok(num) = val_str.parse::<f64>() {
        return json!(num);
    }
    if let Ok(val) = serde_json::from_str::<Value>(val_str) {
        return val;
    }
    Value::String(val_str.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claude_client(location: &str) -> VertexAIClient {
        let client_name = format!("vertexai-fable-route-test-{location}");
        set_access_token(&client_name, "fixture-token".to_string(), i64::MAX);
        let mut model = Model::new(&client_name, "claude-fable-5");
        model.set_max_tokens(Some(128_000), true);
        VertexAIClient {
            global_config: Default::default(),
            config: VertexAIConfig {
                name: Some(client_name),
                project_id: Some("fixture-project".to_string()),
                location: Some(location.to_string()),
                adc_file: None,
                models: vec![],
                patch: None,
                extra: None,
            },
            model,
        }
    }

    fn completion_data(stream: bool) -> ChatCompletionsData {
        ChatCompletionsData {
            messages: vec![Message::new(
                MessageRole::User,
                MessageContent::Text("hello".to_string()),
            )],
            temperature: None,
            top_p: None,
            functions: None,
            stream,
            include_usage: false,
        }
    }

    #[test]
    fn claude_global_and_multiregion_routes_match_vertex_contract() -> Result<()> {
        for (location, host) in [
            ("global", "aiplatform.googleapis.com"),
            ("us", "aiplatform.us.rep.googleapis.com"),
            ("eu", "aiplatform.eu.rep.googleapis.com"),
        ] {
            for (stream, suffix) in [(false, "rawPredict"), (true, "streamRawPredict")] {
                let request = prepare_chat_completions(
                    &claude_client(location),
                    completion_data(stream),
                    &ModelCategory::Claude,
                )?;

                assert_eq!(
                    request.url,
                    format!(
                        "https://{host}/v1/projects/fixture-project/locations/{location}/publishers/anthropic/models/claude-fable-5:{suffix}"
                    )
                );
                let mut expected_body = json!({
                    "messages": [{"role": "user", "content": "hello"}],
                    "max_tokens": 128_000,
                    "anthropic_version": "vertex-2023-10-16"
                });
                if stream {
                    expected_body["stream"] = true.into();
                }
                assert_eq!(request.body, expected_body);
                assert!(request.body.get("model").is_none());
                assert_eq!(
                    request.headers.get("authorization").map(String::as_str),
                    Some("Bearer fixture-token")
                );
            }
        }
        Ok(())
    }

    #[test]
    fn ordinary_regional_routing_remains_unchanged() {
        assert_eq!(
            prediction_base_url(
                "fixture-project",
                "us-central1",
                ModelCategory::Claude
            ),
            "https://us-central1-aiplatform.googleapis.com/v1/projects/fixture-project/locations/us-central1/publishers"
        );
        assert_eq!(
            prediction_base_url("fixture-project", "us", ModelCategory::Gemini),
            "https://us-aiplatform.googleapis.com/v1/projects/fixture-project/locations/us/publishers"
        );
    }

    #[test]
    fn test_recover_malformed_function_call() {
        let msg1 = "Malformed function call: print(default_api.web_search(query=\"Rust async runtimes comparison 2024 trends\", links=true))";
        let tool1 = recover_malformed_function_call(msg1).expect("Should recover tool1");
        assert_eq!(tool1.name, "web_search");
        assert_eq!(tool1.arguments["query"], "Rust async runtimes comparison 2024 trends");
        assert_eq!(tool1.arguments["links"], true);

        let msg2 = "Malformed function call: default_api.fetch_and_summarize(url='https://example.com/test')";
        let tool2 = recover_malformed_function_call(msg2).expect("Should recover tool2");
        assert_eq!(tool2.name, "fetch_and_summarize");
        assert_eq!(tool2.arguments["url"], "https://example.com/test");

        let msg3 = "Malformed function call: web_search(query=\"python asyncio\", count=5)";
        let tool3 = recover_malformed_function_call(msg3).expect("Should recover tool3");
        assert_eq!(tool3.name, "web_search");
        assert_eq!(tool3.arguments["query"], "python asyncio");
        assert_eq!(tool3.arguments["count"], 5);
    }
}
