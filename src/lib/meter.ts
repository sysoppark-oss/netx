/** 진행 표시를 음향 장비의 레벨 미터 모양으로 그릴 때 쓰는 계산. */

/** 칸 수: 초록 9, 노랑 2, 빨강 6 (합쳐 17). */
export const METER_ZONES = { green: 9, yellow: 2, red: 6 } as const;
export const METER_TOTAL = METER_ZONES.green + METER_ZONES.yellow + METER_ZONES.red;

export type MeterTone = "green" | "yellow" | "red";

export function meterTone(index: number): MeterTone {
  if (index < METER_ZONES.green) return "green";
  if (index < METER_ZONES.green + METER_ZONES.yellow) return "yellow";
  return "red";
}

/**
 * 세 구간의 끝나는 시각(초).
 * 예상 시간을 알면: 초록은 예상 시간까지, 노랑은 예상을 조금 넘긴 데까지, 빨강은 자동 중단까지.
 * 모르면: 제한 시간의 절반, 3분의 2, 끝.
 */
export function meterMarks(estimateSecs: number | null, limitSecs: number): [number, number, number] {
  const limit = Math.max(3, limitSecs);
  if (estimateSecs === null || !(estimateSecs > 0)) {
    return [limit / 2, (limit * 2) / 3, limit];
  }
  const green = Math.min(estimateSecs, limit * 0.8);
  const yellow = Math.min(limit * 0.9, Math.max(green * 1.5, green + 5));
  return [green, yellow, limit];
}

/** 지금까지 켜진 칸 수 (0 ~ 17). 실행 중에는 적어도 한 칸은 켠다. */
export function meterLit(elapsedSecs: number, estimateSecs: number | null, limitSecs: number): number {
  const [green, yellow, red] = meterMarks(estimateSecs, limitSecs);
  const t = Math.max(0, elapsedSecs);
  const part = (from: number, to: number, cells: number) => (to <= from ? cells : Math.min(1, Math.max(0, (t - from) / (to - from))) * cells);
  const lit = part(0, green, METER_ZONES.green) + part(green, yellow, METER_ZONES.yellow) + part(yellow, red, METER_ZONES.red);
  return Math.min(METER_TOTAL, Math.max(1, Math.ceil(lit - 1e-9)));
}
