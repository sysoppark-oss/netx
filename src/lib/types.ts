// 화면과 백엔드가 주고받는 데이터 형식.
// 기준: docs/DEV_PLAN_v0.1.md 4장

export type UserLevel = "general" | "junior" | "middle" | "senior" | "op";

export const USER_LEVELS: { id: UserLevel; label: string; hint: string }[] = [
  { id: "general", label: "일반사용자", hint: "쉬운 말 결과와 해결 방법만" },
  { id: "junior", label: "초급 엔지니어", hint: "옵션별 학습용 해설, 안전 옵션만" },
  { id: "middle", label: "중급 엔지니어", hint: "전체 옵션, 위험 옵션은 경고" },
  { id: "senior", label: "고급 엔지니어", hint: "전체 옵션과 직접 입력, 원본 출력 펼침" },
  { id: "op", label: "전산관리자(OP)", hint: "고급 + 감사 로그, 운영 기능" },
];

export type UiMode = "easy" | "expert";

export type EnvMode = "full" | "basic";

export interface AppInfo {
  name: string;
  version: string;
  os: string;
  arch: string;
}

export interface EnvironmentStatus {
  mode: EnvMode;
  wsl_installed: boolean;
  distros: string[];
  message: string;
}

export interface RecipeSummary {
  id: string;
  icon: string;
  title: string;
  input: string;
}

// Phase 2에서 recipes/*.json으로 옮긴다.
export const RECIPES: RecipeSummary[] = [
  { id: "web_down", icon: "🌐", title: "웹 장애 진단", input: "URL" },
  { id: "route_latency", icon: "📉", title: "라우팅 지연 분석", input: "목적지" },
  { id: "subnet_scan", icon: "🔍", title: "서브넷 장비 스캔", input: "대역" },
  { id: "port_hunt", icon: "🚪", title: "포트 점유 추적", input: "포트 번호" },
  { id: "bandwidth", icon: "🚀", title: "대역폭 측정", input: "iperf3 서버" },
  { id: "quick_capture", icon: "👁️", title: "트래픽 퀵 캡처", input: "인터페이스, 포트" },
];

// Phase 2에서 tools/*.json으로 옮긴다.
export const TOOLS: { id: string; title: string }[] = [
  { id: "ping", title: "ping / fping" },
  { id: "tcpdump", title: "tcpdump" },
  { id: "mtr", title: "mtr" },
  { id: "ss", title: "ss" },
  { id: "dig", title: "dig" },
  { id: "nc", title: "nc" },
  { id: "curl", title: "curl" },
  { id: "lsof", title: "lsof" },
  { id: "iperf3", title: "iperf3" },
];
