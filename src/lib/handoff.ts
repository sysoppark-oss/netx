/** 넘겨받은 대상("주소", "주소:포트", "[IPv6]:포트", "도메인")을 주소와 포트로 나눈다. */
export function splitTarget(target: string): { host: string; port: string | null } {
  const v6 = /^\[([0-9a-fA-F:.]+)\]:(\d{1,5})$/.exec(target);
  if (v6) return { host: v6[1], port: v6[2] };
  const one = /^([^:]+):(\d{1,5})$/.exec(target);
  if (one) return { host: one[1], port: one[2] };
  return { host: target, port: null };
}

/**
 * 넘겨받은 대상을 그 진단의 어느 입력칸에 넣을지. 대상을 받는 칸이 없는 진단이면 빈 객체다.
 * 채우기만 한다. 실행은 사용자가 누른다.
 */
export function handoffInputs(recipeId: string, target: string): Record<string, string> {
  const { host, port } = splitTarget(target);
  if (!host) return {};
  switch (recipeId) {
    case "route_latency":
      return { target: host };
    case "quick_capture":
      return port ? { host, port } : { host };
    case "web_down": {
      const name = host.includes(":") ? `[${host}]` : host;
      if (port === "80") return { url: `http://${name}` };
      return { url: port && port !== "443" ? `https://${name}:${port}` : `https://${name}` };
    }
    default:
      return {};
  }
}
