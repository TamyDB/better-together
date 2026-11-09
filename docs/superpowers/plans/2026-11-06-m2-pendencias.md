# M2: pendências para os próximos marcos

Saldo da revisão final do M2. Registro curto do que ficou de fora, por marco.

## Portões do M6 (antes de abrir a sinalização ao público)

- Teto de conexões por IP e de sockets ociosos fora de sala.
- IP real atrás de proxy: aceitar `X-Forwarded-For` só vindo do proxy, ou TLS no processo.
- `Signal` não tem rate limit e a fila cheia derruba o destino, então um convidado consegue
  derrubar o Host. Passar a derrubar o remetente.

## M3 (vídeo)

- Validar o SDP vindo de convidado quando entrar vídeo.
- `WDA_EXCLUDEFROMCAPTURE` na overlay do Host.
- Mover a lembrança de reconexão (`bt.last`) do `sessionStorage` para o Rust.

## Adiados

- Vaga presa até 45 s na sinalização.
- O relógio de render reancora sem suavizar (olhar no E2E).
- Roster inicial do Host ausente na overlay.
- A bandeja fica depois do reset.
