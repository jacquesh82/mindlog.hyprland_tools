# Avatar LYRA

`lyra-avatar-blink.png` est la variante aux yeux fermés, générée avec l’outil
intégré **imagegen**, utilisée pour le clignement. Les mouvements de respiration
et de tête sont produits par les transformations du composant Slint.

Prompt final du clignement :

> Edit target: the attached LYRA avatar PNG. Create a perfectly aligned alternate animation frame of this EXACT character, changing ONLY its TWO cyan eyes to a gentle CLOSED-EYE blink: replace each large glowing eye with a slim curved luminous cyan eyelid line following the same expression and position. Preserve the entire robot head, shell, headphones, faceplate reflections, smile, collar, silhouette, pose, camera, lighting, colors, size, crop, placement and transparent alpha background exactly unchanged. Do not redraw or reposition the head. This image will alternate with the original to animate blinking, so registration and identical geometry are essential. One square image only, not a spritesheet, no text, no new objects. Genuine transparent background.

`lyra-avatar.png` est l'avatar original de LYRA, généré avec l'outil intégré
**imagegen** le 8 novembre 2026. Le PNG conserve son fond transparent. Il est
réduit avec sa variante en textures RGBA 256² pendant le build, puis partagé
entre les avatars Slint dans l'en-tête, l'accueil et les réponses de
l'assistante. Les icônes SVG sont dessinées séparément dans `icons/`.

Prompt final utilisé :

> Use case: stylized-concept. Asset type: production avatar for LYRA, a premium native dark desktop assistant that configures Hyprland / Wayland. Create one original, beautifully crafted floating AI companion, head and tiny upper collar only, centered, square composition, genuinely transparent background. Subject: a softly rounded pearlescent silver-lavender robotic head with a dark midnight glass faceplate, two expressive simple luminous cyan eyes, a subtle friendly expression, compact sculptural side fins resembling headphones, one elegant lilac accent. Sophisticated 3D clay / industrial design rendering, tactile matte ceramic shell and restrained glass details, soft studio illumination, gentle cyan edge light. Warm, calm, intelligent and approachable, professional software brand character, recognisable even at 32px. Front three-quarter view, large face, silhouette fills about 80% of canvas with comfortable margins. No body, no hands, no scenery, no pedestal, no text, no letters, no logos, no watermarks, no star particles, no surrounding objects. Preserve real alpha transparency.
