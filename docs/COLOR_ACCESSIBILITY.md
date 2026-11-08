# Color accessibility and contrast policy (BG/FG)

**Goal:** automatically preserve the readability of every interface element across
themes, colors, and component states. This policy applies to LYRA's own interface
and to the desktop interfaces it proposes or modifies.

## Mandatory requirements

1. **Meet WCAG 2.2 AA as a minimum:**
   - Normal text: contrast ≥ 4.5:1.
   - Large text (≥ 24 px regular or ≥ 18.7 px bold): contrast ≥ 3:1.
   - Essential icons and graphical components: contrast ≥ 3:1.
   - Aim for AAA (≥ 7:1 for normal text) whenever possible.
2. **Check every background/foreground pair before using it.**
3. **Use the official WCAG calculation:** linearize sRGB channels, calculate
   relative luminance, then `(Lmax + 0.05) / (Lmin + 0.05)`. Visual judgment and
   a simple brightness difference are not substitutes.
4. **Automatically correct insufficient contrast:** adjust FG first to preserve
   BG; adjust BG if needed. Preserve the design system and visual identity as far
   as readability allows. Never sacrifice legibility for aesthetics.
5. **Cover every context:** light/dark themes; buttons, forms, links, badges,
   tables, menus and notifications; hover, focus, active, selected and disabled
   states; transparency, gradients, images, nested components and inherited colors.
6. **Prefer semantic tokens:** `--color-background`, `--color-foreground`,
   `--color-surface`, `--color-text-secondary`, `--color-border`, and
   `--color-on-primary`. Avoid unvalidated hardcoded BG/FG pairs.
7. **Integrate development checks:** create or reuse `getContrastRatio(bg, fg)`,
   test component color pairs, automate tests when possible, and report failures.

## Decision rule

**Every new interface or visual change must preserve WCAG AA contrast at minimum,
except for exemptions explicitly provided by the standard.** Correct failing
pairs directly instead of leaving accessibility defects.

**Priority: Readability > Normative accessibility > Design-system consistency > Aesthetics.**

LYRA uses semantic Slint `Theme` tokens for its native interface. The shared
`lyra_check_contrast` tool computes ratios and proposes corrected colors. Known
colors alone do not establish contrast on an unknown image or gradient: test the
worst actual background behind the element, or add a verified opaque surface.
Inactive controls are exempt under the standard, but must stay identifiable.
Do not infer font size or state from a palette alone.

References: [WCAG 2.2](https://www.w3.org/TR/WCAG22/),
[minimum text contrast](https://www.w3.org/WAI/WCAG22/Understanding/contrast-minimum.html),
[non-text contrast](https://www.w3.org/WAI/WCAG22/Understanding/non-text-contrast.html).
