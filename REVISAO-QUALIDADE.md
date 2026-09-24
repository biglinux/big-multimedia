# Revisão de qualidade do workspace — 2026-09-23

Revisão feita por 7 revisores paralelos (media-runtime, audio, video + quick-export,
foundation, app/UI, packaging/i18n/manifestos, duplicação entre crates) mais
verificação própria. Achados baseados em leitura e grep; os marcados "confirmado"
foram checados de novo antes de entrar aqui.

Linha de base: `cargo clippy --workspace --all-targets` limpo; clippy por player
(`-p big-audio-player` etc.) também limpo depois das correções abaixo.

## Corrigido nesta sessão (compilado: clippy workspace + por player; testes verdes)

| Arquivo | Correção |
|---|---|
| `big-audio-player/.../panel_intents.rs` `reconcile_deleted_tracks` | Removia só a 1ª cópia de faixa apagada da fila; agora remove todas |
| `big-audio-player/.../panel_intents.rs` `write_tags` | Falha ao espelhar tags no DB era descartada; agora loga |
| `big-audio-player/.../panel_intents.rs` `persist_rating`, `save_queue_as_playlist`, `reconcile_deleted_tracks` | `let _ =` em escrita de DB trocado por `log::warn!` |
| `big-audio-player/.../sources.rs` `record_radio_stream` | Caminho não-UTF-8 desligava a gravação mas mostrava "Gravando"; falha em `create_dir_all` era ignorada. Agora avisa "Recording was not saved" (msgid já traduzido) |
| `big-video-player/.../fileops.rs` `move_file` | Cópia entre FS que falhava deixava arquivo truncado no destino; agora remove |
| `big-video-player/.../trim/run.rs` `start_trim_export` | Não validava segmentos (end <= start, NaN) antes do ffmpeg; agora usa `validate_export_segments` |
| `big-video-player/.../media_adjust.rs:620` | Doc comment falso ("persiste tamanho") corrigido |
| `big-os-kit/src/url.rs` `filename_from_url_path` | URL terminando em `/..` virava nome de arquivo `..` (escape do diretório de download) |
| `big-player-ui/src/download/transfer.rs` `filename_from_url` | Título do feed (fallback) com `/` ou só pontos chegava ao caminho; sanitizado + 2 asserts |
| `big-media-data/src/db/schema.rs` | Setup FTS (tabela + 3 triggers + backfill) agora em transação; antes, falha parcial deixava busca sem triggers para sempre |
| `big-appearance/src/chrome.rs` | Constantes e parser `#rrggbb` duplicados de `lib.rs` removidos; `body_alpha` com transparência negativa gerava NaN → agora `clamp(0, 100)` |
| `big-os-kit/src/storage.rs:5` | Doc com nome de temp file errado |
| `media-runtime/src/cast/transcode/session.rs:147` | Dois ramos de abertura do `ffmpeg-cmd.log` com mesmo efeito unidos |
| `media-runtime/.../filters/mod.rs:10` | `use software_hwdec` gerava warning no build só-audio; agora `#[cfg(feature = "video")]` |
| `media-runtime/src/cast/gpu/detect.rs` | `detect_gpu` sem nenhum chamador removido (+ re-export) |
| `media-runtime/po/POTFILES.in` | Faltavam `src/cast/daemon.rs` e `src/player/mpv_backend/backend/volume.rs`; xgettext bate com o `.pot`, pt_BR 1483/1483 |
| `media-runtime/Cargo.toml` | Dependência `md-5` sem uso removida (só comentário a citava) |
| `big-relm4-components/Cargo.toml` | `big-appearance` e `serde_json` sem uso removidos (confirmado: 0 referências em código) |
| `big-quick-export/Cargo.toml` | dev-dependency duplicando a dependência normal removida |
| `scripts/review-native.sh` | `cd` sem `|| exit` (SC2164) |
| `.gitignore` | `__pycache__/` adicionado |

Observação: o doctest de `big-media-data` "falha" com `TMPDIR=/tmp` (noexec); passa
com `TMPDIR=~/.cache/...`. Não é bug de código.

## Estado em 2026-09-24 (quinta rodada)

Resolvido nesta rodada (clippy do workspace e dos players sem warnings, `cargo fmt`
limpo, todos os testes do workspace verdes, pt_BR 1440/1440):

- **Parsers**: os módulos `playlist`/`subtitle` do `big-media-engines` saíram — nenhum
  chamador, e a semântica deles não servia (M3U rejeitava `../`, SRT removia `<i>`).
  O runtime tem um só laço M3U (`iptv::parse_m3u_content`, usado também pelo
  `parse_m3u` de playlists), leitura limitada a 64 MiB (`read_playlist_text`),
  filtro de caminhos de sistema também no PLS, e testes de propriedade. O merge de
  legendas do conversor era código morto e saiu junto.
