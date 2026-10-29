#!/usr/bin/env bash
# Startet NOTICE im LAN. Beim ersten Start wird ein Token erzeugt und in .notice-token gespeichert.
# Nur für vertrauenswürdige Netze: die Verbindung ist HTTP (kein TLS) – siehe README/Anleitung.
set -euo pipefail
cd "$(dirname "$0")"

TOKEN_FILE=".notice-token"
PORT="${PORT:-8080}"

# Token: Umgebungsvariable > gespeicherte Datei > neu erzeugen
if [[ -z "${NOTICE_TOKEN:-}" ]]; then
    if [[ ! -s "$TOKEN_FILE" ]]; then
        umask 077
        cargo run -q --release -- --gen-token > "$TOKEN_FILE"
        echo "Neuer Token gespeichert in $TOKEN_FILE"
    fi
    export NOTICE_TOKEN="$(cat "$TOKEN_FILE")"
fi

# LAN-IP des Rechners ermitteln (Route Richtung Internet, Fallback localhost)
LAN_IP="$(ip -4 route get 1.1.1.1 2>/dev/null | awk '{for(i=1;i<=NF;i++) if($i=="src"){print $(i+1); exit}}')"
LAN_IP="${LAN_IP:-127.0.0.1}"

echo "NOTICE startet auf http://$LAN_IP:$PORT"
echo "Login im Browser mit dem Inhalt von $TOKEN_FILE"
exec cargo run --release -- --host "$LAN_IP" --port "$PORT"
