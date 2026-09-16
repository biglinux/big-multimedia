# big-video-player — arquitetura vigente do candidato

Conferida na árvore corrigida em 16/09/2026. **Revisão de fonte; compilação e execução GTK/mpv não realizadas.**

## Organização real

O aplicativo pertence ao workspace raiz com 16 crates. Os manifests declaram edição 2024,
Rust 1.98.1, gtk4 0.11 com a feature de GTK 4.22, libadwaita 0.9 com a feature de 1.9 e
Relm4 0.11. Esses números são requisitos do projeto, não prova de disponibilidade do ambiente.

`src/video/app.rs` inicia o aplicativo; `src/video/ui/video_shell/` contém o componente
Relm4. Não orientar mudanças para a antiga organização `window/imp`. O motor compartilhado
está em `crates/media-runtime/src/player/mpv_backend/`. Preferências, biblioteca e contas
estão em `foundation/big-media-data`; processos, transporte e persistência em
`foundation/big-os-kit`; integrações desktop em `app/big-app-kit` e `app/big-app-kit-core`.
Os componentes GTK reutilizáveis ficam em `ui/`.

## Contratos implementados nesta correção

Os comandos de UI não substituem a confirmação do motor. Volume e mute usam o mpv, sem
polling externo de pactl para cada mudança. O serviço MPRIS compartilhado e seu adaptador
de áudio preservam Playing/Paused/Stopped, identificam a faixa, validam SetPosition no
ponto de aplicação e usam eventos nativos para Seeked; a revisão não acrescentou um novo
serviço MPRIS completo ao aplicativo de vídeo.

O renderer retém o core e o contexto GL e não reutiliza uma textura entregue ao GTK antes
da liberação do consumidor. A cópia do caminho por software é deliberada para manter a
imagem imutável. Nenhum caminho fornece prova de zero-copy global, direct scanout ou
compatibilidade de driver. O encerramento do host deve drenar leases enquanto a infraestrutura
GL/GTK necessária continua disponível.

Screenshots pertencem ao backend: cancelamento retira a intenção de publicação, mas não
libera o arquivo de um escritor ainda ativo. Saídas ficam em staging privado até resposta
terminal; publicar nunca substitui um destino existente. O mesmo princípio vale para
exportação, com comparação do ffprobe ao plano e preservação do original. Split pode
publicar parte dos arquivos antes de uma falha: não há transação atômica de múltiplos destinos.

Persistência distingue falha anterior à publicação de publicação com durabilidade
incerta. Preferências compartilham estado no processo e usam comparação da revisão sob
lock cooperativo. Isso não é isolamento contra escritores maliciosos do mesmo usuário;
as gravações de todas as telas ainda não foram movidas para um worker único.

## Fluxos e recursos restantes

Resultados antigos de busca são descartados por geração e há somente um pedido pesado
ativo mais o mais recente pendente. Erro não é resultado vazio; mídias em discos ausentes
não são apagadas automaticamente. Demais consultas e telas não receberam certificação
integral de todos os caminhos.

HTTP usa curl gerenciado, com dados por stdin e retorno terminal verificado; permanece um
processo por requisição, não um pool. Remoto HTTP é opt-in para LAN confiável, não TLS.
Credenciais locais são privadas e limitadas, mas não foi feita migração para Secret Service.
Callbacks e coleções limitados não demonstram por si só ausência de bloqueio ou baixo PSS.

## Validação

Na raiz, `python3 scripts/audit-final/run.py --output /caminho/novo-logs` executa os gates
estáticos. `--mechanisms` usa curl/FFmpeg já instalados com dados sintéticos; `--browser`
usa os assets reais com transporte simulado. Nenhum desses testes compila o Rust.

`scripts/audit-final/native-check.sh` é exclusivamente para o agente integrador em ambiente
já preparado; não instala ferramentas e foi apenas conferido quanto à sintaxe Bash.
O relatório do pacote distingue alterações candidatas, testes auxiliares executados e gates
nativos. A arquitetura anterior está arquivada em `docs/architecture-historical.md`.
