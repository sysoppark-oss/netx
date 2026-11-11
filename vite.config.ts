import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
// @ts-expect-error type error without @types/node package
import process from "node:process";
const host = process.env.TAURI_DEV_HOST;

// NetX는 보안 설정(tauri.conf.json의 freezePrototype)으로 Object.prototype을 얼린다.
// xterm에는 보통 객체에 `toString`을 대입하는 곳이 한 군데 있어 그 상태에서는 읽히다가 멈춘다.
// 그 대입을 속성 정의로 바꾸고, 같은 종류의 대입이 더 있으면 빌드를 멈춘다(xterm 판을 올릴 때 여기서 걸린다).
const INHERITED = "toString|valueOf|constructor|hasOwnProperty|isPrototypeOf|propertyIsEnumerable|toLocaleString";
function frozenPrototypeSafe() {
  return {
    name: "netx-frozen-prototype-safe",
    transform(this: { error(message: string): never }, code: string, id: string) {
      if (!id.includes("@xterm/")) return null;
      const fixed = code.replace(new RegExp(`([\\w$]+)\\.(${INHERITED})=([\\w$]+);`, "g"), 'Object.defineProperty($1,"$2",{value:$3,writable:true,configurable:true});');
      const left = fixed.match(new RegExp(`\\.(${INHERITED})=(?![=>])`));
      if (left) this.error(`${id}: 얼린 Object.prototype에서 실패하는 대입(${left[0]})이 있습니다. vite.config.ts의 frozenPrototypeSafe를 확인하세요.`);
      return fixed === code ? null : { code: fixed, map: null };
    },
  };
}

// https://vite.dev/config/
export default defineConfig(() => ({
  plugins: [react(), tailwindcss(), frozenPrototypeSafe()],

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));
