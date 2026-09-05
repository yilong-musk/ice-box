import React from "react";
import { createRoot } from "react-dom/client";
import App from "../../../apps/desktop/src/App";
import "./demo.css";

createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
