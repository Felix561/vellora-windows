import React from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import App from "./App";
import { OverlayView } from "./views/OverlayView";
import "./styles.css";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    {getCurrentWindow().label === "overlay" ? <OverlayView /> : <App />}
  </React.StrictMode>,
);
