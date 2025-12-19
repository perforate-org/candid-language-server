use ropey::Rope;
use serde_json::Value;
use std::{fmt, str::FromStr};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ServiceSnippetStyle {
    #[default]
    Call,
    Await,
    Async,
    AwaitLet,
}

impl FromStr for ServiceSnippetStyle {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "call" => Ok(ServiceSnippetStyle::Call),
            "await" => Ok(ServiceSnippetStyle::Await),
            "async" => Ok(ServiceSnippetStyle::Async),
            "awaitlet" | "await-let" | "await_let" | "asynclet" | "async-let" | "async_let" => {
                Ok(ServiceSnippetStyle::AwaitLet)
            }
            _ => Err(format!("Invalid service snippet style: {}", value)),
        }
    }
}

impl fmt::Display for ServiceSnippetStyle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ServiceSnippetStyle::Call => f.write_str("call"),
            ServiceSnippetStyle::Await => f.write_str("await"),
            ServiceSnippetStyle::Async => f.write_str("async"),
            ServiceSnippetStyle::AwaitLet => f.write_str("await-let"),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ServiceSnippetConfig {
    style: ServiceSnippetStyle,
}

impl ServiceSnippetConfig {
    pub fn style(&self) -> ServiceSnippetStyle {
        self.style
    }

    pub fn set_style(&mut self, style: ServiceSnippetStyle) {
        self.style = style;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompletionModeSetting {
    #[default]
    Auto,
    Full,
    Lightweight,
}

impl FromStr for CompletionModeSetting {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" => Ok(CompletionModeSetting::Auto),
            "full" | "standard" => Ok(CompletionModeSetting::Full),
            "light" | "lightweight" | "fast" => Ok(CompletionModeSetting::Lightweight),
            other => Err(format!("Invalid completion mode: {other}")),
        }
    }
}

