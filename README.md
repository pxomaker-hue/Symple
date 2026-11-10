# Symple

Synchronisation de dossiers **en temps réel**, légère et rapide (Tauri 2 + Rust). Dossiers locaux ou sur le réseau (NAS, `\\serveur\partage`).

## Fonctionnalités

- Plusieurs paires de dossiers, chacune en **bidirectionnel** (le plus récent gagne) ou en **miroir A → B**.
- Comparaison par **date de modification + taille** (tolérance de 2 s pour FAT/SMB).
- **Temps réel** : surveillance du système de fichiers + vérification complète périodique (indispensable sur NAS/SMB, où la surveillance n'est pas fiable).
- **Suppressions** synchronisées en option : les fichiers vont à la **corbeille** (dans `.symple-trash/AAAA-MM-JJ/` sur les partages réseau, qui n'ont pas de corbeille).
- **Conflits** (modifié des deux côtés), au choix pour chaque paire :
  - *Me demander* : une fenêtre propose de garder A, B ou les deux ;
  - *Plus récent + copie* : l'autre version est renommée `nom (conflit AAAA-MM-JJ HHhMM).ext` ;
  - *Plus récent* : rien n'est conservé.
- **Exclusions** par motifs (`*.tmp`, `Thumbs.db`, `node_modules`, `cache/*`…).
- Copie **atomique** (fichier temporaire puis renommage), date d'origine conservée, copies parallèles.
- Sécurité : si un dossier devient inaccessible ou vide d'un coup (NAS débranché), la synchro est suspendue au lieu de tout supprimer.
- Icône dans la barre des tâches, lancement au démarrage, démarrage réduit. Fermer la fenêtre laisse Symple tourner en arrière-plan.

## Télécharger

Les installateurs Windows (`.exe` NSIS et `.msi`) sont produits par GitHub Actions : onglet **Actions** → dernier build → artefact `symple-windows`. Pousser un tag `v*` crée une release (brouillon).

## Compiler

Prérequis : [Rust](https://rustup.rs), Node.js 18+, et sous Windows WebView2 (déjà présent sur Windows 10/11).

```sh
npm install
npm run dev      # lancement en développement
npm run build    # installateurs dans target/release/bundle/
cargo test -p symple-core
```

## Structure

| Dossier | Rôle |
|---|---|
| `core/` | Moteur de synchro en Rust pur (scan, comparaison, copie, corbeille, conflits, surveillance). Sans dépendance à l'interface, testé. |
| `src-tauri/` | Application : commandes, icône de la barre des tâches, démarrage auto, persistance de la config. |
| `ui/` | Interface HTML/CSS/JS sans framework ni étape de build. |

La configuration et l'état de synchro sont dans `%APPDATA%\org.symple.app\`.

## Licence

MIT
