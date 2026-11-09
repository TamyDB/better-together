# Roteiro de teste manual do M2 (rede)

Teste ponta a ponta com janelas reais: 1 Host e 1 convidado na mesma máquina, com a
sinalização local. Com 3 convidados é desejável, mas o limite é coberto pelos testes.

Gere o executável: `pnpm --dir app tauri build --debug --no-bundle`.

## 1. Fluxo principal (3 terminais)

1. `cargo run -p dm-signaling`
2. `DM_DEV_WINDOW=1 ./target/debug/better-together` (Host: nome "Tamy", Pato, "Criar sala")
3. `./target/debug/better-together` (Convidado: código digitado em minúsculas e com espaço)

Esperado:

- o lobby mostra o Pato ocupado;
- escolher a Capivara abre o viewer;
- WASD no viewer move a capivara na janela do Host com atraso imperceptível;
- o pato do Host aparece vagando no viewer;
- fechar o convidado deixa a capivara dormindo no Host, e ela some em 30 s;
- fechar o Host mostra "O Host saiu" no convidado.

## 2. Falha

Com a sinalização desligada: "Criar sala" mostra um erro, sem travar; um código inexistente
mostra "Sala não encontrada".

## 3. Reconexão

No convidado, clique **Sair** (sem fechar o app) e, em até 30 s, clique **Entrar** de novo com
o mesmo código. Esperado: volta ao mesmo bichinho, sem escolher animal (funciona até com a
sala cheia). Depois de 30 s, o bichinho some e o lobby volta a oferecer o animal.

**Fechar** o app gera um token novo e entra como jogador novo: é o esperado.

## 4. Janelas

- A janela do viewer abre em 1280x720.
- A overlay do Host (Windows, sem `DM_DEV_WINDOW`) não pisca escuro ao abrir.

## 5. Pela internet

Máquinas diferentes, só STUN. Se a rede de alguém exigir TURN, defina as três `BT_TURN_*` e
anote para o M6.

## Limitações no Linux

- Não há `WH_KEYBOARD_LL`: o teclado do Host só funciona com a janela focada.
- O Host precisa de `DM_DEV_WINDOW=1`; sem ele a overlay é transparente e click-through. O
  convidado não precisa.
- `Ctrl+Shift+P` e a bandeja podem não funcionar em Wayland/GNOME.
- Fechar o app abruptamente pode levar alguns segundos até o bichinho ficar "dormindo", e só
  então contam os 30 s.
