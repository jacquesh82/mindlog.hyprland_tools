# Règle — Accessibilité et contraste des couleurs (BG/FG)

**Objectif :** garantir automatiquement la lisibilité de tous les éléments d'interface, quels que soient le thème, les couleurs ou les états des composants.

## Exigences obligatoires

1. **Respecter WCAG 2.2 niveau AA au minimum :**
   - Texte normal : contraste ≥ 4.5:1.
   - Grand texte (≥ 24 px normal ou ≥ 18.7 px gras) : contraste ≥ 3:1.
   - Icônes et composants graphiques essentiels : contraste ≥ 3:1.
   - Viser WCAG AAA (≥ 7:1) pour les textes lorsque cela est possible.

2. **Vérifier systématiquement chaque association `background` (BG) / `foreground` (FG)** avant de l'utiliser dans l'interface.

3. **Calculer le contraste avec la formule officielle WCAG :**
   - Convertir les couleurs sRGB en luminance relative.
   - Ratio = `(Lmax + 0.05) / (Lmin + 0.05)`.
   - Ne jamais se fier uniquement à une appréciation visuelle.

4. **Corriger automatiquement les contrastes insuffisants :**
   - Privilégier l'ajustement de FG pour préserver BG.
   - Si nécessaire, ajuster BG.
   - Conserver autant que possible l'identité visuelle et les couleurs du design system.
   - Ne jamais sacrifier la lisibilité pour des raisons esthétiques.

5. **Gérer tous les contextes :**
   - Thèmes clair et sombre.
   - Boutons, formulaires, liens, badges, tableaux, menus et notifications.
   - États hover, focus, active, selected, disabled.
   - Fonds transparents, dégradés et images.
   - Composants imbriqués et couleurs héritées.

6. **Privilégier les tokens sémantiques :**
   - `--color-background`
   - `--color-foreground`
   - `--color-surface`
   - `--color-text-secondary`
   - `--color-border`
   - `--color-on-primary`
   - Éviter les couleurs FG/BG codées en dur sans validation de contraste.

7. **Intégrer les contrôles dans le développement :**
   - Créer ou réutiliser une fonction `getContrastRatio(bg, fg)`.
   - Tester les combinaisons de couleurs des composants.
   - Ajouter des tests automatisés lorsque l'environnement le permet.
   - Signaler toute combinaison non conforme.
   - Ne pas utiliser une simple différence de luminosité comme substitut au calcul WCAG.

## Règle de décision

**Toute nouvelle interface ou modification visuelle doit conserver un contraste WCAG AA au minimum, sauf exemptions explicitement prévues par la norme.**

Si une combinaison BG/FG est insuffisante, la corriger directement plutôt que de laisser un problème d'accessibilité.

**Priorité : Lisibilité > Accessibilité normative > Cohérence du design system > Esthétique.**
