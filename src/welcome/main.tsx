import React from "react";
import ReactDOM from "react-dom/client";
import WelcomeApp from "./WelcomeApp";
import "../index.css";

document.addEventListener("contextmenu", (e) => e.preventDefault());

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <WelcomeApp />
  </React.StrictMode>,
);
