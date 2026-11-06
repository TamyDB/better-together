# Desktop Multiplayer: Spec de Design (v1)

## Contexto
O repositório `desktop-multiplayer` só tem o `CLAUDE.md` com a ideia inicial: um app Windows, no estilo Desktop Goose, em que um Host compartilha o desktop e até 3 amigos remotos andam por ele como bichinhos (Pato, Capivara, Cavalo, Galinha), interagindo, usando emotes e conversando. Esta spec fecha as decisões arquiteturais tomadas no brainstorming e serve de base para os planos de implementação de cada marco.

**Próximo passo:** após a aprovação desta spec, criar o plano de implementação do **M1** via `superpowers:writing-plans`.

## Decisões (validadas com o usuário)
| Tema | Decisão |
|---|---|
| Propósito | Casual, para grupo de amigos. Diversão acima de tudo |
| Interação com o sistema | "Travessuras leves": nunca clicar nem digitar nos apps do Host |
| Visão do convidado | Stream de vídeo da tela do Host (WebRTC, 720p a 30 fps) |
| Conexão | P2P WebRTC em estrela (Host↔cada convidado) + servidor leve de sinalização/lobby + TURN de fallback |
| Cliente | App único Windows (Tauri), que funciona como Host ou Convidado. Todos instalam |
| Pet do Host | Sim. Hotkey global liga o "modo bichinho" (WASD). Fora dele, idle autônomo |
| Stack | Rust + **Tauri 2** (WebView2 cuida de WebRTC e vídeo; Rust cuida de regras, Win32 e simulação) |
| Arte | Placeholders no MVP. Atlas no formato Aseprite JSON, 64×64 |

## Arquitetura

### Estrutura (Cargo workspace + pnpm)
```
crates/protocol/   Mensagens serde+bincode, PROTOCOL_VERSION, tipos compartilhados
crates/sim/        Simulação pura e determinística a 60 Hz (física, animais, emotes, estado). Sem I/O
crates/mischief/   trait DesktopEffector + Win32Effector (windows-rs) + FakeEffector; HostPolicy
crates/signaling/  axum + WebSocket: salas "PATO-4821", relay de SDP/ICE, código válido enquanto a sessão existir, rate limit
app/src-tauri/     Papel Host/Convidado, loop da sim, hotkeys globais, janelas, IPC
app/ui/            TS + Vite: lobby, seleção de animal, chat, WebRTC, render dos sprites em canvas
assets/            Sprite sheets (placeholders)
```
**Fronteiras:** `sim` não conhece rede nem Windows. `mischief` é o único ponto que toca o sistema do Host. O TS só transporta bytes opacos e renderiza, sem regra de negócio.

### Janelas
- **Host overlay:** tela cheia, transparente, topmost, click-through (`WS_EX_LAYERED|WS_EX_TRANSPARENT|WS_EX_TOOLWINDOW`) e `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)` para ficar fora do vídeo.
- **Host painel** (bandeja): código da sala, jogadores, permissões, kick, botão de pânico.
- **Convidado viewer:** vídeo do Host + canvas de sprites por cima + chat. Input WASD e teclas de ação.

### Fluxo de dados
1. O Host cria a sala e recebe o código. O convidado entra com o código e a sinalização troca SDP/ICE. O WebRTC sobe P2P, ou via TURN se o P2P falhar.
2. Handshake: `Hello{protocol_version, name, desired_animal}` → `Welcome{player_id, animal, initial_snapshot}`. O animal é único: se o desejado estiver ocupado, o convidado escolhe outro no lobby.
3. Canais por par:
   - Vídeo (MediaTrack): tela do monitor primário, bitrate adaptativo do Chromium
   - `state` (DC não confiável, sem ordem): Host→Convidado, snapshots a 20 Hz
   - `input` (DC não confiável): Convidado→Host, input a 60 Hz com `seq`
   - `events` (DC confiável e ordenado): chat, emotes, `MischiefRequest`, join/leave, kick
4. Caminho: tecla → TS → IPC Tauri → Rust (bincode) → DC → TS do Host → IPC → `sim`.
5. **O Host é a autoridade.** O convidado roda a mesma `sim` para prever o próprio bichinho e reconcilia pelo `last_processed_seq`. Os outros bichinhos são interpolados com cerca de 100 ms de buffer.
6. Coordenadas normalizadas em [0,1] do monitor primário do Host. Multi-monitor fica fora do MVP.

