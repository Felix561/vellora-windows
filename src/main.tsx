import React from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import App from "./App";
import { HaloView } from "./views/HaloView";
import { OverlayView } from "./views/OverlayView";
import "./styles.css";

const windowLabel = getCurrentWindow().label;

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    {windowLabel === "assist-halo" ? <HaloView /> : windowLabel === "overlay" ? <OverlayView /> : <App />}
  </React.StrictMode>,
);
