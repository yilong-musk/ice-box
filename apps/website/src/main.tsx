import React from "react";
import { createRoot } from "react-dom/client";
import App from "../../desktop/src/App";
import "../../desktop/src/index.css";

createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
