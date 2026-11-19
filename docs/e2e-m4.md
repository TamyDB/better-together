# Roteiro de teste manual do M4 (chat e emotes)

Teste ponta a ponta no **Windows 10 2004+ ou 11** (VM ou PC), com WebView2, de preferência com
**1 Host + 2 convidados**. Rode no mesmo build do M3 (`docs/e2e-m3.md`), que também nunca rodou
no Windows: faça as duas partes juntas.

Antes de começar:

- A branch é `m4-social` (base `m3-video`). Confira a CI `windows-latest` verde.
- Gere o executável no próprio Windows: `pnpm --dir app tauri build --debug --no-bundle`.
- `cargo run -p dm-signaling` num terminal; Host e convidados com `.\target\debug\better-together.exe`.

Teclas do M4: no viewer do convidado, `Enter` abre o campo de chat, `Enter` envia e `Esc`
cancela. `1` a `6` mandam os emotes (❤️ 😂 😮 😠 🎵 ❓), também pelos botões. No Host, o modo
bichinho (`Ctrl+Shift+P`) aceita `1` a `6` do mesmo jeito.

Anote os números pedidos (marcados com 📏) no fim do arquivo ou no PR.

## 1. Balões e histórico (Host + 2 convidados)

- [ ] O convidado A digita "oi" e dá `Enter`: o balão aparece sobre o bichinho dele **nos três
      lados** (viewer de A, viewer de B e overlay do Host), no lugar certo, acompanhando o
      bichinho quando ele anda.
- [ ] Ninguém vê a própria mensagem antes do eco do Host (com a rede boa a diferença é
      imperceptível; com a rede ruim o balão demora a aparecer, e isso é o esperado).
- [ ] O balão de chat some em ~5 s (`CHAT_MS`). 📏 Cronometre.
- [ ] Um chat novo do mesmo jogador **substitui** o balão anterior.
- [ ] O histórico do viewer mostra as 6 mensagens mais recentes, com o nome do autor, na ordem.
- [ ] O Host também fala (pelo painel) e aparece nos viewers com o nome dele.
- [ ] O painel do Host mostra o histórico das mensagens de todos.
- [ ] Nome longo (32 caracteres) ou com emoji: o balão e o histórico não estouram nem
      cobrem o bichinho de forma ilegível.

## 2. Emotes por tecla e por botão

- [ ] No viewer, as teclas `1` a `6` mostram ❤️ 😂 😮 😠 🎵 ❓ sobre o bichinho, nos três lados.
- [ ] Os seis botões de emote do viewer fazem o mesmo.
- [ ] O emote some em ~2,5 s (`EMOTE_MS`). 📏 Cronometre.
- [ ] Um emote novo substitui o anterior, e um emote **não** apaga o balão de chat (tipos
      diferentes convivem).
- [ ] **Tecla segurada:** segure `1` por 3 s. Aparece **um** emote só (sem metralhadora).
      Solte e aperte de novo: um novo emote.
- [ ] **Botão e Espaço:** clique num botão de emote e depois aperte `Espaço`. Anote se o
      emote é reenviado (o botão mantém o foco; **pendência conhecida**, não é regressão).

## 3. Modo bichinho do Host

- [ ] Com o modo ligado (`Ctrl+Shift+P`), `1` a `6` disparam emotes no pet do Host, e as
      teclas **não** chegam ao app que estava em foco (são engolidas como o WASD).
- [ ] Segurar `1` no modo bichinho dispara um único emote.
- [ ] **Painel do Host em foco:** com o modo ligado, clique no painel e digite
      `wasd 123` no campo de chat dele. O texto aparece inteiro no campo, o pet **não**
      se move e **nenhum** emote é disparado.
- [ ] Com o painel em foco e o modo ligado, `Esc` **não** desliga o modo (é do campo de
      texto). Para desligar, use `Ctrl+Shift+P`.
- [ ] Clique em outro app: o modo volta a engolir WASD/`1`-`6`/`Esc` normalmente.
- [ ] Com o modo desligado, digitar `1` a `6` em qualquer app funciona normalmente.

## 4. Digitar no chat do convidado sem mover o pet

- [ ] Com o campo de chat aberto, digite `wasd 123`: o texto aparece inteiro, o bichinho
      **não** anda e **nenhum** emote dispara.
