# Roteiro de teste manual do M3 (vídeo)

Teste ponta a ponta no **Windows 10 2004+ ou 11** (VM ou PC), com WebView2. Ele absorve o
roteiro do M2 (`docs/e2e-m2.md`), que nunca rodou no Windows: faça as duas partes no mesmo
build.

Antes de começar:

- Dê push da branch `m3-video` e confira a CI `windows-latest` verde. Ela compila o código
  Win32 e roda `exclude_hwnd_sets_affinity`. Se esse teste falhar com erro de sessão ou DWM
  (e não num `assert`), é ambiente do runner, não bug.
- Gere o executável no próprio Windows: `pnpm --dir app tauri build --debug --no-bundle`.

Anote os números pedidos (marcados com 📏) no fim do arquivo ou no PR.

## 0. Spike: captura no WebView2 (5 min, faça primeiro)

1. `cargo run -p dm-signaling`
2. `$env:DM_DEV_WINDOW=1; .\target\debug\better-together.exe`
3. Na janela do launcher, abra o DevTools (`F12` ou `Ctrl+Shift+I`) e rode no console:
   `await navigator.mediaDevices.getDisplayMedia({ video: true })`

Esperado: o seletor de tela do Chromium aparece e a promise resolve com um `MediaStream`.

**Se falhar** (sem seletor, `NotAllowedError` imediato ou `mediaDevices` indefinido): pare
aqui e avise. O plano B é a Task 4b: liberar a permissão de captura via `PermissionRequested`
do WebView2 no `host_start`.

## 1. Fluxo principal com vídeo (1 Host + 1 convidado, mesma máquina)

Três terminais PowerShell:

1. `cargo run -p dm-signaling`
2. `.\target\debug\better-together.exe` (Host: nome "Tamy", Pato, **Criar sala**)
3. `.\target\debug\better-together.exe` (Convidado: código digitado em minúsculas e com espaço)

Esperado:

- [ ] O clique em **Criar sala** abre o seletor de tela. Escolha **Tela inteira** do monitor
      principal. O painel (360×340) mostra o código, "Compartilhando a tela" e nenhum aviso.
- [ ] O convidado vê o lobby com o Pato ocupado. Escolher a Capivara abre o viewer em 1280×720.
- [ ] O viewer mostra "Aguardando o vídeo do Host..." e depois a tela do Host ao vivo.
- [ ] Os bichinhos aparecem **por cima** do vídeo, sobre o mesmo ponto da tela do Host.
      Mova a capivara até um ícone ou canto e confira nos dois lados.
- [ ] O vídeo **não** mostra os bichinhos da overlay (não pode haver pato duplicado nem
      "fantasma" atrasado). Esse é o teste do `WDA_EXCLUDEFROMCAPTURE`.
- [ ] Redimensione a janela do viewer: vídeo e bichinhos continuam alinhados (letterbox igual).
- [ ] 📏 Defasagem vídeo × bichinhos: deixe um cronômetro com milissegundos na tela do Host,
      tire foto das duas telas juntas e compare. Mova o pato do Host e estime quanto o vídeo
      atrasa em relação ao sprite.

## 2. Admissão: vídeo só para quem entrou

- [ ] Enquanto o convidado está no **lobby** (antes de escolher o animal), o Gerenciador de
      Tarefas do Host não mostra upload de vídeo relevante (~2,5 Mbps só começa depois da
      escolha).
- [ ] Com os 4 animais ocupados (ou outro convidado recusado), o convidado recusado não
      recebe vídeo.
- [ ] Reconexão (seção 6): o convidado que volta recebe vídeo de novo; a conexão antiga
      para de receber.

## 3. Controles do Host

- [ ] **Pausar vídeo**: o painel mostra "Vídeo pausado". 📏 Em quantos segundos o
      convidado vê "Vídeo pausado pelo Host"? (esperado: ~3 s, `STALL_MS`).
- [ ] **Falso positivo:** com o vídeo **ativo** e a tela do Host totalmente parada (sem
      relógio, sem cursor piscando) por 30 s, o convidado **não** pode ver "Vídeo pausado
      pelo Host". Se aparecer, anote o intervalo e aumente `STALL_MS`.
- [ ] **Retomar vídeo**: a imagem volta e o texto some.
- [ ] **Trocar tela** → escolha uma **janela**: o painel avisa "Você está compartilhando uma
      janela: os bichinhos vão ficar desalinhados. Compartilhe a tela inteira." Troque de
      volta para a tela inteira: o aviso some. Nenhum convidado cai.
