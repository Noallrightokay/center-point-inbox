#!/usr/bin/env bash
# =============================================================================
# Center Point Inbox — Quick Setup Script
# Usage: chmod +x setup.sh && ./setup.sh
# =============================================================================
set -e

BOLD="\033[1m"
GOLD="\033[33m"
GREEN="\033[32m"
RESET="\033[0m"

echo ""
echo -e "${GOLD}${BOLD}  ╔═══════════════════════════════════════════╗${RESET}"
echo -e "${GOLD}${BOLD}  ║       CENTER POINT INBOX — MVP Setup      ║${RESET}"
echo -e "${GOLD}${BOLD}  ╚═══════════════════════════════════════════╝${RESET}"
echo ""

# ── Check prerequisites ──────────────────────────────────────────────────────
check_command() {
    if ! command -v "$1" &> /dev/null; then
        echo -e "  ✗ $1 is not installed. Please install it first."
        exit 1
    else
        echo -e "  ${GREEN}✓${RESET} $1 found"
    fi
}

echo -e "${BOLD}Checking prerequisites...${RESET}"
check_command docker
check_command "docker compose" 2>/dev/null || check_command docker-compose
echo ""

# ── Create .env if missing ───────────────────────────────────────────────────
if [ ! -f .env ]; then
    echo -e "${BOLD}Creating .env from template...${RESET}"
    cp .env.example .env
    echo -e "  ${GREEN}✓${RESET} .env created (edit it to add OAuth keys if needed)"
    echo ""
fi

# ── Generate package-lock.json if missing ────────────────────────────────────
if [ ! -f src/frontend/package-lock.json ]; then
    echo -e "${BOLD}Generating frontend lock file...${RESET}"
    if command -v npm &> /dev/null; then
        (cd src/frontend && npm install --package-lock-only)
        echo -e "  ${GREEN}✓${RESET} package-lock.json generated"
    else
        echo -e "  ⚠ npm not found — Docker build will handle dependencies"
    fi
    echo ""
fi

# ── Build and start ─────────────────────────────────────────────────────────
echo -e "${BOLD}Building and starting all services...${RESET}"
echo -e "  This may take 3-5 minutes on the first run."
echo ""

docker compose up -d --build

echo ""
echo -e "${BOLD}Waiting for services to become healthy...${RESET}"

# Wait up to 120 seconds for API gateway
TIMEOUT=120
ELAPSED=0
while [ $ELAPSED -lt $TIMEOUT ]; do
    if curl -sf http://localhost:5000/health/live > /dev/null 2>&1; then
        break
    fi
    sleep 5
    ELAPSED=$((ELAPSED + 5))
    echo -e "  Waiting... (${ELAPSED}s / ${TIMEOUT}s)"
done

if [ $ELAPSED -ge $TIMEOUT ]; then
    echo ""
    echo -e "  ⚠ Some services may still be starting. Check: docker compose ps"
else
    echo -e "  ${GREEN}✓${RESET} API Gateway is healthy"
fi

# Wait briefly for frontend
sleep 5

echo ""
echo -e "${GOLD}${BOLD}═══════════════════════════════════════════════${RESET}"
echo -e "${GOLD}${BOLD}  Center Point Inbox is running!${RESET}"
echo -e "${GOLD}${BOLD}═══════════════════════════════════════════════${RESET}"
echo ""
echo -e "  ${BOLD}Web App:${RESET}           http://localhost:3000"
echo -e "  ${BOLD}API Gateway:${RESET}       http://localhost:5000"
echo -e "  ${BOLD}RabbitMQ Console:${RESET}  http://localhost:15672"
echo -e "  ${BOLD}MinIO Console:${RESET}     http://localhost:9001"
echo ""
echo -e "  ${BOLD}Chrome Extension:${RESET}  Load 'extension/' folder at chrome://extensions"
echo -e "  ${BOLD}PWA Install:${RESET}       Visit http://localhost:3000 → click install icon"
echo ""
echo -e "  To stop:  ${GOLD}docker compose down${RESET}"
echo -e "  Logs:     ${GOLD}docker compose logs -f <service>${RESET}"
echo ""
