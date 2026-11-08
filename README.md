# LYRA

**Réglez votre bureau Hyprland en discutant.**

LYRA est une application de chat native pour explorer votre configuration,
essayer des réglages et choisir l’apparence de votre bureau. Elle fonctionne
avec **Zcode, Claude Code ou Codex**.

![Interface de LYRA : suggestions de réglages, chat et sélecteur d’abonnement](docs/images/lyra.png)

## Ce que vous pouvez lui demander

- « Montre l’état de mes écrans et mes raccourcis. »
- « Passe les gaps à 10 pour essayer, puis remets-les comme avant. »
- « Propose-moi trois palettes de couleurs et laisse-moi choisir. »

Vous pouvez cliquer sur les propositions de LYRA : textes, couleurs, images ou
schémas. Le bouton **Joindre** permet aussi d’envoyer vos images et fichiers texte.

## Installer depuis les sources

Il vous faut **Linux avec Hyprland, Git, Rust/Cargo et systemd**.
Le projet est testé avec Hyprland **0.57.1 et sa configuration Lua**.
Installez aussi `grim` pour les captures d’écran et `zenity` pour joindre des fichiers.

```bash
git clone https://github.com/jacquesh82/mindlog.hyprland_tools.git
cd mindlog.hyprland_tools
./scripts/install.sh
~/.local/bin/hyprpanel-chat
```

Le script compile l’application, installe les binaires dans `~/.local/bin`
et démarre le service local nécessaire aux réglages du bureau.

## Choisir votre abonnement

Dans le menu près d’**Envoyer**, choisissez le fournisseur à utiliser.
Votre choix est mémorisé et vous pouvez le changer entre deux messages.

| Fournisseur | Comment le connecter |
| --- | --- |
| **Zcode** | La clé du Coding Plan Z.ai est reprise de votre configuration Zcode si elle est disponible. Sinon, renseignez-la dans les réglages de LYRA. |
| **Claude Code** | Installez le client `claude`, puis connectez votre abonnement avec `claude auth login`. |
| **Codex** | Installez le client `codex`, puis connectez votre compte ChatGPT avec `codex login`. |

Avec Zcode, le texte apparaît progressivement. Avec Claude Code et Codex,
la réponse s’affiche une fois validée ; l’indicateur de réflexion reste visible
pendant l’attente.

## Réglages et sauvegardes

- **⚙ Réglages** : langue français/anglais, modèle et animation de l’avatar.
- **⟲ Historique** : retrouvez les sauvegardes et restaurez une configuration.
- **Détails des outils** : dépliez le lien dans le chat si vous souhaitez voir les opérations effectuées.

Les réglages à chaud servent à tester des changements. Une sauvegarde de la
configuration est demandée avant chaque message.

La conversation est conservée le temps de la fenêtre. Les sauvegardes de
configuration, elles, restent disponibles après la fermeture.

## Besoin d’aide ?

Le bouton **?** dans LYRA présente les fonctions et le fonctionnement du chat.
Pour vérifier vos connexions :

```bash
~/.local/bin/hyprpanel-chat check-providers
```

Le [guide technique](docs/TECHNICAL.md) détaille le dépannage, la configuration,
l’utilisation du serveur MCP et les commandes pour contribuer au projet.

## Licences

L’application LYRA (`hyprpanel-chat`) est sous **GPL-3.0-only**.
Le cœur et le serveur (`hyprpanel-core`, `hyprpanel-server`) sont sous **MIT**.
