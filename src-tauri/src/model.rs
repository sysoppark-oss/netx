//! 화면·AI·백엔드가 공유하는 데이터 형식. 기준: docs/DEV_PLAN_v0.1.md 4장

use serde::{Deserialize, Serialize};

/// 사용자 등급. 뒤로 갈수록 권한이 넓다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    #[default]
    General,
    Junior,
    Middle,
    Senior,
    Op,
}

/// 위험도. `Forbidden`은 어떤 경우에도 실행하지 않는다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Risk {
    #[default]
    Safe,
    Caution,
    Danger,
    Forbidden,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Engine {
    Wsl,
    Windows,
    Builtin,
}

/// 옵션 값과 위치 인자의 종류. 종류마다 검사 규칙이 다르다(security/values.rs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueType {
    /// 값이 없는 켜고 끄는 옵션
    Boolean,
    Number,
    Float,
    String,
    Enum,
    Host,
    Url,
    Port,
    PortOrRange,
    /// 포트 또는 범위. 내 PC 조회처럼 범위가 넓어도 위험하지 않은 곳에 쓴다.
    PortRange,
    Cidr,
    /// 호스트 또는 CIDR 대역
    Target,
    /// 작업 폴더 안의 파일 이름
    Path,
    Iface,
    /// tcpdump·ss 필터식
    Expr,
    /// curl -w 형식 문자열
    Format,
    /// 100M, 1G 같은 크기·속도
    Rate,
    /// dig 인자: 이름, 레코드 종류, @서버
    DigArg,
    /// lsof -i 주소 지정
    LsofInet,
    /// 포트 목록: 22,80,443 또는 20-25 (내장 포트 스캔)
    PortList,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OptionSpec {
    pub flag: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(rename = "type")]
    pub kind: ValueType,
    pub label: String,
    #[serde(default)]
    pub risk: Risk,
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
    #[serde(default)]
    pub values: Vec<String>,
    /// 이 값이면 옵션 위험도를 적용하지 않는다(예: curl -o /dev/null).
    #[serde(default)]
    pub allow_values: Vec<String>,
    /// 이 글자로 시작하는 값은 거부한다(예: curl에서 @는 파일 읽기).
    #[serde(default)]
    pub deny_prefix: Vec<String>,
    /// 값을 생략할 수 있는 옵션(예: lsof -i)
    #[serde(default)]
    pub optional_value: bool,
    #[serde(default)]
    pub min_level: Option<Level>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PositionalSpec {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: ValueType,
    pub label: String,
    #[serde(default)]
    pub required: bool,
    /// 남은 인자를 모두 받는다. 마지막 위치 인자만 가능.
    #[serde(default)]
    pub variadic: bool,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
    #[serde(default)]
    pub values: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolSchema {
    pub tool: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    pub engines: Vec<Engine>,
    #[serde(default)]
    pub needs_root: bool,
    #[serde(default)]
    pub streaming: bool,
    /// `--`로 옵션 끝을 표시할 수 있는 도구
    #[serde(default)]
    pub end_of_options: bool,
    #[serde(default)]
    pub risk: Risk,
    #[serde(default)]
    pub min_level: Level,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub viz: Option<String>,
    pub options: Vec<OptionSpec>,
    #[serde(default)]
    pub positional: Vec<PositionalSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecipeInput {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: ValueType,
    pub label: String,
    #[serde(default)]
    pub example: Option<String>,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
    /// 고를 수 있는 값(type이 enum일 때)과 화면에 보일 이름
    #[serde(default)]
    pub values: Vec<String>,
    #[serde(default)]
    pub value_labels: Vec<String>,
    /// 비워 두면 쓰는 값
    #[serde(default)]
    pub default: Option<String>,
    /// 비워 둘 수 있는 입력
    #[serde(default)]
    pub optional: bool,
    /// 목록을 실행 중에 채우는 입력(예: "interfaces")
    #[serde(default)]
    pub source: Option<String>,
    /// 입력칸 아래에 늘 보이는 안내
    #[serde(default)]
    pub hint: Option<String>,
    /// 골라 쓸 수 있는 추천 값(직접 입력도 가능)과 화면에 보일 이름
    #[serde(default)]
    pub suggestions: Vec<String>,
    #[serde(default)]
    pub suggestion_labels: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    pub id: String,
    pub icon: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub min_level: Level,
    pub inputs: Vec<RecipeInput>,
    /// AI가 없을 때 규칙 엔진이 상황 입력과 맞춰 보는 낱말
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub parallel: bool,
    pub steps: Vec<PlanStep>,
}

/// 실행 계획의 한 단계. AI 또는 레시피가 만든다. 위험도·엔진은 검증기가 붙인다.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStep {
    #[serde(default)]
    pub order: Option<u32>,
    pub tool: String,
    pub args: Vec<String>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub expect: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionPlan {
    #[serde(default)]
    pub intent: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub target: Option<String>,
    pub steps: Vec<PlanStep>,
}

/// 계획의 출처. 레시피는 미리 검토된 조합이라 낮은 등급에서도 주의 옵션을 허용한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Recipe,
    /// AI가 만든 계획 (Phase 5에서 사용)
    #[allow(dead_code)]
    Ai,
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Confirm {
    None,
    /// 실행 전에 한 번 더 확인
    Click,
    /// Windows Hello 확인 (프로세스 종료 등, Phase 3 이후 사용)
    #[allow(dead_code)]
    Hello,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Block,
    Warn,
}

#[derive(Debug, Clone, Serialize)]
pub struct Issue {
    pub severity: Severity,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidatedStep {
    pub order: u32,
    pub tool: String,
    pub title: String,
    pub engines: Vec<Engine>,
    /// 실제로 실행할 인자 배열(도구 이름 제외)
    pub argv: Vec<String>,
    /// 화면 표시용 명령 문자열. 실행에는 쓰지 않는다.
    pub display: String,
    pub risk: Risk,
    pub needs_root: bool,
    pub streaming: bool,
    /// 예상 소요 시간(초). 알 수 없으면 없음.
    pub estimate_secs: Option<u32>,
    pub confirm: Confirm,
    pub blocked: bool,
    pub issues: Vec<Issue>,
    pub reason: Option<String>,
    pub expect: Option<String>,
    /// 사용자가 직접 추가한 단계
    pub manual: bool,
    /// 옵션별 풀이(옵션, 뜻)
    pub explain: Vec<Explain>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Explain {
    pub token: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidatedPlan {
    pub intent: String,
    pub summary: String,
    pub target: Option<String>,
    pub parallel: bool,
    pub risk: Risk,
    pub blocked: bool,
    pub steps: Vec<ValidatedStep>,
}
