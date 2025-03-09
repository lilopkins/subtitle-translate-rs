use serde::{Deserialize, Serialize};

#[derive(Serialize, Debug, Default)]
pub struct Query {
    pub q: String,
    pub source: String,
    pub target: String,
    pub alternatives: u32,
    pub format: Option<Format>,
    pub api_key: Option<String>,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    Text,
    Html,
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
pub enum TranslationResult {
    Ok(Translation),
    Err(TranslationError),
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Translation {
    pub translated_text: String,
    pub alternatives: Option<Vec<String>>,
    pub detected_language: Option<DetectedLanguage>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct DetectedLanguage {
    pub confidence: u8,
    pub language: String,
}

#[derive(Deserialize, Clone, Debug)]
pub struct TranslationError {
    pub error: String,
}
