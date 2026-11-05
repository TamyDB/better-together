### Visão do projeto
Fazer um aplicativo para Windows, que rode em cima da área de trabalho, [assim como o Desktop Goose, pesquisa pra entender melhor] em que um usuário é o Host da sessão do Desktop, e outros usuários vão poder se mover no Desktop do Host e interagirem com apps, e consigo mesmos, usar emotes, conversar, entre outras interações.

### Features
- Sessão inicial com 4 jogadores
- 4 opções de animais selecionaveis [Pato, Capivara, Cavalo e Galinha]
- 1 Usuário Host e outros 3 se conectando remotamente

### Decisões arquiteturais (v1)
Spec completa: `docs/superpowers/specs/2026-11-05-desktop-multiplayer-design.md`

- **Público:** grupo de amigos, uso casual.
- **Stack:** Rust + Tauri 2. O WebView2 cuida de WebRTC e vídeo; o Rust cuida de simulação, regras, Win32 e overlay. O TS só transporta bytes e renderiza.
- **Rede:** WebRTC P2P em estrela (Host↔cada convidado) + servidor de sinalização (axum, salas "PATO-4821") + TURN de fallback.
- **Autoridade:** o Host simula a 60 Hz e envia snapshots a 20 Hz. O convidado manda inputs e prevê o próprio bichinho.
- **Visão:** o convidado vê a tela do Host por stream de vídeo (720p a 30 fps). Os bichinhos NÃO vão no vídeo (overlay com `WDA_EXCLUDEFROMCAPTURE`) e são desenhados localmente por cima.
- **App único** Windows, que funciona como Host ou Convidado. Todos instalam.
- **Pet do Host:** hotkey global liga o "modo bichinho" (WASD).
- **Travessuras leves apenas:** arrastar janela, puxar cursor, deixar coisas na tela. Nunca clicar nem digitar nos apps do Host.
- **O Host manda:** botão de pânico, permissões por travessura, kick. Convidados nunca recebem títulos de janela nem nomes de processo.
- **Arte:** placeholders no MVP (atlas Aseprite JSON, 64×64).

### Estrutura
`crates/{protocol,sim,mischief,signaling}`, `app/src-tauri`, `app/ui`, `assets/`. A `sim` é pura (sem I/O). A `mischief` é o único ponto que toca o sistema do Host (trait `DesktopEffector` + fake para testes).

### Ambiente de desenvolvimento
O desenvolvimento é no Fedora. Os crates puros são testados no Linux; Win32, overlay e build do Tauri exigem Windows (VM/PC ou CI `windows-latest`).

### Marcos
M1 Fundação → M2 Rede → M3 Vídeo → M4 Social → M5 Travessuras → M6 Distribuição. Cada marco tem seu próprio plano em `docs/superpowers/plans/`.