### Travessuras (MVP)
| Travessura | Mecânica | Limites padrão |
|---|---|---|
| Arrastar janela | O Host envia só os retângulos anônimos (`EnumWindows`). O bichinho morde a barra de título e o Host move com `SetWindowPos` | Ignora janelas maximizadas, em tela cheia, do sistema e elevadas. 1 janela por bichinho. Máx. 5 s |
| Puxar cursor | `SetCursorPos` ao longo de um trajeto | Máx. 3 s. Cooldown de 30 s por jogador. Movimento forte do Host solta |
| Deixar coisas | Pegadas, notas e imagens de uma biblioteca embutida, desenhadas na overlay | TTL de 60 s. Máx. 20 itens. Sem upload |

Fluxo: `MischiefRequest` → `HostPolicy` (permissão, cooldown, alvo válido) → `DesktopEffector` → evento de resultado para todos.

### Segurança e controles do Host
- Botão de pânico com hotkey global (`Ctrl+Alt+F12`): congela travessuras, esconde a overlay e solta janela e cursor.
- Permissões por travessura (para todos ou por jogador), kick, trancar sala, pausa automática quando o Host entra em tela cheia.
- Convidados nunca recebem títulos de janela nem nomes de processo. O Host valida o esquema de toda mensagem e aplica rate limit por canal.
- Código da sala com cerca de 20 bits, válido enquanto a sessão existir (o Host conectado). Rate limit por IP na sinalização, que é a defesa contra quem tenta adivinhar códigos. DTLS do WebRTC para criptografia ponta a ponta.

### Erros e resiliência
- Convidado cai: o bichinho "dorme" por 30 s esperando a reconexão, depois é removido.
- Host cai: fim da sessão e tela "O Host saiu".
- P2P falha: TURN. TURN falha: erro claro na tela.
- Versão do protocolo incompatível: conexão recusada com aviso "atualize o app".
- Vídeo degradado: o Chromium ajusta sozinho e os sprites seguem nítidos, porque não fazem parte do vídeo.

### Requisitos e limites
- Upload do Host: cerca de 6 a 8 Mbps (3 streams de 720p a 30 fps). Documentar no app.
- Windows 10 2004+ (necessário para `WDA_EXCLUDEFROMCAPTURE`) com WebView2.

## Testes
- `protocol`/`sim`: unitários + proptest (round-trip, determinismo, reconciliação). Rodam no Linux.
- `mischief`: `HostPolicy` testada com `FakeEffector`. `Win32Effector` com integração só no Windows.
- `signaling`: integração com axum (criar, entrar, encerrar a sala, rate limit).
- E2E: modo `--loopback` (Host + 3 convidados falsos em uma máquina).
- CI: GitHub Actions `ubuntu-latest` (crates puros) + `windows-latest` (Win32 + build Tauri). O desenvolvimento é no Fedora, então os testes manuais precisam de VM ou PC Windows.

## Marcos (cada um com plano próprio via writing-plans)
1. **M1 Fundação:** workspace, `protocol`, `sim`, overlay do Host com 1 bichinho local (hotkey + WASD)
2. **M2 Rede:** sinalização, salas, WebRTC, DataChannels, lobby com seleção de animal, 4 bichinhos sincronizados
3. **M3 Vídeo:** stream da tela, overlay excluída da captura, viewer do convidado
4. **M4 Social:** chat + emotes
5. **M5 Travessuras:** as 3 travessuras, painel de permissões, pânico
6. **M6 Distribuição:** instalador do Tauri, auto-update, deploy da sinalização + coturn

## Fora do escopo (v1)
Multi-monitor, upload de imagens, controle real de mouse/teclado, cliente via navegador, mais de 4 jogadores, interações físicas entre bichinhos (empurrar/abraçar), Mac/Linux como Host.

## Verificação (desta etapa)
- A spec existe em `docs/superpowers/specs/2026-11-05-desktop-multiplayer-design.md` e foi commitada (`git log` mostra o commit).
- Autorrevisão da spec: sem TBD, sem contradições, escopo dividido em marcos.
- O usuário revisa e aprova a spec. Só então vem o `writing-plans` para o M1.