- [ ] Com dois monitores, se possível, escolha o secundário com proporção diferente: aviso
      "Essa não é a tela principal...".
- [ ] Barra do Chromium **"Parar de compartilhar"** (se aparecer): painel em
      "Compartilhamento encerrado" com o botão **Compartilhar tela**; os bichinhos seguem
      sincronizados. Compartilhar de novo devolve o vídeo sem reconectar ninguém.
- [ ] Clique em **Compartilhar tela** duas vezes seguidas com o seletor ainda aberto: anote o
      que o WebView2 faz (segundo seletor, erro ou nada). Não pode sobrar indicador de
      captura aceso.
- [ ] Recuse o seletor no **Criar sala** (Esc/Cancelar): a sala abre mesmo assim, com
      "Tela não compartilhada" e o botão **Compartilhar tela**. O convidado vê "Aguardando o
      vídeo do Host...".

## 4. Fim de sessão

Em todos os casos o indicador de compartilhamento do Windows/Chromium tem que **apagar**:

- [ ] **Encerrar** no painel.
- [ ] Erro durante a criação da sala (sinalização desligada) depois de escolher a tela.
- [ ] `F5` na janela do Host (DevTools aberto) e fechar a janela do Host.
- [ ] Ao fechar o Host, o convidado vê "O Host saiu".

## 5. Janelas, bandeja e overlay

- [ ] A overlay do Host (sem `DM_DEV_WINDOW`) não pisca escuro ao abrir.
- [ ] A overlay já nasce com o nome do Host em cima do pato, antes de alguém entrar.
- [ ] O seletor de tela não fica escondido atrás da overlay topmost e aceita cliques.
- [ ] O painel com os dois avisos visíveis e 4 jogadores: anote se precisa rolar.
- [ ] Depois de **Encerrar**, o ícone da bandeja some. Criar outra sala faz ele voltar.
- [ ] Com `DM_DEV_WINDOW=1`, a janela da overlay aparece e recebe foco (WASD funciona sem clique).

## 6. Roteiro do M2 (no mesmo build)

Rode as seções 1 a 5 de `docs/e2e-m2.md`: WASD sem atraso perceptível, pet do Host vagando,
convidado fechado dorme e some em 30 s, falha com sinalização desligada, "Sala não
encontrada", reconexão com **Sair**/**Entrar** em até 30 s (agora a lembrança vive no Rust:
deve funcionar igual, inclusive depois do **Voltar**), viewer em 1280×720.

Observe também a pendência adiada: o relógio de render **reancora com salto**? Anote se os
bichinhos dão "pulos" periódicos.

## 7. Desempenho e internet

- [ ] Painel do Host **minimizado por 6 minutos**: o vídeo e os bichinhos no convidado não
      congelam.
- [ ] Pela internet (máquinas diferentes, só STUN), com 3 convidados se possível.
      📏 Upload do Host (Gerenciador de Tarefas → Desempenho → Ethernet/Wi-Fi) e 📏 CPU do
      processo do Host. Esperado: ~7,5 Mbps com 3 convidados.
- [ ] Se a CPU passar de ~50%, anote: o próximo passo é testar H.264 via
      `setCodecPreferences` (fica para o M6).
- [ ] Se alguma rede exigir TURN, defina as três `BT_TURN_*` e anote para o M6.

## Limitações conhecidas

- Windows anterior ao 10 2004: `WDA_EXCLUDEFROMCAPTURE` age como `WDA_MONITOR` e a overlay
  vira um retângulo preto no vídeo (a tela inteira, já que ela cobre o monitor). O app não
  detecta isso; o mínimo suportado é 2004+.
- O vídeo mostra a tela inteira do Host, incluindo títulos de janelas e notificações. O
  botão **Pausar vídeo** existe para esses momentos.
- No Linux não há WebRTC (WebKitGTK): nada do vídeo é testável fora do Windows.

## Números medidos

| Medida | Valor | Observação |
|---|---|---|
| Defasagem vídeo × bichinhos | | |
| Tempo até "Vídeo pausado pelo Host" | | |
| Falso positivo de pausa com tela parada | | |
| Upload do Host (1 / 3 convidados) | | |
| CPU do Host (1 / 3 convidados) | | |
