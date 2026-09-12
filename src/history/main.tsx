import React from "react";
import ReactDOM from "react-dom/client";
import HistoryApp from "./HistoryApp";
import "../index.css";

document.addEventListener("contextmenu", (e) => e.preventDefault());

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <HistoryApp />
  </React.StrictMode>,
);
