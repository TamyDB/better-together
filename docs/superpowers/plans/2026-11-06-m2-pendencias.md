# M2: pendências para os próximos marcos

Saldo da revisão final do M2. Registro curto do que ficou de fora, por marco.

## Portões do M6 (antes de abrir a sinalização ao público)

- Teto de conexões por IP e de sockets ociosos fora de sala.
- IP real atrás de proxy: aceitar `X-Forwarded-For` só vindo do proxy, ou TLS no processo.
- `Signal` não tem rate limit e a fila cheia derruba o destino, então um convidado consegue
  derrubar o Host. Passar a derrubar o remetente.

## M3 (vídeo): resolvido na branch `m3-video`

- ~~Validar o SDP vindo de convidado quando entrar vídeo.~~ (`net/sdp.ts`)
- ~~`WDA_EXCLUDEFROMCAPTURE` na overlay do Host.~~ (`capture.rs`; confirmar no E2E)
- ~~Mover a lembrança de reconexão (`bt.last`) do `sessionStorage` para o Rust.~~

## Adiados

- Vaga presa até 45 s na sinalização.
- O relógio de render reancora sem suavizar (olhar no E2E).
- ~~Roster inicial do Host ausente na overlay.~~ (M3)
- ~~A bandeja fica depois do reset.~~ (M3)

## Saldo da revisão final do M3

- Windows anterior ao 10 2004: `WDA_EXCLUDEFROMCAPTURE` age como `WDA_MONITOR` (vídeo preto).
  Checar o build (`RtlGetVersion` ≥ 19041) ou exigir o mínimo no instalador (M6).
- A overlay pode pedir `host_roster` antes do `set_mode` e receber vazio (janela estreita):
  repetir o pedido quando vier vazio.
- `captureWarning` só é recalculado ao mudar o estado do vídeo (janela capturada
  redimensionada não atualiza o aviso).
- "Compartilhar tela" sem trava com o seletor já aberto (ver o que o WebView2 faz no E2E).
- `hostFlow` em `main-window.ts` cresceu: extrair o painel de vídeo do Host no M4.
- `STALL_MS` do aviso de vídeo pausado precisa de calibração no E2E. Se der falso positivo
  com a tela parada, sinalizar a pausa pelo canal `events` no M4.
