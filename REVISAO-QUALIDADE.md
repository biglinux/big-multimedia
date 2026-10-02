# Revisão de qualidade do workspace — pendências

Só itens em aberto. O que já foi corrigido está no histórico do git.

## Pendente

- **Screenshots do AppStream**: os metainfo apontam para
  `docs/screenshots/{audio,video}-player-main.png`, que ainda não existem.
- **Memória do vídeo com jemalloc não foi medida em release.** Uma medição
  antiga registrava 390 MB Pss (glibc) contra 451–496 MB (jemalloc) num 1080p;
  no smoke test (720p, debug) o vídeo ficou em 328 MB Pss. Comparar em release
  com o mesmo arquivo antes de publicar.
- **Trocar o shim do jemalloc quando sair o crate com jemalloc 5.4** — o
  `tikv/jemallocator` já tem o 5.4.0 no `main` (#185, commit `9d6e381`, versão
  `0.7.0+5.4.0`), mas o crates.io ainda está em `0.7.1+5.3.1`. Quando publicar:
  subir `tikv-jemalloc-sys` no workspace, ativar
  `override_allocator_on_supported_platforms`, apagar
  `big-os-kit/src/allocator.rs` (manter só o `_rjem_malloc_conf`, que vira
  `malloc_conf`) e conferir com `nm -D` se o export no `build.rs` dos players
  ainda é necessário.

## Simplificações levantadas na revisão de 2026-10-02, não feitas

Maiores ou dependentes de conferência na GUI; cada uma foi lida no código.

- **TV:** os dois `tv.rs` repetem a mesma máquina de estados (`tv_last`,
  `tv_was_released`, fim a 3 s do término). Mover para `TvLink` um
  `observe()` e o último estado; um `TvMedia::handoff` cobriria as 4 montagens
  de `PlaybackStart`; `toggle_pause`/`toggle_mute`/`set_volume` em `TvLink`
  cobririam os 3 lugares que soletram os comandos.
- **Vídeo:** fiação de diálogo com entrada repetida 3× (Abrir URL, Abrir disco,
  Ir para tempo; manter o botão fraco do AGENTS.md); moldura de miniatura
  montada 3× (`library.rs`, `queue_view.rs`, `panel.rs`); três tabelas paralelas
  por id em `zones.rs`; `IdleBackground::show/hide` recebem um `Display` que
  todos os chamadores buscam igual.
- **Vídeo, busca da biblioteca:** `library::refresh` tira o cache `scanned`
  com `mem::take`; uma segunda busca durante a varredura reescaneia tudo.
- **Áudio:** `BrowserNodeContract` (só testes leem `label/secondary/count`)
  custa 3 clones e um `i18n` por nó na thread da GUI; 9 ações `win.*` repetem o
  mesmo closure em `component.rs`, cujo `ViewStack` local ainda recebe títulos
  e ícones que ninguém mostra.

## Não verificado na interface

Cobertos só por leitura e compilação:

- clique na barra lateral do vídeo com Aleatório ligado;
- toggles Repetir/Aleatório do menu da playlist depois de mudança feita pelas
  Preferências ou pelo MPRIS;
- fechamento da janela quando o mpv encerra no modo ocioso.

## Não feito, com motivo

- Mover time/bytes/playlist_order para o `big-media-engines`: os módulos do
  runtime que os usam dependem de glib/gtk de qualquer forma, então a feature
  `ui-core` não cai.
- Conexão D-Bus assíncrona do MPRIS: medida em 5–11 ms; exigiria mudar a API.
