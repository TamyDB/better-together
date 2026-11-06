import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { startRenderer } from "./render/canvas";
import type { Snapshot } from "./types";

const canvas = document.createElement("canvas");
document.body.appendChild(canvas);
const renderer = startRenderer(canvas);

listen<Snapshot>("sim://snapshot", (e) => renderer.update(e.payload));

// Modo dev (DM_DEV_WINDOW=1): a janela tem foco, então o teclado vem da webview.
// Na overlay normal ela nunca recebe foco e o hook do Windows cuida do teclado.
const forwardKey = (down: boolean) => (e: KeyboardEvent) => {
  if (e.repeat) return;
  if (down && e.ctrlKey && e.shiftKey && e.code === "KeyP") {
    invoke("toggle_pet_mode");
    return;
  }
  invoke("dev_key", { code: e.code, down });
};
window.addEventListener("keydown", forwardKey(true));
window.addEventListener("keyup", forwardKey(false));
