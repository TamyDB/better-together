import { listen } from "@tauri-apps/api/event";
import { startRenderer } from "./render/canvas";
import type { Snapshot } from "./types";

const canvas = document.createElement("canvas");
document.body.appendChild(canvas);
const renderer = startRenderer(canvas);

listen<Snapshot>("sim://snapshot", (e) => renderer.update(e.payload));
