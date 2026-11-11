//! 앱 설정. AI 연결 정보는 설정 파일에, API 키는 Windows 자격 증명 저장소에 둔다.
//! API 키는 화면으로 다시 돌려주지 않는다(있는지 여부만 알려 준다).

use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    /// AI 사용 안 함. 키워드로 레시피를 찾아 준다.
    #[default]
    None,
    /// 로컬 Ollama (예: Tailscale로 연결한 미니 PC)
    Ollama,
    /// OpenAI 호환 방식의 온라인 API
    Openai,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Settings {
    #[serde(default)]
    pub provider: Provider,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub model: String,
    /// 온라인으로 보낼 때 내부 IP를 가린다.
    #[serde(default)]
    pub redact: bool,
    /// 새 버전을 자동으로 설치하지 않는다(알림만). 기본은 자동 설치.
    #[serde(default)]
    pub manual_update: bool,
}

const FILE: &str = "settings.json";
#[cfg(target_os = "windows")]
const KEY_SERVICE: &str = "NetX";
#[cfg(target_os = "windows")]
const KEY_USER: &str = "llm-api-key";

pub fn load(dir: &Path) -> Settings {
    std::fs::read_to_string(dir.join(FILE)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

pub fn save(dir: &Path, settings: &Settings) -> Result<(), String> {
    validate(settings)?;
    let text = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(dir).map_err(|e| format!("설정 폴더를 만들지 못했습니다: {e}"))?;
    std::fs::write(dir.join(FILE), text).map_err(|e| format!("설정을 저장하지 못했습니다: {e}"))
}

pub fn validate(s: &Settings) -> Result<(), String> {
    if s.provider == Provider::None {
        return Ok(());
    }
    let url = url::Url::parse(s.base_url.trim()).map_err(|_| "주소 형식이 아닙니다 (예: http://100.64.0.5:11434)".to_string())?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none_or(str::is_empty) {
        return Err("주소는 http 또는 https로 시작해야 합니다".into());
    }
    if s.model.trim().is_empty() || s.model.len() > 100 || s.model.chars().any(|c| c.is_control()) {
        return Err("모델 이름을 입력하세요".into());
    }
    Ok(())
}

#[cfg(target_os = "windows")]
pub fn api_key() -> Option<String> {
    keyring::Entry::new(KEY_SERVICE, KEY_USER).ok()?.get_password().ok().filter(|k| !k.is_empty())
}

/// 키를 저장한다. 빈 문자열이면 지운다.
#[cfg(target_os = "windows")]
pub fn set_api_key(key: &str) -> Result<(), String> {
    let entry = keyring::Entry::new(KEY_SERVICE, KEY_USER).map_err(|e| format!("자격 증명 저장소를 열지 못했습니다: {e}"))?;
    if key.is_empty() {
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(format!("API 키를 지우지 못했습니다: {e}")),
        }
    } else {
        entry.set_password(key).map_err(|e| format!("API 키를 저장하지 못했습니다: {e}"))
    }
}

// Windows가 아닌 환경(개발·테스트)에서는 환경 변수 NETX_LLM_API_KEY만 읽는다.
#[cfg(not(target_os = "windows"))]
pub fn api_key() -> Option<String> {
    std::env::var("NETX_LLM_API_KEY").ok().filter(|k| !k.is_empty())
}

#[cfg(not(target_os = "windows"))]
pub fn set_api_key(_key: &str) -> Result<(), String> {
    Err("API 키 저장은 Windows에서만 지원합니다".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_ai_settings() {
        let ok = Settings { provider: Provider::Ollama, base_url: "http://100.64.0.5:11434".into(), model: "llama3".into(), redact: false, manual_update: false };
        assert!(validate(&ok).is_ok());
        assert!(validate(&Settings::default()).is_ok());
        for (url, model) in [("", "m"), ("ftp://x", "m"), ("file:///etc/passwd", "m"), ("http://x", ""), ("not a url", "m")] {
            let s = Settings { provider: Provider::Openai, base_url: url.into(), model: model.into(), redact: true, manual_update: false };
            assert!(validate(&s).is_err(), "{url} {model}");
        }
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = std::env::temp_dir().join(format!("netx-settings-test-{}", std::process::id()));
        let s = Settings { provider: Provider::Ollama, base_url: "http://127.0.0.1:11434".into(), model: "phi3".into(), redact: true, manual_update: false };
        save(&dir, &s).unwrap();
        let back = load(&dir);
        assert_eq!(back.provider, Provider::Ollama);
        assert_eq!(back.model, "phi3");
        assert!(back.redact);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(load(&dir).provider, Provider::None);
    }
}
