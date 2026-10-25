# Histórico de versões

O formato segue o [Keep a Changelog](https://keepachangelog.com/pt-BR/1.1.0/) e as versões seguem o
[Versionamento Semântico](https://semver.org/lang/pt-BR/).

## [Não lançado]

## [0.2.0] — 2026-10-25

### Adicionado
- Interface em 12 idiomas, os mais falados: português, inglês, chinês (simplificado), hindi, espanhol, árabe, francês,
  bengali, russo, indonésio, urdu e alemão. O idioma padrão é o do Windows e, se ele não tiver tradução, o inglês;
  *Arquivo › Opções… › Idioma* escolhe outro e reinicia o Snapnab e o Editor para aplicar. Chinês, hindi e bengali usam
  uma fonte com os caracteres do idioma; em árabe e urdu os textos são lidos da direita para a esquerda (a disposição
  das janelas continua a mesma).
- Traduções em catálogos gettext (`lang/<idioma>.po`, modelo `lang/snapnab.pot`), editáveis no Poedit;
  `scripts/i18n.py` extrai os textos do código e confere os catálogos (textos faltando e marcadores `%d`/`%s`).
- Instalador `Snapnab-<versão>-setup.exe`, gerado pelo `build.bat` junto com o zip portátil (com o Inno Setup 6
  instalado). Instala só para o usuário, sem pedir administrador, ou para todos os usuários; cria os atalhos no menu
  Iniciar, atalho na área de trabalho opcional e "iniciar com o Windows"; fecha o Snapnab aberto antes de atualizar;
  desinstala por *Configurações › Aplicativos* mantendo capturas e configurações. Assistente nos mesmos 12 idiomas do
  programa (escolhidos pelo idioma do Windows), no tema claro/escuro do Windows e com o ícone do Snapnab.
- Webcam na gravação de vídeo: escolha da câmera em *Webcam*, dentro da área gravada, com botão no painel de gravação
  para mostrar ou ocultar. Formatos círculo, quadrado arredondado, quadrado, retângulo 16:9 e 4:3, e remoção do fundo
  por cor (chroma key, com cor e tolerância ajustáveis), que deixa a webcam sem moldura.
- Engrenagem da Seleção "Região" (imagem e vídeo): *Selecionar a região na captura* ou *Região fixa* (largura, altura e,
  se quiser, posição fixa na tela, capturada sem seletor). Substitui o item "Região com tamanho fixo" da lista; presets
  antigos são convertidos.
- Editor: barra de ferramentas centralizada com 10 ferramentas e o botão "Mais" com as demais. Por padrão, Mover,
  Seleção, Texto, Marcador e Forma ficam nas 5 primeiras, a Lupa na última e as posições 6 a 9 mostram as ferramentas
  usadas por último; arrastar um item sobre outro troca os dois de lugar.
- Editor: a barra aceita, além de ferramentas, efeitos, saídas (arquivo, copiar tudo, e-mail, imprimir, programa
  externo), comandos (desfazer, refazer, nova imagem, abrir, salvar, duplicar, excluir, mesclar, girar, corte
  automático, Grab Text) e espaçadores, escolhidos na janela *Personalizar a barra* (pelo "Mais").
- Editor: painel de Efeitos à direita (aba ao lado de Propriedades) com a galeria de efeitos rápidos (Sombra, Borda e
  sombra, Papel rasgado, Dobra de página, Onda, Serrilhado, Esmaecer, Chanfrado e os que você adicionar) e os efeitos
  com prévia na imagem atual. Clicar num efeito abre os ajustes no próprio painel, com prévia ao vivo na imagem, e
  *Aplicar*/*Cancelar*.
- Editor: efeitos novos Perspectiva, Dobra de página e Spotlight; borda especial Chanfrada; filtros Relevo, Mosaico,
  Nitidez e Desfoque; Ajuste de cores junta brilho, contraste, gama, matiz e saturação.
- Editor: bandeja de capturas recentes (até 10, configurável em `RecentTrayDocs`) com arrastar e soltar: arraste uma
  ou várias capturas para outro programa (vão como PNG com as anotações, vídeos como MP4) e solte arquivos do Explorer
  para importá-los. O botão *Biblioteca* mostra todas as capturas em grade.
- Editor: biblioteca com seleção múltipla (Ctrl+clique, Shift+clique, Ctrl+A), exportar e excluir as selecionadas, X na
  miniatura para excluir, Del exclui as selecionadas e contorno de destaque ao passar o mouse.
- Editor: seleção por retângulo na bandeja e na biblioteca, como no Explorer: segure o botão esquerdo num espaço vazio
  (na bandeja, também na faixa do título) e arraste sobre as miniaturas; com Ctrl ou Shift soma à seleção anterior e,
  perto da borda, a lista rola sozinha.
- Editor: o Marcador desenha um retângulo arrastado na cor e na opacidade escolhidas (objeto de destaque editável).
- Editor: estilos rápidos em cada ferramenta, como na referência: galeria com os estilos do tema (Básico, Industrial
  ou um tema seu com 4 cores), "+" para guardar o estilo atual, estrela para favoritar e a ferramenta Favoritos com os
  favoritos de todas as ferramentas. Temas: Novo tema…, Importar…, editar, duplicar, exportar (.snaptheme) e excluir.
- Editor: propriedades de cada ferramenta como na referência — cores com paleta, sombra com direção (grade 3×3), cor,
  opacidade, desfoque e distância; pontas de seta (8 tipos) e tracejado; larguras com controle deslizante; curva de
  Bézier; texto com contorno, negrito/itálico/sublinhado, alinhamento horizontal e vertical e margem; 24 formatos de
  balão (fala, nuvem, pensamento, caixa com seta, setas, nota, explosão, sussurro) com degradê; formas (retângulo,
  arredondado, elipse, triângulo, losango, paralelogramo, hexágono, estrela); passos em círculo, gota ou quadrado,
  numerados por números, letras ou algarismos romanos; lupa redonda ou quadrada, com ampliação e lente separada da
  área; borracha com opacidade e suavização; Preencher com conta-gotas e tolerância; Smart Move no Mover.
- Editor: carimbos coloridos em categorias (Negócios, Interface, Atividades, Comida e bebida, Feriados, Smileys e
  pessoas, Adesivos, Símbolos, Viagem e lugares) com busca, e "Meus carimbos" com imagens importadas.
- Editor: janela Personalizar a barra em grade (ferramentas de desenho, itens de menu, destinos, efeitos e espaçador):
  arraste um item até a barra para acrescentá-lo; com a janela aberta, arraste da barra para fora para remover.
  Novas na barra: Favoritos e Varinha mágica.
- Editor: apresentação da Lupa no primeiro uso; rodapé com "Capturas recentes", tamanho da imagem e menu de zoom.
- Editor: os estilos de cada ferramenta são lembrados entre as sessões.
- Editor: menu Imagem como na referência — Ferramentas, Efeitos (com a amostra de cada efeito), Organizar (trazer para a
  frente Ctrl+Shift+F, primeiro plano, enviar para trás Ctrl+Shift+B, plano de fundo, Alinhar, espelhar na vertical e na
  horizontal), Agrupar/Desagrupar, Mesclar (Ctrl+T, só o objeto selecionado), Mesclar tudo (Ctrl+Shift+T), Aparar,
  Redimensionar imagem/canvas, Aplicar modelo, Girar (horário, anti-horário, 180°, ângulo personalizado), Encaixe no
  canvas (Ctrl+;) e Etiquetar imagem.
- Editor: Alinhar e Espelhar agem no objeto selecionado (sem objeto, Espelhar vira a imagem inteira); o encaixe no
  canvas gruda bordas e centro do objeto nas da imagem ao mover (Alt desliga na hora).
- Editor: etiquetas por imagem (Imagem › Etiquetar imagem…), gravadas na captura e mostradas nas informações.
- Editor: na janela Personalizar a barra, escolha quantas ferramentas a barra automática mostra (6 a 16).
- Editor: seleção de vários objetos com o Mover — Ctrl/Shift+clique soma ou tira da seleção, arrastar no fundo desenha
  um retângulo de seleção e Ctrl+A seleciona todos. Os selecionados se movem, excluem, mudam de ordem, alinham entre si,
  espelham e mesclam juntos; a mudança de estilo vale para os do mesmo tipo.
- Editor: Agrupar (Ctrl+Shift+G) e Desagrupar (Ctrl+Shift+U): o grupo se move e redimensiona como um objeto só e fica
  gravado no .snap como `<Group>` com os objetos dentro.
- Botão "Pular" na contagem regressiva antes de gravar (o botão de gravar e o atalho também pulam a contagem).
- One Click (*Exibir › Mostrar o One Click*): painel preso à borda da tela que recolhe numa faixa fina e abre com o mouse.
- Presets "Captura com rolagem panorâmica" e "Capturar texto" no topo da lista.
- Seleção *Rolagem panorâmica*: o usuário rola o conteúdo e os quadros são juntados numa imagem só.
- Destino *Texto (OCR)* em *Compartilhar*.
- Menu do "+" dos presets: novo preset com as configurações atuais, novo preset e ajuda; engrenagem e "Adicionar atalho"
  em cada linha da lista.

### Corrigido
- Editor: arrastar capturas da bandeja e da biblioteca para outros programas não funcionava. Agora o arraste começa
  mesmo com um movimento rápido, leva a miniatura junto do cursor e entrega os arquivos também como itens do Shell; o
  Explorer copia e apps como o Paint (que só aceitam "mover") abrem a imagem. Vídeos vão com o caminho da biblioteca e
  nunca são movidos. Soltar o item de volta no próprio Editor não importa mais uma cópia dele.
- Editor: Texto clicando e arrastando: a caixa aparece tracejada durante o arraste e enquanto se digita, fica com a
  largura arrastada e cresce para baixo conforme o texto (antes o texto que passava da altura arrastada era cortado e
  nada aparecia ao arrastar); o texto na edição fica no mesmo lugar em que fica na imagem. O cursor da ferramenta é a
  cruz, como o da Seleção.
- Editor: a barra de rolagem da janela Personalizar a barra (e a do painel de propriedades) agora pode ser arrastada.
- Clique no ícone da bandeja às vezes abria a janela e às vezes capturava: agora um clique captura e o duplo clique
  só abre a janela.
- Vídeos com largura fora do múltiplo de 16 (ex.: 854 px) apareciam inclinados e embaralhados no Editor.
- O player do Editor não tocava o áudio do vídeo; agora toca sincronizado com a imagem e pula os trechos cortados.
- Microfone gravado muito baixo: o gravador aplica ganho automático (até 8×) e um limitador suave na mistura.
- Instalador: escolher *Arquivos de Programas* (ou outra pasta protegida) na instalação "só para mim" falhava com
  "acesso negado" no meio da cópia. Agora o instalador avisa ao sair da página da pasta e oferece reiniciar como
  administrador, para todos os usuários, já com a pasta escolhida e sem pedir a licença de novo.

### Alterado
- Nomes dos presets, temas, efeitos rápidos e carimbos de fábrica aparecem no idioma escolhido.
- O Editor ganhou o mesmo visual da janela de captura: barra de título própria com os menus, cores da cor de destaque do
  Windows, controles modernos no painel de propriedades e barra de status discreta. A barra de título, as cores e os
  controles agora são compartilhados pelos dois programas (`src/common/CaptionBar`, `Theme`, `Widgets`).
- Janela de captura mais estreita, com barra de título própria (ícone e menus na mesma faixa), botão de captura vermelho
  sem texto embaixo, ícones das abas no estilo material e fonte Segoe UI Variable.
- A engrenagem de ajustes fica dentro das caixas Seleção, Compartilhar e Qualidade.
- "Salvar em arquivo" virou o destino *Arquivo* em *Compartilhar*; no lugar dele fica *Capturar o cursor*, como na referência.
- Atraso virou uma linha "Atraso  Desligado ›" com menu; quadros/s, redução para 1080p e codificação por hardware foram
  para a engrenagem da Qualidade.
- Editor: *Imagem › Efeitos* abre o efeito no painel de efeitos (com prévia ao vivo) em vez de uma janela de diálogo.
- Editor: uma ferramenta de desenho só pega objetos do mesmo tipo; sobre os outros ela desenha (dá para marcar ou
  apontar um texto sem movê-lo). Ctrl continua forçando desenhar sobre um objeto do mesmo tipo.
- Editor: arrastar um item na barra o leva para a posição de destino (antes trocava os dois de lugar).
- Editor: Grab Text (OCR) passou para Ctrl+Shift+O (Ctrl+Shift+T agora é Mesclar tudo, como na referência);
  "Cortar para a seleção" foi para o menu Editar.
- Editor: barra de ferramentas mais compacta; ícones novos de Carimbo, Passos, Recortar faixa e Preencher (balde
  inclinado com a gota de tinta caindo); "Passo" virou "Passos".
- Editor: o botão "Mais" abre uma lista própria, no estilo material, com o ícone, o nome e o atalho de cada ferramenta
  (antes era um menu comum do Windows só com texto).
- Editor: com o Mover, arrastar no fundo agora seleciona objetos (rolar a imagem continua com o botão do meio, a roda e as
  barras de rolagem).
- Editor: efeitos renomeados como no menu da referência: Bordas, Informações da captura, Substituição de cor e
  Spotlight e lupa.

## [0.1.0] — 2026-10-24

Primeira versão de pré-lançamento: os recursos principais já funcionam, mas o programa ainda está em desenvolvimento.

### Captura (Snapnab.exe)
- Captura de região, janela, tela inteira, monitor, região fixa, mão livre, várias áreas, objeto, rolagem,
  janela estendida, menu, área de transferência e arquivo.
- Gravação de vídeo MP4 (H.264 + AAC) com microfone e áudio do sistema, pausa, painel de controle fora do vídeo.
- Presets com atalhos globais, efeitos, destinos (Editor, área de transferência, arquivo, programa externo).
- 12 efeitos de captura: borda, filtros, sombra, informações da captura, substituição de cor, redimensionar,
  bordas especiais, cortar bordas, ajuste de cor, girar, marca d'água e resolução.
- Janela de captura compacta, com controles modernos e cores que acompanham a cor de destaque do Windows.

### Editor (SnapnabEditor.exe)
- Ferramentas de anotação (seta, formas, texto, balões, passos, carimbos, desfoque, destaque, lupa…),
  seleção com Smart Move, varinha mágica e preenchimento inteligente.
- Efeitos de imagem, OCR (Grab Text), exportação PNG/JPG/BMP/GIF/TIFF/PDF/SNAP, impressão e e-mail.
- Edição de vídeo: cortes, salvar quadro, GIF animado e exportação MP4.
- Biblioteca de capturas com miniaturas.
