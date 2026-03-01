# Center Point Inbox — Quickstart Guide

## Prerequisites

- **Docker Desktop** (v4.0+) with Docker Compose v2
- **Node.js 20+** and **npm** (for local frontend dev)
- **Git** (to clone/extract the project)

---

## Option A: One-Command Start (Docker Compose)

This spins up everything — PostgreSQL, Redis, RabbitMQ, MinIO, all backend microservices, and the frontend.

```bash
# 1. Extract the ZIP and navigate into the project
cd center-point-inbox

# 2. Copy the environment template (customize if needed)
cp .env.example .env

# 3. Start all services
docker compose up -d --build

# 4. Wait for health checks (takes ~2-3 minutes on first build)
docker compose ps
```

Once healthy, open **http://localhost:3000** in your browser.

### Service URLs

| Service               | URL                         |
|-----------------------|-----------------------------|
| Frontend (Web App)    | http://localhost:3000        |
| API Gateway           | http://localhost:5000        |
| Auth Service          | http://localhost:5001        |
| Google Provider       | http://localhost:5002        |
| Microsoft Provider    | http://localhost:5003        |
| Merger Service        | http://localhost:5004        |
| AI Assistant          | http://localhost:5006        |
| Audit & DLP           | http://localhost:5007        |
| Email Service (IMAP)  | http://localhost:5008        |
| RabbitMQ Console      | http://localhost:15672       |
| MinIO Console         | http://localhost:9001        |

### Default Credentials (Dev Only)

| System    | Username          | Password                |
|-----------|-------------------|-------------------------|
| PostgreSQL| cpi_admin         | cpi_dev_password_2024   |
| Redis     | (no user)         | cpi_redis_password      |
| RabbitMQ  | cpi_rabbit        | cpi_rabbit_password     |
| MinIO     | cpi_minio_admin   | cpi_minio_password      |

---

## Option B: Local Frontend Dev (Hot Reload)

If you want to iterate on the UI with hot reloading:

```bash
# 1. Start just the infrastructure + backend services
docker compose up -d postgres redis rabbitmq minio auth-service api-gateway \
  google-provider microsoft-provider merger-service translation-worker \
  ai-assistant audit-dlp email-service

# 2. Install frontend dependencies
cd src/frontend
npm install

# 3. Start Next.js dev server
npm run dev
```

Frontend runs at **http://localhost:3000** with hot reload.

---

## Option C: Load the Chrome Extension

1. Open Chrome and navigate to `chrome://extensions/`
2. Enable **Developer mode** (toggle in top-right)
3. Click **Load unpacked**
4. Select the `extension/` folder from this project
5. The Center Point Inbox extension icon appears in your toolbar

The extension connects to your running frontend at `http://localhost:3000`.

---

## Option D: Install as PWA (Mobile/Desktop App)

1. Open **http://localhost:3000** in Chrome
2. Click the install icon in the address bar (or Menu > "Install Center Point Inbox")
3. The app installs as a standalone window with offline support

---

## Connecting Email Accounts

### Google / Microsoft OAuth
Click "Sign in with Google" or "Sign in with Microsoft" on the login page.
Requires OAuth credentials — set `Google__ClientId`, `Google__ClientSecret`, etc. in `.env`.

### Any Email via IMAP (Yahoo, iCloud, ProtonMail, Zoho, AOL, Custom)
1. Log in or navigate to **Settings > Accounts > Add Account**
2. Enter your email address — the system auto-detects provider settings
3. Enter your email password (or App Password for providers that require it)
4. The system tests the connection and syncs your inbox

**Provider-specific notes:**
- **Gmail**: Use an [App Password](https://myaccount.google.com/apppasswords) (requires 2FA enabled)
- **Yahoo**: Use an [App Password](https://login.yahoo.com/account/security)
- **iCloud**: Use an [App-Specific Password](https://appleid.apple.com/account/manage)
- **ProtonMail**: Requires [ProtonMail Bridge](https://protonmail.com/bridge) running locally

---

## Stopping Everything

```bash
# Stop all containers (preserves data)
docker compose down

# Stop and DELETE all data volumes
docker compose down -v
```

---

## Project Structure

```
center-point-inbox/
├── src/
│   ├── backend/           # .NET 8 microservices
│   │   ├── Shared/        # Common library (auth, models, middleware)
│   │   ├── ApiGateway/    # YARP reverse proxy (port 5000)
│   │   ├── AuthService/   # JWT auth, OAuth (port 5001)
│   │   ├── EmailService/  # IMAP/SMTP universal email (port 5008)
│   │   ├── GoogleProviderService/
│   │   ├── MicrosoftProviderService/
│   │   ├── MergerService/
│   │   ├── TranslationWorker/
│   │   ├── AiAssistantService/
│   │   └── AuditDlpService/
│   └── frontend/          # Next.js 14 (TypeScript, Tailwind, Radix UI)
├── extension/             # Chrome Extension (Manifest V3)
├── database/migrations/   # PostgreSQL schema & seed data
├── infrastructure/        # Dockerfiles, Kubernetes manifests, CI/CD
├── docker-compose.yml     # Full-stack orchestration
└── .env.example           # Environment variable template
```

---

## Troubleshooting

**Containers failing to start?**
```bash
docker compose logs -f <service-name>
```

**Port already in use?**
Change the host port in `docker-compose.yml` (e.g., `"3001:3000"` for frontend).

**Database not initialized?**
Migrations run automatically on first Postgres start via the mounted `database/migrations/` directory.

**Frontend build fails in Docker?**
Make sure `src/frontend/package-lock.json` exists. If not:
```bash
cd src/frontend && npm install && cd ../..
docker compose up -d --build frontend
```
