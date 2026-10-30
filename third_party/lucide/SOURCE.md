# Lucide icons

Source: https://lucide.dev/icons/
Upstream: https://github.com/lucide-icons/lucide/tree/be09619c1c1124ad09c19fc3e87ee3b939409bbd/icons
SVGs in assets/lucide are unmodified. The renderer replaces currentColor with white
for theme tinting in memory. ISC/Feather MIT notices: LICENSE.
Same pinned icon source as ReplayLite 6a7dc0c. No network loading at runtime.

assets/app.svg derives from sparkles.svg, with a violet background.
app.png and multi-size app.ico are rasterized from app.svg using egui_extras/resvg
and Pillow (build-time conversion only). The same notices apply.

Regenerate PNG/ICO: `python tools/render_icon.py` (developer Rust/Pillow only).