impl fmt::Display for CompletionModeSetting {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CompletionModeSetting::Auto => f.write_str("auto"),
            CompletionModeSetting::Full => f.write_str("full"),
            CompletionModeSetting::Lightweight => f.write_str("lightweight"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionEngineMode {
    Full,
    Lightweight,
}

#[derive(Debug, Clone)]
pub struct CompletionConfig {
    mode: CompletionModeSetting,
    auto_line_limit: usize,
    auto_char_limit: usize,
}

impl Default for CompletionConfig {
    fn default() -> Self {
        Self {
            mode: CompletionModeSetting::Auto,
            auto_line_limit: 2000,
            auto_char_limit: 120_000,
        }
    }
}

impl CompletionConfig {
    pub fn behavior_for(&self, rope: &Rope) -> CompletionEngineMode {
        match self.mode {
            CompletionModeSetting::Full => CompletionEngineMode::Full,
            CompletionModeSetting::Lightweight => CompletionEngineMode::Lightweight,
            CompletionModeSetting::Auto => {
                if rope.len_lines() > self.auto_line_limit
                    || rope.len_chars() > self.auto_char_limit
                {
                    CompletionEngineMode::Lightweight
                } else {
                    CompletionEngineMode::Full
                }
            }
        }
    }

    fn apply_section(&mut self, value: &Value) {
        match value {
            Value::String(raw) => self.apply_mode_string(raw),
            Value::Object(map) => {
                if let Some(mode) = map.get("mode").and_then(Value::as_str) {
                    self.apply_mode_string(mode);
                } else if let Some(mode) = map.get("completionMode").and_then(Value::as_str) {
                    self.apply_mode_string(mode);
                }
                if let Some(auto) = map.get("auto").and_then(Value::as_object) {
                    if let Some(limit) = auto.get("lineLimit").and_then(Value::as_u64) {
                        self.auto_line_limit = sanitize_limit(limit, self.auto_line_limit);
                    }
                    if let Some(limit) = auto.get("charLimit").and_then(Value::as_u64) {
                        self.auto_char_limit = sanitize_limit(limit, self.auto_char_limit);
                    }
                }
            }
            _ => {}
        }
    }

    fn apply_mode_string(&mut self, raw: &str) {
        if let Ok(mode) = CompletionModeSetting::from_str(raw) {
            self.mode = mode;
        }
    }

    fn set_mode(&mut self, mode: CompletionModeSetting) {
        self.mode = mode;
    }
}

fn sanitize_limit(value: u64, fallback: usize) -> usize {
    let limit = value as usize;
    if limit == 0 { fallback } else { limit }
}

#[derive(Debug, Clone)]
pub struct FormatConfig {
    pub enabled: bool,
}

impl Default for FormatConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

impl FormatConfig {
    fn apply_section(&mut self, value: &Value) {
        if let Some(enabled) = value.as_bool() {
            self.enabled = enabled;
            return;
        }
        if let Some(obj) = value.as_object()
            && let Some(enabled) = obj.get("enabled").and_then(Value::as_bool)
        {
            self.enabled = enabled;
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ServerConfig {
    service_snippets: ServiceSnippetConfig,
    completion: CompletionConfig,
    format: FormatConfig,
}

impl ServerConfig {
    pub fn service_snippet_style(&self) -> ServiceSnippetStyle {
        self.service_snippets.style()
    }

    pub fn completion_mode_for(&self, rope: &Rope) -> CompletionEngineMode {
        self.completion.behavior_for(rope)
    }

    pub fn format_enabled(&self) -> bool {
        self.format.enabled
    }

    pub fn apply_settings(&mut self, value: Value) {
        if let Some(style) = extract_service_snippet_style(&value) {
            self.service_snippets.set_style(style);
        }
        if let Some(section) = completion_section(&value) {
            self.completion.apply_section(section);
        } else if let Some(mode) = completion_mode_from_value(&value) {
            self.completion.set_mode(mode);
        }
        if let Some(section) = format_section(&value) {
            self.format.apply_section(section);
        }
    }
}

fn extract_service_snippet_style(value: &Value) -> Option<ServiceSnippetStyle> {
    if let Some(style) = style_from_object(value) {
        return Some(style);
    }
    value
        .as_str()
        .and_then(|raw| ServiceSnippetStyle::from_str(raw).ok())
}

fn style_from_object(value: &Value) -> Option<ServiceSnippetStyle> {
    let obj = value.as_object()?;
    if let Some(snippets) = obj.get("serviceSnippets")
        && let Some(style) = extract_service_snippet_style(snippets)
    {
        return Some(style);
    }
    for key in ["serviceSnippetStyle", "snippetStyle", "snippet", "style"] {
        if let Some(raw) = obj.get(key).and_then(|value| value.as_str())
            && let Ok(style) = ServiceSnippetStyle::from_str(raw)
        {
            return Some(style);
        }
    }
    if let Some(section) = obj.get("candidLanguageServer") {
        return extract_service_snippet_style(section);
    }
    None
}

fn completion_section(value: &Value) -> Option<&Value> {
    if let Some(obj) = value.as_object() {
        if let Some(section) = obj.get("completion") {
            return Some(section);
        }
        if let Some(root) = obj.get("candidLanguageServer") {
            return completion_section(root);
        }
    }
    None
}

fn format_section(value: &Value) -> Option<&Value> {
    if let Some(obj) = value.as_object() {
        if let Some(section) = obj.get("format") {
            return Some(section);
        }
        if let Some(root) = obj.get("candidLanguageServer") {
            return format_section(root);
        }
    }
    None
}

fn completion_mode_from_value(value: &Value) -> Option<CompletionModeSetting> {
    if let Some(text) = value.as_str() {
        return CompletionModeSetting::from_str(text).ok();
    }
    let obj = value.as_object()?;
    if let Some(mode) = obj.get("completionMode").and_then(Value::as_str) {
        return CompletionModeSetting::from_str(mode).ok();
    }
    if let Some(root) = obj.get("candidLanguageServer") {
        return completion_mode_from_value(root);
    }
    None
}
