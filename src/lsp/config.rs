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
    pub indent_width: Option<usize>,
    pub blank_lines: Option<usize>,
}

impl Default for FormatConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            indent_width: None,
            blank_lines: None,
        }
    }
}

impl FormatConfig {
    fn apply_section(&mut self, value: &Value) {
        if let Some(enabled) = value.as_bool() {
            self.enabled = enabled;
            return;
        }
        if let Some(obj) = value.as_object() {
            if let Some(enabled) = get_bool(obj, "enabled") {
                self.enabled = enabled;
            }
            if let Some(width) = get_u64(obj, "indentWidth")
                && width > 0
            {
                self.indent_width = Some(width as usize);
            }
            if let Some(lines) = get_u64(obj, "blankLines") {
                self.blank_lines = Some(lines as usize);
            }
        }
    }
}

fn get_bool(obj: &serde_json::Map<String, Value>, key: &str) -> Option<bool> {
    get_value(obj, key).and_then(Value::as_bool)
}

fn get_u64(obj: &serde_json::Map<String, Value>, key: &str) -> Option<u64> {
    get_value(obj, key).and_then(Value::as_u64)
}

fn get_value<'a>(obj: &'a serde_json::Map<String, Value>, key: &str) -> Option<&'a Value> {
    let snake = to_snake_case(key);
    let kebab = to_kebab_case(key);
    obj.get(key)
        .or_else(|| obj.get(&snake))
        .or_else(|| obj.get(&kebab))
}

fn to_snake_case(key: &str) -> String {
    let mut output = String::with_capacity(key.len());
    for (idx, ch) in key.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if idx > 0 {
                output.push('_');
            }
            output.push(ch.to_ascii_lowercase());
        } else {
            output.push(ch);
        }
    }
    output
}

fn to_kebab_case(key: &str) -> String {
    let mut output = String::with_capacity(key.len());
    for (idx, ch) in key.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if idx > 0 {
                output.push('-');
            }
            output.push(ch.to_ascii_lowercase());
        } else {
            output.push(ch);
        }
    }
    output
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

    pub fn format_indent_width(&self) -> Option<usize> {
        self.format.indent_width
    }

    pub fn format_blank_lines(&self) -> Option<usize> {
        self.format.blank_lines
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
