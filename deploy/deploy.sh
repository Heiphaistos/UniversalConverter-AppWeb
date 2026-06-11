#!/bin/bash
# deploy.sh — Déploiement UniversalConverter Web sur VPS Debian 13
# Usage : exécuter SUR le VPS depuis /opt/universalconverter
set -euo pipefail

APP_DIR="/opt/universalconverter"
LOG() { echo "[$(date -Iseconds)] [INFO] $*"; }

cd "$APP_DIR"

# ── 1. Rust toolchain ──────────────────────────────────────────────
if ! command -v cargo >/dev/null 2>&1; then
    source "$HOME/.cargo/env" 2>/dev/null || {
        LOG "Installation rustup..."
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
        source "$HOME/.cargo/env"
    }
fi

# ── 2. Build serveur ───────────────────────────────────────────────
LOG "cargo build --release..."
cd "$APP_DIR/server"
cargo build --release
LOG "Binaire : $APP_DIR/server/target/release/universalconverter-server"

# ── 3. PM2 ─────────────────────────────────────────────────────────
cd "$APP_DIR"
if pm2 describe universalconverter >/dev/null 2>&1; then
    LOG "Restart PM2 universalconverter..."
    pm2 restart universalconverter --update-env
else
    LOG "Création process PM2 universalconverter (port 3003)..."
    STATIC_DIR="$APP_DIR/web/dist" \
    PORT=3003 \
    pm2 start "$APP_DIR/server/target/release/universalconverter-server" --name universalconverter
    pm2 save
fi

# ── 4. Vérification ────────────────────────────────────────────────
sleep 2
curl -fsS http://127.0.0.1:3003/api/health && echo "" && LOG "Déploiement OK ✅"
