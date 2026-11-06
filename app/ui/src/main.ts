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
// O autorepeat é repassado: ele mantém o input "fresco" enquanto a tecla está segurada.
const forwardKey = (down: boolean) => (e: KeyboardEvent) => {
  if (down && e.ctrlKey && e.shiftKey && e.code === "KeyP") {
    if (!e.repeat) invoke("toggle_pet_mode");
    return;
  }
  invoke("dev_key", { code: e.code, down });
};
window.addEventListener("keydown", forwardKey(true));
window.addEventListener("keyup", forwardKey(false));
// Sem foco, os key-ups não chegam: solta tudo para o bichinho não ficar andando.
window.addEventListener("blur", () => {
  for (const code of ["KeyW", "KeyA", "KeyS", "KeyD", "ShiftLeft"]) invoke("dev_key", { code, down: false });
});
