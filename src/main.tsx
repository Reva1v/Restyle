import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { commands, events } from "./lib/ipc";
import "./index.css";

// Оверлей: отключаем контекстное меню WebView, чтобы не всплывало поверх панели.
document.addEventListener("contextmenu", (e) => e.preventDefault());

// Тема: итоговый режим (dark/light) перекрашивает акриловый тинт окна.
const mq = window.matchMedia("(prefers-color-scheme: dark)");
let theme = "system";
function applyTheme() {
  const dark = theme === "system" ? mq.matches : theme !== "light";
  document.documentElement.dataset.theme = dark ? "dark" : "light";
  void commands.setOverlayTint(dark);
}
mq.addEventListener("change", () => theme === "system" && applyTheme());
void commands.getSettings().then((s) => {
  theme = s.theme;
  applyTheme();
});
void events.onSettingsChanged((s) => {
  theme = s.theme;
  applyTheme();
});

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
