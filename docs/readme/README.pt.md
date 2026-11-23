# MMO Mouse Control

[日本語](../../README.md) · [English](README.en.md) · [简体中文](README.zh-Hans.md) · [繁體中文](README.zh-Hant.md) · [한국어](README.ko.md) · [Español](README.es.md) · [Français](README.fr.md) · [Deutsch](README.de.md) · **Português**

Ferramenta para Windows 10/11 (x64) para configurar mouses com vários botões. Ajuste a atribuição de botões, o DPI, a iluminação e os perfis salvos no mouse. Também é possível fazer backup e restaurar as configurações.

## Instalação

Baixe **MMOMouseControl-Setup-0.12.4-x64.exe** em [GitHub Releases](https://github.com/nekomimiyks/mmo-mouse-control/releases/latest) e execute o arquivo. Se usava a versão portátil, selecione a pasta dela durante a instalação. No final, escolha **Sim** para abrir o aplicativo ou **Não** para sair.

Durante a atualização, o instalador fecha a cópia instalada antes de substituí-la. Se o aplicativo aguardar uma confirmação, responda e execute o instalador novamente.

## Novidades da versão 0.12.4

A instalação cria um atalho na área de trabalho. Na edição do GitHub, use **Configurações de atualização** para ativar a verificação ao iniciar. Ela vem desativada. A Microsoft Store gerencia as atualizações da sua edição. Quando houver uma nova versão, escolha **Atualizar** para baixar e iniciar o instalador verificado.

## Fazer backup e restaurar configurações

Em **Backup**, use **Escolher onde salvar** para criar um arquivo `.mmo-backup`. Abra o arquivo com **Carregar configurações**. Escolha **Sim** para gravar os perfis no mouse ou **Não** para visualizá-los. As preferências entram em vigor na próxima inicialização.

## Compatibilidade

- Testados com os dispositivos: Redragon M908 Impact com fio e Logitech G600r (Logicool G600r no Japão).
- Compatibilidade esperada com base nos IDs USB publicados: [Monster Pusat V8 MMO](https://github.com/dokutan/mouse_m908/blob/965d71f957023ebdfe9bc3ea8d2affbfb21c10ff/README.md#supported-mice) com fio e [Logitech G600](https://github.com/libratbag/piper/blob/master/data/svgs/svg-lookup.ini). Esses modelos ainda não foram testados com este aplicativo.

## Idiomas do aplicativo

Japonês, inglês, chinês simplificado, chinês tradicional, coreano, espanhol, francês, alemão e português.

Abra **G600 / G600r → Configurações → Idioma**. **Automático / Windows** usa o idioma de exibição do Windows e usa o inglês se esse idioma não estiver disponível. Essa configuração se aplica à tela do G600 / G600r. A tela do M908 / Pusat V8 está em japonês.

## Capturas de tela

As capturas mostram configurações de exemplo.

### Confirmação da restauração

![Confirmação da restauração](../../screenshots/localized/pt/backup.png)

### Configurações do G600r

![Configurações do G600r](../../screenshots/localized/pt/settings.png)

### Atribuição de botões do G600r

![Atribuição de botões do G600r](../../screenshots/localized/pt/buttons.png)

## Compilar a partir do código-fonte

No Windows, prepare a cadeia de ferramentas Rust GNU, o compilador de recursos do MinGW e as dependências do Cargo. Execute **./build.ps1** no PowerShell.

[Licença](../../LICENSE) · [Avisos de terceiros](../../THIRD_PARTY.md) · [Política de privacidade](../../PRIVACY.md)
