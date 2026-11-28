// SPDX-License-Identifier: GPL-3.0-or-later

import type { MarketingKey } from "./zh";

export const ptBr: Record<MarketingKey, string> = {
  "head.title": "ice-box — a melhor sing-box GUI",
  "head.description":
    "ice-box é um cliente de proxy leve para macOS, Windows, Linux e Android: fácil de configurar, rápido para iniciar, econômico em recursos e pronto para Agentes via MCP.",
  "nav.languageAria": "Escolher idioma",
  "nav.themeToLight": "Alternar para o modo claro",
  "nav.themeToDark": "Alternar para o modo escuro",
  "nav.features": "Recursos",
  "nav.agent": "Agente",
  "nav.download": "Download",
  "hero.release": "{version} já disponível",
  "hero.title": "A melhor <span class=\"accent\">sing-box GUI</span>",
  "hero.subtitle":
    "Um cliente de proxy leve para macOS, Windows, Linux e Android. Importe uma assinatura, escolha um nó e ative a chave. Ele fica discreto enquanto roda, e seu Agente pode trabalhar ao seu lado via MCP.",
  "hero.installer": "Download gratuito",
  "hero.installerPlatform": "Baixar para {platform}",
  "hero.tryDemo": "Testar demo ao vivo",
  "hero.platforms": "macOS · Windows · Linux · Android",
  "demo.windowBar": "demo interativa do produto",
  "demo.iframeTitle": "Demo ao vivo do app desktop ice-box",
  "features.kicker": "Recursos",
  "features.title": "Tudo o que você precisa, sempre à mão",
  "features.body":
    "De importar uma assinatura a inspecionar uma conexão, as tarefas do dia a dia estão a apenas um ou dois cliques.",
  "features.groupDaily": "No dia a dia",
  "features.groupBackground": "Em segundo plano",
  "features.subs.title": "Assinaturas",
  "features.subs.body":
    "Detecta sing-box JSON, Clash e links de compartilhamento automaticamente, além de importar arquivos locais. No desktop, as assinaturas atualizam sozinhas, exibindo tráfego restante e validade num relance.",
  "features.routing.title": "Roteamento",
  "features.routing.body":
    "Alterne entre Regra, Global e Direto quando quiser. Ative ou desative cada regra da assinatura individualmente e sobreponha suas próprias regras.",
  "features.nodes.title": "Nós",
  "features.nodes.body":
    "Escolha uma saída para cada grupo de estratégia, teste a latência de um nó ou da lista inteira e fixe seus favoritos.",
  "features.connections.title": "Conexões",
  "features.connections.body":
    "No desktop (macOS, Windows e Linux), veja cada conexão ativa: destino, regra correspondente, saída e tráfego. Pesquise, ordene ou encerre em um clique.",
  "features.capture.title": "Captura",
  "features.capture.body":
    "Use o proxy do sistema ou TUN no desktop. No Android, o ice-box roda como VPN do sistema com roteamento por app: seguir regras, proxy, direto ou bloquear.",
  "features.tray.title": "Bandeja",
  "features.tray.body":
    "Inicie ou pare o proxy e troque modos, assinaturas ou saídas sem abrir a janela. No macOS, a barra de menus também mostra o tráfego ao vivo.",
  "features.upkeep.title": "Manutenção discreta",
  "features.upkeep.body":
    "Inicialização no login, atualizações no app verificadas por assinatura, tráfego e memória em tempo real no Início, 16 idiomas com suporte completo a RTL e temas claro e escuro.",
  "agent.kicker": "Pronto para Agentes",
  "agent.title": "Seu Agente trabalha na mesma sessão que você",
  "agent.body":
    "O app desktop inclui um serviço MCP local opcional. Ative-o e entregue a configuração ao seu Agente: ele pode consultar o status, alternar modos e nós, gerenciar assinaturas e regras e inspecionar conexões, tudo sobre os mesmos dados exibidos na janela.",
  "agent.point1": "Sem processo extra em segundo plano, sem duplicar o diretório de dados",
  "agent.point2": "Você e seu Agente veem as alterações um do outro instantaneamente",
  "agent.point3": "Desativado por padrão e escuta apenas nesta máquina",
  "agent.terminalTitle": "Sessão do Agente",
  "agent.prompt": "Mude para o modo Global e me mostre as conexões com mais tráfego",
  "agent.reply":
    "Alterado para Global. Das 8 conexões, googlevideo.com (YouTube) é a que mais consome tráfego, roteada via Los Angeles / edge-03.",
  "phoneDemo.iframeTitle": "Demo ao vivo do app Android ice-box",
  "download.kicker": "Download",
  "download.title": "Escolha sua plataforma",
  "download.body": "Os instaladores ficam hospedados no GitHub Releases. Sem cadastro.",
  "download.tabListAria": "Escolher sistema operacional",
  "download.actionPlatform": "Baixar para {platform}",
  "download.yours": "Este dispositivo",
  "download.allReleases": "Ver todas as versões e o changelog",
  "download.macos.name": "macOS",
  "download.macos.kind": "Apple Silicon",
  "download.macos.requires": "macOS 12 ou posterior",
  "download.macos.note":
    "Se o macOS informar que o desenvolvedor não pode ser verificado na primeira abertura, permita o app em Ajustes do Sistema → Privacidade e Segurança.",
  "download.windows.name": "Windows",
  "download.windows.kind": "64 bits",
  "download.windows.requires": "Windows 10 ou posterior",
  "download.windows.note":
    "Instala para o usuário atual sem privilégios de administrador. O SmartScreen pode pedir confirmação.",
  "download.linux.name": "Linux",
  "download.linux.requires": "Ubuntu 22.04+ e distros modernas",
  "download.linux.note":
    "AppImage inclui atualizações assinadas no app; .deb atualiza pelo gerenciador de pacotes. O proxy do sistema suporta GNOME/KDE; o TUN usa systemd e Polkit.",
  "download.linux.archAria": "Escolher arquitetura do Linux",
  "download.linux.appimageAction": "Baixar AppImage",
  "download.linux.debAction": "Baixar .deb",
  "download.android.name": "Android",
  "download.android.kind": "arm64",
  "download.android.requires": "Android 14 ou posterior",
  "download.android.note":
    "Ainda não está em nenhuma loja de apps. Quando o Android pedir, permita instalações desta fonte.",
  "download.cli.subtitle":
    "CLI e daemon sem interface para macOS, Windows e Linux — feitos para agentes, servidores e scripts.",
  "download.cli.shellAria": "Escolher comando de instalação da CLI",
  "download.cli.copy": "Copiar",
  "download.cli.copied": "Copiado",
  "download.cli.archives": "Arquivos diretos",
  "download.cli.docs": "Docs da CLI",
  "footer.source": "Código-fonte",
  "footer.releases": "Versões",
  "footer.changelog": "Changelog",
  "footer.license": "Licenciado sob GPL-3.0-or-later",
};
