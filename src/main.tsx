import React from "react";
import ReactDOM from "react-dom/client";
import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import App from "./App";
import "./index.css";

// 터미널 화면(xterm)은 터미널 창에서만 읽어 들인다.
const TerminalApp = React.lazy(() => import("./Terminal"));

// 창 이름이 "terminal"이면 넷엑스 터미널 창이다(같은 화면 묶음을 쓴다).
const terminal = isTauri() ? getCurrentWindow().label === "terminal" : new URLSearchParams(location.search).has("terminal");

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    {terminal ? (
      <React.Suspense fallback={null}>
        <TerminalApp />
      </React.Suspense>
    ) : (
      <App />
    )}
  </React.StrictMode>,
);