- **Dados**: migrações versionadas (`PRAGMA user_version`, `MIGRATIONS` em
  `schema.rs`), índice duplicado removido na migração 1; linhas ilegíveis são
  puladas com log (`db::decoded_rows`); `validate_column` rejeita coluna
  desconhecida; salvar fila como playlist é transacional e avisa em caso de falha;
  offset do fallback de fila derivado da lista de colunas; escritas de registro do
  convertd logadas.
- **Segurança**: token do servidor do cast comparado em tempo constante e servidor
  recusa subir sem `/dev/urandom`; `--` antes dos caminhos no big-video-converter;
  caminho do papel de parede precisa ser absoluto; renomear/mover usam
  `renameat2(RENAME_NOREPLACE)` (com teste).
- **Desempenho**: ordenação por data/tamanho da playlist fora da thread GTK;
  servidor de pipe do cast com `Condvar` em vez de `sleep`.
- **Duplicação**: boot comum `app_base::boot_player`; dono de processo filho e
  `set_nonblocking` compartilhados no `reaper`; conversões do editor de tags sem
  wrap (`i32::try_from`); `ffmpeg_timestamp` único (e com arredondamento correto);
  detecção de tipo de arquivo única no engines (sniff de MPEG-TS exige 2 pacotes);
  fachadas `ui_helpers` e `mpris` do runtime removidas; rasterizadores do papel de
  parede com uma checagem só; laço de export do trim confere a saída.
- **Código morto**: ~160 funções públicas sem chamador (conferido também contra
  `/note/big-host` e `/note/big-modules`), 5 diálogos inteiros, itens privados que
  só elas usavam e os testes deles.
- **Vídeo**: toggles Repetir/Aleatório do menu refletem o estado ao abrir; modo
  ocioso aplicado quando um arquivo carrega, e a janela fecha quando o mpv encerra.
- **Baixa prioridade**: erros engolidos agora logados (settings rejeitados, sinais
  MPRIS, migração de rádio, systemctl do timer — agora reaproveitado pelo
  `reaper`); caches com escrita atômica; frames de scrub gravados à parte e
  renomeados; limite da arte MPRIS não é mais travado por arquivos estranhos;
  `is_some()+expect` removido; seleção da grade exposta a leitores de tela; toast
  de jargão removido; erros de export traduzidos; herança de workspace nos
  manifestos; `--locked` nos PKGBUILDs; README corrigido; scripts Python passam no
  ruff; CI e `rust-toolchain.toml` aninhados e assets nunca instalados removidos;
  URLs do AppStream apontam para big-multimedia.

## Ainda pendente

- **Screenshots do AppStream**: os metainfo apontam para
  `docs/screenshots/{audio,video}-player-main.png`, que ainda não existem.
- **Modo GPU automático** (`converter/queue/mod.rs`): usa só a primeira GPU; o
  caminho multi-GPU só roda com seleção manual. Decisão de produto: misturar
  encoders diferentes num mesmo lote?
- **Pkgrel** dos PKGBUILDs (audio 10, video 23) não incrementado.
- **Memória do vídeo com jemalloc** não medida em release.
- **Shim do jemalloc**: trocar quando o crate com jemalloc 5.4 sair (ver abaixo).
- **Não feito, com motivo**:
  - Mover time/bytes/playlist_order para o engines: os módulos do runtime que os
    usam dependem de glib/gtk de qualquer forma, então a feature `ui-core` não cai.
  - Conexão D-Bus assíncrona do MPRIS: medida em 5–11 ms; exigiria mudar a API.
  - Lint pedantic do clippy: não está habilitado no workspace; é escolha de
    política, não defeito.
- **Não verificado na interface**: clique na barra lateral com aleatório ligado,
  toggles do menu após mudança externa, fechamento da janela no modo ocioso.

## Resolvido na segunda rodada (decisões do usuário)

- **jemalloc embutido na compilação** — `big-os-kit` ganhou a feature `jemalloc`
  (`src/allocator.rs`): `tikv-jemalloc-sys` 0.7.1 prefixado + shim que exporta
  `malloc`/`free`/`calloc`/`realloc`/`posix_memalign`/`aligned_alloc`/`memalign`/
  `valloc`/`pvalloc`/`malloc_usable_size`. Os players ativam a feature e exportam
  os símbolos no `build.rs` (`--export-dynamic-symbol`). Config antes passada via
  `MALLOC_CONF` agora está em `_rjem_malloc_conf`. `free_sized` fica de fora de
  propósito: o jemalloc 5.3.1 empacotado quebra com `free_sized(NULL)`, e o da
  glibc 2.44 só salta para `free`. `.desktop` sem `big-jemalloc-run`; PKGBUILDs
  sem `optdepends`; `tune_allocator` (mallopt glibc) e `big-os-kit::mem`
  (`trim_arenas`, sem chamador) removidos.
