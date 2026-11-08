# Logo assets

The mark is a five-blade impeller inside a bearing ring, with a damped
response curve below it and a terminal cursor in the hub.

## Files

| File | Use |
|---|---|
| `logo.svg` | Horizontal lockup, 1000×320; social previews and banners |
| `logo-mark.svg` | Square badge, 512×512; README header, avatar, favicon source |
| `logo-mono.svg` | Black-on-transparent lockup for light backgrounds |
| `logo-mono-inverse.svg` | White-on-transparent lockup for dark backgrounds |
| `logo.png` | 1000×320 PNG export |
| `logo-mark-512.png` | 512×512 avatar/PNG export |
| `logo-mark-128.png` | 128×128 PNG export |
| `favicon-32.png` | 32×32 favicon export |
| `social-preview.png` | 1280×640 GitHub social preview |

## Palette

| Role | Color |
|---|---|
| background | `#12131a` |
| bearing / impeller | `#5eead4` |
| response curve / cursor | `#c4b5fd` |
| wordmark | `#f8fafc` |
| muted tagline | `#9aa7ba` |

The monochrome files use `#0f172a` (light) and `#f8fafc` (dark).

## Regenerate

```sh
rsvg-convert -w 1000 -h 320 assets/logo.svg -o assets/logo.png
rsvg-convert -w 512 -h 512 assets/logo-mark.svg -o assets/logo-mark-512.png
rsvg-convert -w 128 -h 128 assets/logo-mark.svg -o assets/logo-mark-128.png
rsvg-convert -w 32 -h 32 assets/logo-mark.svg -o assets/favicon-32.png

magick -size 1280x640 xc:'#12131a' \
  \( assets/logo.png -resize 1000x320 \) -gravity center -composite \
  assets/social-preview.png
```

The wordmark uses `DejaVu Sans` (falling back to Inter/Arial) and the tagline
`DejaVu Sans Mono` (falling back to JetBrains Mono). The SVG keeps the text
editable; use Inkscape's text-to-path export if you need a font-independent
version.
