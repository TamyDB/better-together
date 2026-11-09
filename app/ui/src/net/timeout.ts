export const CONNECT_TIMEOUT_MS = 15000;

/** Rejeita com `message` se `p` não resolver em `ms`. */
export function withTimeout<T>(p: Promise<T>, ms: number, message: string): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(message)), ms);
    p.then(
      (v) => {
        clearTimeout(timer);
        resolve(v);
      },
      (e) => {
        clearTimeout(timer);
        reject(e);
      },
    );
  });
}

export interface Deadline {
  arm(): void;
  settle(): void;
}

/** Prazo de uma espera: conta a partir de `arm`; `settle` a encerra, mesmo antes de armar. */
export function deadline(ms: number, onExpire: () => void): Deadline {
  let timer: ReturnType<typeof setTimeout> | null = null;
  let settled = false;
  return {
    arm() {
      if (settled || timer !== null) return;
      timer = setTimeout(() => {
        settled = true;
        timer = null;
        onExpire();
      }, ms);
    },
    settle() {
      settled = true;
      if (timer !== null) clearTimeout(timer);
      timer = null;
    },
  };
}
