import React from "react";
import ReactDOM from "react-dom/client";
import { HashRouter } from "react-router-dom";
import App from "./App";
import "./styles/globals.css";

// XandSuite is a desktop application built with Tauri.
// It requires the Tauri WebView runtime to function — it cannot run in a regular browser.
const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

if (!isTauri) {
  document.getElementById("root")!.innerHTML = `
    <div style="
      min-height: 100vh;
      display: flex;
      flex-direction: column;
      align-items: center;
      justify-content: center;
      background: #0f0f13;
      color: #cdd6f4;
      font-family: system-ui, sans-serif;
      text-align: center;
      padding: 2rem;
      gap: 1rem;
    ">
      <svg width="56" height="56" viewBox="0 0 24 24" fill="none" stroke="#89b4fa" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
        <rect x="2" y="3" width="20" height="14" rx="2"/><path d="M8 21h8M12 17v4"/>
      </svg>
      <h1 style="font-size: 1.5rem; font-weight: 700; margin: 0;">XandSuite is a desktop app</h1>
      <p style="color: #a6adc8; max-width: 420px; margin: 0; line-height: 1.6;">
        This application requires the Tauri desktop runtime and cannot run in a web browser.
        Please download and install the desktop application.
      </p>
      <a
        href="https://github.com/XandNet/XandSuite/releases"
        style="
          margin-top: 0.5rem;
          padding: 0.6rem 1.4rem;
          background: #89b4fa;
          color: #1e1e2e;
          border-radius: 8px;
          font-weight: 600;
          text-decoration: none;
          font-size: 0.9rem;
        "
      >Download XandSuite</a>
    </div>
  `;
} else {
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      <HashRouter>
        <App />
      </HashRouter>
    </React.StrictMode>
  );
}
