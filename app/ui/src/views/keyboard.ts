/**
 * Teclado do jogo numa janela do app (viewer do convidado ou painel do Host): as teclas vão para
 * `onKey`, menos enquanto o campo de chat tem o teclado. `Enter` sem foco foca o campo; focar o
 * campo (por `Enter` ou clique) e perder o foco da janela soltam o que estava pressionado.
 * Devolve o `dispose`, que para de escutar e solta as teclas.
 */
export function bindGameKeys(opts: {
  input: HTMLInputElement;
  onKey: (code: string, down: boolean) => void;
}): () => void {
  const { input, onKey } = opts;
  const pressed = new Set<string>();
  // Digitando no chat o teclado é do campo: nada vai para o `onKey`.
  const typing = (e: KeyboardEvent) => document.activeElement === input || e.target === input;
  const release = () => {
    for (const c of pressed) onKey(c, false);
    pressed.clear();
  };
  const onDown = (e: KeyboardEvent) => {
    if (typing(e)) return;
    if (e.key === "Enter") {
      // Solta o que estava pressionado antes de o campo ficar com o teclado.
      release();
      input.focus();
      return;
    }
    pressed.add(e.code);
    onKey(e.code, true);
  };
  const onUp = (e: KeyboardEvent) => {
    if (typing(e) || e.key === "Enter") return;
    pressed.delete(e.code);
    onKey(e.code, false);
  };
  input.addEventListener("focus", release);
  window.addEventListener("keydown", onDown);
  window.addEventListener("keyup", onUp);
  window.addEventListener("blur", release);
  return () => {
    input.removeEventListener("focus", release);
    window.removeEventListener("keydown", onDown);
    window.removeEventListener("keyup", onUp);
    window.removeEventListener("blur", release);
    release();
  };
}
