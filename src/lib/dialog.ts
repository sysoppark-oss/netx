// 겹침 창의 공통 동작: 열면 초점이 창 안으로 오고, Tab이 창 안에서 돌고, Esc로 닫히고, 닫으면 초점이 연 단추로 돌아간다.
// 쓰는 법: const dialog = useDialog(onClose);  <div {...dialog} aria-label="창 이름" className="fixed inset-0 …">
import { useEffect, useRef, type KeyboardEvent } from "react";

const FOCUSABLE = 'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), summary, [tabindex]:not([tabindex="-1"])';

function focusables(root: HTMLElement): HTMLElement[] {
  return Array.from(root.querySelectorAll<HTMLElement>(FOCUSABLE)).filter((el) => el.offsetParent !== null || el === document.activeElement);
}

export function useDialog(onClose: () => void, options: { escape?: boolean } = {}) {
  const ref = useRef<HTMLDivElement>(null);
  const escape = options.escape ?? true;

  useEffect(() => {
    const before = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    // 창 자체에 초점을 둔다(첫 단추에 두면 Enter로 잘못 누를 수 있다). 다음 Tab이 창 안의 첫 조작 요소로 간다.
    ref.current?.focus({ preventScroll: true });
    return () => {
      // 이 창 위에 다른 창이 떠 있다가 닫힌 경우가 아니면, 연 자리로 초점을 돌려준다.
      if (before && document.contains(before)) before.focus({ preventScroll: true });
    };
  }, []);

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const root = ref.current;
    if (!root) return;
    if (e.key === "Escape") {
      // 한글 조합을 취소하는 Esc는 창을 닫지 않는다. 겹쳐 뜬 창은 맨 위 것만 닫힌다.
      if (e.nativeEvent.isComposing) return;
      e.stopPropagation();
      if (escape) onClose();
      return;
    }
    if (e.key !== "Tab") return;
    e.stopPropagation();
    const items = focusables(root);
    if (items.length === 0) {
      e.preventDefault();
      return;
    }
    const first = items[0];
    const last = items[items.length - 1];
    const active = document.activeElement;
    if (e.shiftKey && (active === first || active === root)) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && active === last) {
      e.preventDefault();
      first.focus();
    } else if (active instanceof HTMLElement && !root.contains(active)) {
      e.preventDefault();
      first.focus();
    }
  };

  return { ref, role: "dialog" as const, "aria-modal": true as const, tabIndex: -1, onKeyDown };
}
