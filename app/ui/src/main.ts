import "./ui.css";

const view = new URLSearchParams(location.search).get("view");

if (view === "overlay") {
  document.documentElement.classList.add("overlay");
  void import("./views/overlay").then((m) => m.startOverlay());
} else {
  void import("./views/main-window").then((m) => m.startMainWindow());
}