- **Flatpak e Nix removidos** (4 manifestos + menções nos READMEs).
- **`git rm`** de `scripts/audit-final/__pycache__` (untrack) e
  `crates/media-runtime/Cargo.lock`.
- **MPRIS no video player** — `video_shell/mpris.rs`: bus
  `org.mpris.MediaPlayer2.BigVideoPlayer`, Identity "Big Video Player", MIME de
  vídeo; Play/Pause/PlayPause/Stop/Next/Previous/Seek/SetPosition/Volume/Rate/
  OpenUri/Raise/Quit; sinal `Seeked`. Desligado no modo papel de parede.

## Resolvido na terceira rodada

- `save_queue_session` volta a conexão para `synchronous=NORMAL` depois de salvar,
  inclusive quando falha (regressão em `tests/queue.rs`).
- Detecção de GPU do cast cacheada por sessão (`CastSessionController::resolve_gpu`),
  limpa em `stop_casting`; o bloco "override ou detectar" copiado 3× virou esse método.
- Repetir do vídeo: `MpvBackend::set_loop_mode` substitui `set_loop_file`/
  `set_loop_playlist`; o menu da playlist agora aplica ao mpv, o boot aplica
  `loop-mode`, `keep-open` e `keep-open-pause`; MPRIS publica e aceita `LoopStatus`
  e `CanGoNext` considera a volta da lista.
- `dedup_output_path`: o achado de perda de dados não procede — o worker recusa
  qualquer saída existente antes do ffmpeg (`receipts.rs:117`).

## Resolvido na quarta rodada

- **Aleatório do vídeo** via mpv (`MpvBackend::playlist_shuffle`): embaralha e
  move o arquivo atual para o início, para o Próximo alcançar todos. Ligado ao
  menu da playlist, ao carregar uma lista com a opção ativa e ao MPRIS
  (`Shuffle` lido/escrito). Saltos e remoções na playlist passam a achar o índice
  pelo caminho na ordem viva do mpv (`get_playlist_paths`).
- **Abrir um arquivo tocava o anterior da pasta** — `playlist_play_index` usava
  `set_property` síncrono, que rodava antes dos `loadfile` assíncronos; agora é
  `playlist-play-index` na mesma fila.
- **Retomar reprodução no vídeo**: aplicado no boot; ao fechar, grava a posição
  enquanto o arquivo está carregado e desliga o save-on-quit do mpv, que
  sobrescrevia o arquivo com conteúdo vazio no desligamento.
- `pactl_volume.rs` (sem uso, substituído por `backend/volume.rs`) removido do git.

## Pendente novo

- **O rádio "Repetir" do menu da playlist não reflete mudanças feitas pelas
  Preferências ou pelo MPRIS** até reabrir a janela (estado da GAction). O mesmo
  vale para o toggle "Aleatório".
- **Clique na barra lateral com aleatório ligado não foi verificado na interface**
  (a barra não abriu na sessão de teste); o caminho foi coberto só por leitura
  e compilação.
- **Modo ocioso** (`idle-mode`) continua só aplicado pelas Preferências: aplicar
  `no` no boot faria o mpv encerrar ao abrir o player sem arquivo.
- **Trocar o shim do jemalloc quando sair o crate com jemalloc 5.4** — o
  `tikv/jemallocator` já tem o 5.4.0 no `main` (#185, commit `9d6e381`, versão
  `0.7.0+5.4.0`), mas o crates.io ainda está em `0.7.1+5.3.1`. Quando publicar:
  subir `tikv-jemalloc-sys` no workspace, ativar
  `override_allocator_on_supported_platforms`, apagar
  `big-os-kit/src/allocator.rs` (manter só o `_rjem_malloc_conf`, que vira
  `malloc_conf`) e conferir com `nm -D` se o export no `build.rs` dos players
  ainda é necessário.
- **Pkgrel** dos dois PKGBUILDs não foi incrementado (audio 10, video 23).
- **Memória do vídeo com jemalloc não foi medida.** O comentário removido de
  `app_base.rs` registrava 390 MB Pss (glibc) vs 451–496 MB (jemalloc) num 1080p.
  No smoke test (720p, debug) o vídeo ficou em 328 MB Pss; comparar em release
  com o mesmo arquivo antes de publicar.

## Correções a achados dos revisores

- `log` e `serde_json` em `big-relm4-components` — `log` é usado (6 refs); `serde_json`
  só aparecia em comentário (removido).
- `big-media-engines` em `big-media-data` foi apontado como sem uso, mas tem 5
  referências. Mantido.
