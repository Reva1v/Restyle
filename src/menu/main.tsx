import React from "react";
import ReactDOM from "react-dom/client";
import MenuApp from "./MenuApp";
import "../index.css";

document.addEventListener("contextmenu", (e) => e.preventDefault());

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <MenuApp />
  </React.StrictMode>,
);