- [ ] Segure `W` (bichinho andando), aperte `Enter` para abrir o chat: o bichinho **para**
      (teclas soltas ao focar o campo).
- [ ] `Esc` fecha o campo sem enviar e devolve o controle: WASD volta a mover, `1`-`6` voltam
      a mandar emote.
- [ ] `Enter` com o campo vazio não envia nada.
- [ ] Depois de enviar, o campo fecha e o teclado volta ao jogo.

## 5. Texto: acentos, IME, emoji, tamanho

- [ ] Acentos e `ç`/`ã`/`é`/`ô` (inclusive via tecla morta, como `'` + `a`): chegam certos
      no balão e no histórico dos três lados.
- [ ] IME (japonês, chinês ou coreano, se disponível): a composição não envia nada no
      meio, e o texto final chega certo. Anote se `Enter` para confirmar a composição também
      envia a mensagem (**pendência conhecida**: `isComposing` no `Enter`).
- [ ] Emoji no texto (😀, 🦆, família 👨‍👩‍👧, bandeira 🇧🇷): aparece no balão e no histórico, sem
      quadradinhos nem corte no meio do caractere.
- [ ] Texto com mais de 120 caracteres é cortado em 120 no Host; o balão quebra em linhas
      e não sai da tela.
- [ ] Colar texto com quebra de linha ou tabs: vira uma linha só, sem controle estranho.
- [ ] Texto com `<b>x</b>` ou `<img src=x>`: aparece **literal**, nunca como HTML.

## 6. Limites e flood

- [ ] **Flood de chat:** segure `Enter` com o campo cheio de texto repetido (ou cole e
      reenvie rápido). O app não trava nem deixa de responder; chegam 3 mensagens seguidas e
      depois ~1 a cada 2 s (`token bucket` de 3, recarga de 2 s). As barradas não aparecem
      para ninguém, **nem para o autor**.
- [ ] 📏 Em 10 s de flood, quantas mensagens chegaram? (esperado: ~3 + 5 = 8.)
- [ ] **Flood de emote:** segure/aperte `1`-`6` o mais rápido possível: no máximo 1 emote
      por segundo aparece.
- [ ] O flood de A não atrapalha B: B fala normalmente durante o flood de A.
- [ ] O vídeo e os bichinhos seguem fluidos durante o flood.

## 7. Sono, reconexão e saída

- [ ] Convidado A fecha a janela (bichinho **dormindo**): ele continua aparecendo e **recebe**
      balões? (Quem está dormindo não fala, mas os outros continuam falando; confira que
      nada quebra.)
- [ ] Os balões dos outros aparecem normalmente quando A volta pelo **Entrar** (rejoin em até
      30 s) e A continua falando com o mesmo nome e bichinho.
- [ ] A sai com **Sair**: o balão dele some e o histórico mantém as mensagens antigas.
- [ ] Convidado recusado (sala cheia) ou ainda no lobby não manda nem recebe chat.
- [ ] O Host encerra: os viewers mostram "O Host saiu" sem erro de chat no console.

## 8. Roteiro do M3 (no mesmo build)

Rode o `docs/e2e-m3.md` inteiro (vídeo, controles, fim de sessão, janelas e desempenho).
O chat e os emotes não devem alterar o vídeo nem a sincronia dos bichinhos.

## Limitações conhecidas

- O botão de emote clicado mantém o foco; `Espaço` ou `Enter` logo depois pode reenviar o emote
  (respeitado o limite de 1/s).
- `Enter` durante uma composição de IME pode enviar a mensagem antes da hora.
- Com o painel do Host em foco e o modo bichinho ligado, WASD e `1`-`6` não movem o pet:
  é o esperado ao digitar. Com o modo ligado, o Host não digita `1`-`6` nos outros apps;
  `Ctrl+Shift+P` desliga.
- O histórico guarda só as 50 últimas mensagens e não persiste entre sessões.
- No Linux não há WebRTC (WebKitGTK): nada do chat entre máquinas é testável fora do Windows.

## Números medidos

| Medida | Valor | Observação |
|---|---|---|
| Duração do balão de chat (esperado ~5 s) | | |
| Duração do emote (esperado ~2,5 s) | | |
| Mensagens recebidas em 10 s de flood (esperado ~8) | | |
| `Enter` confirma composição de IME e envia? | | |
| Botão de emote + `Espaço` reenvia? | | |
