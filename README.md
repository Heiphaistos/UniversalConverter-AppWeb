# UniversalConverter Web
## Démonstration

<video controls width="100%" preload="none">
  <source src="https://media.heiphaistos.org/videos/universalconverter.mp4" type="video/mp4">\n</video>\n
Version web de UniversalConverter — conversion de fichiers en ligne (images, PDF, Word, Excel, PowerPoint, CSV, JSON), hébergée sur VPS.

**Prod : https://universalconverter.heiphaistos.org**

## Architecture

- `server/` — Rust **axum** : API + frontend statique, réutilise les engines du desktop (audit v1.7.0)
- `web/` — **React 19 + Vite + Tailwind v4** : UI identique à la version desktop
- `deploy/` — script déploiement VPS + vhost nginx

### API

| Route | Méthode | Détail |
|-------|---------|--------|
| `/api/convert` | POST | multipart `file` + `output_format` [+ `quality`, `resize_width`, `resize_height`, `rotation`, `output_name`] → fichier converti |
| `/api/merge-pdf` | POST | multipart `files` (2+) + `mode` (pages\|single) → PDF |
| `/api/split-pdf` | POST | multipart `file` + `pages` ("1,3,5") → PDF |
| `/api/pdf-page-count` | POST | multipart `file` → `{pages}` |
| `/api/health` | GET | `{status, version}` |

### Conversions supportées

Identiques au desktop : images raster ↔ images / PDF, SVG → raster/PDF, PDF → TXT/HTML + split,
TXT/MD/HTML → PDF, DOCX/PPTX → TXT/HTML/PDF, Excel → CSV/JSON/TXT/PDF, CSV ↔ JSON/XLSX.

### Protections (accès public)

- Upload max **60 MB**
- Rate-limit **20 req/min/IP**
- Max **2 conversions simultanées** (sémaphore)
- Timeout requête 120 s
- Fichiers temporaires RAII (suppression garantie)
- Écoute `127.0.0.1:3003` uniquement (exposé via nginx)

## Dev local

```bash
cd server && cargo run          # backend :3003
cd web && npm install && npm run dev   # frontend :1421 (proxy /api)
```

## Déploiement VPS (212.227.140.45)

```bash
# 1. Build frontend local : cd web && npm run build (dist/ inclus dans le sync)
# 2. Sync vers /opt/universalconverter puis sur le VPS :
cd /opt/universalconverter && bash deploy/deploy.sh

# 3. nginx (une fois) :
cp deploy/nginx-universalconverter.conf /etc/nginx/sites-available/universalconverter
ln -s /etc/nginx/sites-available/universalconverter /etc/nginx/sites-enabled/
nginx -t && systemctl reload nginx
certbot --nginx -d universalconverter.heiphaistos.org --non-interactive --agree-tos --redirect

# 4. DNS Ionos : A universalconverter → 212.227.140.45
```

## Versions

- **1.0.0** (2026-06-11) — portage web initial depuis UniversalConverter desktop v1.7.0