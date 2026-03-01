# Center Point Inbox -- Architecture Documentation

## Table of Contents

- [1. System Overview](#1-system-overview)
- [2. High-Level Architecture Diagram](#2-high-level-architecture-diagram)
- [3. Service Descriptions](#3-service-descriptions)
- [4. Sequence Diagrams](#4-sequence-diagrams)
  - [4.1 OAuth Login Flow](#41-oauth-login-flow)
  - [4.2 Unified File Feed](#42-unified-file-feed)
  - [4.3 Translation Flow](#43-translation-flow)
  - [4.4 AI Query Flow](#44-ai-query-flow)
  - [4.5 DLP Scan Flow](#45-dlp-scan-flow)
- [5. Service Communication Patterns](#5-service-communication-patterns)
- [6. Data Flow Diagram](#6-data-flow-diagram)
- [7. Technology Stack](#7-technology-stack)
- [8. Database Schema Overview](#8-database-schema-overview)

---

## 1. System Overview

Center Point Inbox is a multi-tenant SaaS platform that provides a unified, AI-powered document workspace integrating Google Workspace and Microsoft 365. It enables enterprises operating in hybrid cloud environments to browse, translate, search, and govern documents from a single pane of glass.

The platform follows a microservices architecture deployed on Kubernetes, with each service responsible for a distinct business capability. Services communicate via synchronous HTTP (REST) for request-response flows and asynchronous messaging (RabbitMQ) for background work such as document translation and DLP scanning.

### Design Principles

- **Tenant Isolation**: Every database query is scoped to a tenant ID embedded in the JWT. Cross-tenant access is structurally impossible at the data layer.
- **Provider Abstraction**: Google and Microsoft are pluggable providers behind a unified Merger Service, so adding future providers (Dropbox, Box) requires no frontend changes.
- **Compliance by Default**: DLP scanning, audit logging, and PHI detection are built into the core pipeline rather than bolted on as afterthoughts.
- **Stateless Services**: All backend services are stateless and horizontally scalable. Session state lives in Redis; persistent state lives in PostgreSQL and S3/MinIO.

---

## 2. High-Level Architecture Diagram

```mermaid
graph TB
    subgraph Clients["Clients"]
        Browser["Browser (React/Next.js)"]
        MobileApp["Mobile App (Future)"]
        APIConsumer["API Consumer"]
    end

    subgraph Frontend["Frontend -- Next.js 14"]
        NextApp["Next.js App Router<br/>React 18 + TanStack Query<br/>Zustand + Tailwind CSS"]
    end

    subgraph Gateway["API Gateway Layer"]
        APIGateway["API Gateway<br/>.NET 8 / Port 5000<br/>JWT Validation, Routing,<br/>Rate Limiting, CORS"]
    end

    subgraph BackendServices["Backend Microservices (.NET 8)"]
        AuthService["Auth Service<br/>Port 5001<br/>OAuth2 + JWT Issuance<br/>Token Rotation"]
        GoogleProvider["Google Provider<br/>Port 5002<br/>Google Drive API<br/>Google Docs Export"]
        MicrosoftProvider["Microsoft Provider<br/>Port 5003<br/>OneDrive + SharePoint<br/>Microsoft Graph API"]
        MergerService["Merger Service<br/>Port 5004<br/>Unified File Feed<br/>Cross-Provider Merge"]
        TranslationWorker["Translation Worker<br/>(No HTTP port)<br/>Format Conversion<br/>RabbitMQ Consumer"]
        AiAssistant["AI Assistant<br/>Port 5006<br/>Semantic Search<br/>Summarization + Q&A"]
        AuditDlp["Audit & DLP Service<br/>Port 5007<br/>Audit Logging<br/>PHI/PII Detection"]
    end

    subgraph DataStores["Data Stores"]
        PostgreSQL["PostgreSQL 16<br/>+ pgvector Extension<br/>Port 5432"]
        Redis["Redis 7<br/>Session Cache + Rate Limits<br/>Port 6379"]
        RabbitMQ["RabbitMQ 3<br/>Task Queues<br/>Ports 5672 / 15672"]
        MinIO["MinIO (S3-Compatible)<br/>Translation Output Storage<br/>Ports 9000 / 9001"]
    end

    subgraph ExternalAPIs["External APIs"]
        GoogleAPI["Google APIs<br/>Drive API v3<br/>Docs/Sheets/Slides Export"]
        MicrosoftAPI["Microsoft Graph API<br/>OneDrive + SharePoint<br/>File Content + Metadata"]
        OpenAI["OpenAI API<br/>GPT-4o (Chat)<br/>text-embedding-ada-002"]
    end

    Browser --> NextApp
    MobileApp -.-> APIGateway
    APIConsumer -.-> APIGateway
    NextApp -->|"REST /api/*"| APIGateway

    APIGateway -->|"/api/auth/*"| AuthService
    APIGateway -->|"/api/files"| MergerService
    APIGateway -->|"/api/files/google/*"| GoogleProvider
    APIGateway -->|"/api/files/microsoft/*"| MicrosoftProvider
    APIGateway -->|"/api/translate/*"| MergerService
    APIGateway -->|"/api/ai/*"| AiAssistant
    APIGateway -->|"/api/audit/*"| AuditDlp
    APIGateway -->|"/api/admin/*"| AuditDlp

    MergerService -->|"HTTP"| GoogleProvider
    MergerService -->|"HTTP"| MicrosoftProvider
    MergerService -->|"Publish: translation.requested"| RabbitMQ

    TranslationWorker -->|"Consume: translation.requested"| RabbitMQ
    TranslationWorker -->|"Store translated files"| MinIO
    TranslationWorker -->|"Publish: translation.completed"| RabbitMQ

    AuditDlp -->|"Consume: file.indexed"| RabbitMQ
    AuditDlp -->|"Publish: dlp.violation.detected"| RabbitMQ

    AuthService -->|"OAuth2 Authorization Code"| GoogleAPI
    AuthService -->|"OAuth2 Authorization Code"| MicrosoftAPI
    GoogleProvider -->|"Drive API v3"| GoogleAPI
    MicrosoftProvider -->|"Graph API"| MicrosoftAPI
    AiAssistant -->|"Embeddings + Chat Completions"| OpenAI

    AuthService --> PostgreSQL
    AuthService --> Redis
    GoogleProvider --> PostgreSQL
    MicrosoftProvider --> PostgreSQL
    MergerService --> PostgreSQL
    AiAssistant --> PostgreSQL
    AuditDlp --> PostgreSQL
    TranslationWorker --> PostgreSQL

    APIGateway --> Redis

    classDef frontend fill:#3b82f6,stroke:#1d4ed8,color:#fff
    classDef gateway fill:#8b5cf6,stroke:#6d28d9,color:#fff
    classDef service fill:#10b981,stroke:#059669,color:#fff
    classDef datastore fill:#f59e0b,stroke:#d97706,color:#fff
    classDef external fill:#ef4444,stroke:#dc2626,color:#fff
    classDef client fill:#6b7280,stroke:#4b5563,color:#fff

    class Browser,MobileApp,APIConsumer client
    class NextApp frontend
    class APIGateway gateway
    class AuthService,GoogleProvider,MicrosoftProvider,MergerService,TranslationWorker,AiAssistant,AuditDlp service
    class PostgreSQL,Redis,RabbitMQ,MinIO datastore
    class GoogleAPI,MicrosoftAPI,OpenAI external
```

---

## 3. Service Descriptions

| Service | Port | Responsibility | Dependencies |
|---------|------|----------------|--------------|
| **API Gateway** | 5000 | JWT validation, request routing, rate limiting, CORS enforcement | Redis, Auth Service, all downstream services |
| **Auth Service** | 5001 | OAuth2 authorization code flow with Google/Microsoft, JWT issuance and refresh, token encryption at rest, session management | PostgreSQL, Redis, RabbitMQ, Google/Microsoft OAuth |
| **Google Provider** | 5002 | Google Drive file listing, metadata sync, file download, Docs/Sheets/Slides export | PostgreSQL, RabbitMQ, Google Drive API |
| **Microsoft Provider** | 5003 | OneDrive/SharePoint file listing, metadata sync, file download | PostgreSQL, RabbitMQ, Microsoft Graph API |
| **Merger Service** | 5004 | Unified file feed aggregation across providers, cross-provider search, translation job dispatch | PostgreSQL, RabbitMQ, Google Provider, Microsoft Provider |
| **Translation Worker** | -- | Background worker consuming translation jobs from RabbitMQ, format conversion (DOCX to PDF, Sheets to CSV, etc.), output storage to S3/MinIO | PostgreSQL, RabbitMQ, MinIO |
| **AI Assistant** | 5006 | Semantic search over document embeddings (pgvector), document summarization, natural language Q&A with source attribution | PostgreSQL, RabbitMQ, OpenAI API |
| **Audit & DLP** | 5007 | Immutable audit log recording, DLP policy enforcement, PHI/PII pattern scanning, file quarantine management | PostgreSQL, RabbitMQ |
| **Frontend** | 3000 | Next.js 14 App Router SPA with server-side rendering, Zustand state management, TanStack Query for data fetching | API Gateway |

---

## 4. Sequence Diagrams

### 4.1 OAuth Login Flow

```mermaid
sequenceDiagram
    autonumber
    actor User
    participant Frontend as Frontend<br/>(Next.js)
    participant Gateway as API Gateway
    participant Auth as Auth Service
    participant Google as Google OAuth
    participant Microsoft as Microsoft OAuth
    participant DB as PostgreSQL
    participant Cache as Redis

    User->>Frontend: Click "Sign in with Google"
    Frontend->>Gateway: POST /api/auth/login {provider: "google"}
    Gateway->>Auth: Forward login request
    Auth->>Auth: Generate state + PKCE verifier
    Auth->>Cache: Store state & PKCE (TTL: 10 min)
    Auth-->>Gateway: 302 Redirect to Google OAuth consent URL
    Gateway-->>Frontend: Return authorization URL
    Frontend->>Google: Redirect user to consent screen

    User->>Google: Grant consent
    Google-->>Frontend: Redirect with authorization code + state
    Frontend->>Gateway: POST /api/auth/callback {code, state, provider}
    Gateway->>Auth: Forward callback
    Auth->>Cache: Validate state, retrieve PKCE verifier
    Auth->>Google: Exchange code for tokens (with PKCE)
    Google-->>Auth: Return access_token + refresh_token
    Auth->>Google: GET /userinfo (email, name, picture)
    Google-->>Auth: Return user profile
    Auth->>DB: Upsert user + tenant, store encrypted OAuth tokens
    Auth->>Auth: Generate JWT (access + refresh)
    Auth->>Cache: Store session metadata
    Auth->>DB: Insert audit_log (action: "user.login")
    Auth-->>Gateway: Return {accessToken, refreshToken, user}
    Gateway-->>Frontend: Return JWT + user profile
    Frontend->>Frontend: Store tokens in memory (not localStorage)
    Frontend-->>User: Redirect to Dashboard
```

### 4.2 Unified File Feed

```mermaid
sequenceDiagram
    autonumber
    actor User
    participant Frontend as Frontend
    participant Gateway as API Gateway
    participant Merger as Merger Service
    participant Google as Google Provider
    participant Microsoft as Microsoft Provider
    participant DB as PostgreSQL
    participant Cache as Redis

    User->>Frontend: Open Dashboard / Files page
    Frontend->>Gateway: GET /api/files?page=1&sort=modified
    Gateway->>Gateway: Validate JWT, extract tenant_id + user_id
    Gateway->>Merger: Forward request with auth context

    par Parallel Provider Fetch
        Merger->>Google: GET /api/files/google?page=1&sort=modified
        Google->>DB: SELECT files_metadata WHERE provider='google' AND tenant_id=?
        Google-->>Merger: Google files page

        Merger->>Microsoft: GET /api/files/microsoft?page=1&sort=modified
        Microsoft->>DB: SELECT files_metadata WHERE provider='microsoft' AND tenant_id=?
        Microsoft-->>Merger: Microsoft files page
    end

    Merger->>Merger: Merge, deduplicate, sort by last_modified
    Merger->>Cache: Cache merged result (TTL: 60s)
    Merger-->>Gateway: Unified file list with pagination
    Gateway-->>Frontend: JSON response
    Frontend-->>User: Render unified file feed
```

### 4.3 Translation Flow

```mermaid
sequenceDiagram
    autonumber
    actor User
    participant Frontend as Frontend
    participant Gateway as API Gateway
    participant Merger as Merger Service
    participant Queue as RabbitMQ
    participant Worker as Translation Worker
    participant S3 as MinIO / S3
    participant DB as PostgreSQL
    participant DLP as Audit & DLP

    User->>Frontend: Select file, choose target format (e.g., PDF)
    Frontend->>Gateway: POST /api/translate {fileId, targetFormat}
    Gateway->>Merger: Forward translation request
    Merger->>DB: INSERT translation_jobs (status: 'queued')
    Merger->>Queue: Publish to "translation.requested" queue
    Merger-->>Gateway: {jobId, status: "queued"}
    Gateway-->>Frontend: Return job ID
    Frontend-->>User: Show "Translation in progress..."

    Queue->>Worker: Consume "translation.requested" message
    Worker->>DB: UPDATE translation_jobs SET status='processing'
    Worker->>DB: SELECT files_metadata for source file
    Worker->>Worker: Download source content from provider
    Worker->>Worker: Convert format (e.g., DOCX to PDF)
    Worker->>S3: Upload translated file to bucket
    Worker->>DB: UPDATE translation_jobs SET status='completed', output_storage_path=?
    Worker->>Queue: Publish to "translation.completed" exchange
    Worker->>Queue: Publish to "file.indexed" (triggers DLP scan)

    Queue->>DLP: Consume "file.indexed" for DLP scanning

    loop Polling (every 3s)
        Frontend->>Gateway: GET /api/translate/{jobId}
        Gateway->>Merger: Forward status check
        Merger->>DB: SELECT status FROM translation_jobs WHERE id=?
        Merger-->>Gateway: {status: "completed", downloadUrl}
        Gateway-->>Frontend: Return completion status
    end

    Frontend-->>User: Show "Download Ready" with link
    User->>Frontend: Click download
    Frontend->>Gateway: GET /api/translate/{jobId}/download
    Gateway->>S3: Fetch translated file
    S3-->>Gateway: File stream
    Gateway-->>Frontend: File download
```

### 4.4 AI Query Flow

```mermaid
sequenceDiagram
    autonumber
    actor User
    participant Frontend as Frontend
    participant Gateway as API Gateway
    participant AI as AI Assistant
    participant DB as PostgreSQL<br/>(pgvector)
    participant OpenAI as OpenAI API

    User->>Frontend: Type question: "What are Q4 revenue projections?"
    Frontend->>Gateway: POST /api/ai/ask {question, conversationId?}
    Gateway->>Gateway: Validate JWT, extract tenant_id
    Gateway->>AI: Forward question with auth context

    AI->>OpenAI: Generate embedding for question text
    OpenAI-->>AI: Return 1536-dim vector

    AI->>DB: SELECT chunk_text, file_id<br/>FROM embeddings<br/>WHERE tenant_id = ?<br/>ORDER BY embedding <=> query_vector<br/>LIMIT 10
    DB-->>AI: Top 10 relevant document chunks

    AI->>AI: Build prompt with retrieved context chunks
    AI->>OpenAI: POST /v1/chat/completions<br/>{system: context, user: question}
    OpenAI-->>AI: Generated answer

    AI->>AI: Attach source file references to response
    AI->>DB: INSERT audit_logs (action: "ai.query")
    AI-->>Gateway: {answer, sources: [{fileId, fileName, snippet}]}
    Gateway-->>Frontend: Return answer with source attribution
    Frontend-->>User: Display answer with clickable source links
```

### 4.5 DLP Scan Flow

```mermaid
sequenceDiagram
    autonumber
    participant Provider as Google/Microsoft<br/>Provider
    participant Queue as RabbitMQ
    participant DLP as Audit & DLP Service
    participant DB as PostgreSQL
    participant Admin as Admin Dashboard

    Provider->>DB: INSERT/UPDATE files_metadata (file synced)
    Provider->>Queue: Publish to "file.indexed" exchange

    Queue->>DLP: Consume "file.indexed" message
    DLP->>DB: SELECT file metadata + content reference
    DLP->>DLP: Extract text content from file

    DLP->>DLP: Run PHI/PII pattern matchers:<br/>- SSN (XXX-XX-XXXX)<br/>- Credit Card (Luhn check)<br/>- Email addresses<br/>- Phone numbers<br/>- Medical Record Numbers

    alt Violation Detected
        DLP->>DB: INSERT dlp_violations (severity, pattern_matched)
        DLP->>DB: UPDATE files_metadata SET phi_detected=true

        alt Severity = Critical or High
            DLP->>DB: UPDATE files_metadata SET quarantined=true
            DLP->>DB: INSERT audit_logs (action: "file.quarantined")
            DLP->>Queue: Publish "dlp.violation.detected" event
        else Severity = Medium or Low
            DLP->>DB: INSERT audit_logs (action: "dlp.violation.logged")
        end
    else No Violation
        DLP->>DB: UPDATE files_metadata SET phi_detected=false
        DLP->>DB: INSERT audit_logs (action: "dlp.scan.clean")
    end

    Admin->>DLP: GET /api/admin/dlp/violations
    DLP->>DB: SELECT dlp_violations WHERE tenant_id=?
    DLP-->>Admin: List of violations with severity + status

    Admin->>DLP: POST /api/admin/dlp/violations/{id}/resolve
    DLP->>DB: UPDATE dlp_violations SET resolved=true, resolved_by=?
    DLP->>DB: INSERT audit_logs (action: "dlp.violation.resolved")
    DLP-->>Admin: Confirmation

    Admin->>DLP: POST /api/admin/dlp/files/{fileId}/unquarantine
    DLP->>DB: UPDATE files_metadata SET quarantined=false
    DLP->>DB: INSERT audit_logs (action: "file.unquarantined")
    DLP-->>Admin: Confirmation
```

---

## 5. Service Communication Patterns

| Source | Destination | Protocol | Pattern | Exchange/Queue | Description |
|--------|------------|----------|---------|----------------|-------------|
| Frontend | API Gateway | HTTPS | Request-Response | -- | All client API calls route through the gateway |
| API Gateway | Auth Service | HTTP | Request-Response | -- | Login, token refresh, token revocation, user profile |
| API Gateway | Merger Service | HTTP | Request-Response | -- | Unified file listing, translation job creation |
| API Gateway | Google Provider | HTTP | Request-Response | -- | Direct Google file operations (download, export) |
| API Gateway | Microsoft Provider | HTTP | Request-Response | -- | Direct Microsoft file operations (download) |
| API Gateway | AI Assistant | HTTP | Request-Response | -- | AI queries, summarization, semantic search |
| API Gateway | Audit & DLP | HTTP | Request-Response | -- | Audit log retrieval, DLP dashboard, admin operations |
| Merger Service | Google Provider | HTTP | Request-Response | -- | Fetch Google files for unified feed |
| Merger Service | Microsoft Provider | HTTP | Request-Response | -- | Fetch Microsoft files for unified feed |
| Merger Service | RabbitMQ | AMQP | Publish | `translation.requested` | Dispatch translation jobs |
| Translation Worker | RabbitMQ | AMQP | Consume | `translation.requested` | Pick up translation jobs |
| Translation Worker | RabbitMQ | AMQP | Publish | `translation.completed` | Signal job completion |
| Translation Worker | RabbitMQ | AMQP | Publish | `file.indexed` | Trigger DLP scan on translated output |
| Translation Worker | MinIO | HTTP (S3) | Request-Response | -- | Store translated file output |
| Google Provider | RabbitMQ | AMQP | Publish | `file.indexed` | Trigger DLP scan after file sync |
| Microsoft Provider | RabbitMQ | AMQP | Publish | `file.indexed` | Trigger DLP scan after file sync |
| Audit & DLP | RabbitMQ | AMQP | Consume | `file.indexed` | DLP scanning pipeline |
| Audit & DLP | RabbitMQ | AMQP | Publish | `dlp.violation.detected` | Alert on DLP violations |
| AI Assistant | OpenAI | HTTPS | Request-Response | -- | Embedding generation and chat completions |
| Auth Service | Google OAuth | HTTPS | Request-Response | -- | OAuth2 authorization code exchange |
| Auth Service | Microsoft OAuth | HTTPS | Request-Response | -- | OAuth2 authorization code exchange |
| All Services | PostgreSQL | TCP | Request-Response | -- | Persistent data storage |
| API Gateway, Auth | Redis | TCP | Request-Response | -- | Caching, sessions, rate limit counters |

---

## 6. Data Flow Diagram

```mermaid
graph LR
    subgraph Ingestion["Data Ingestion"]
        G_API["Google Drive API"] -->|File metadata + content| GP["Google Provider"]
        M_API["Microsoft Graph API"] -->|File metadata + content| MP["Microsoft Provider"]
    end

    subgraph Storage["Canonical Storage"]
        GP -->|INSERT/UPDATE| FM["files_metadata<br/>(PostgreSQL)"]
        MP -->|INSERT/UPDATE| FM
    end

    subgraph Events["Event Pipeline"]
        GP -->|file.indexed| RMQ["RabbitMQ"]
        MP -->|file.indexed| RMQ
        RMQ -->|file.indexed| DLP["DLP Scanner"]
        DLP -->|violation?| DLPV["dlp_violations<br/>(PostgreSQL)"]
        DLP -->|quarantine?| FM
    end

    subgraph AIProcessing["AI Processing"]
        FM -->|File content| EMB["Embedding Pipeline"]
        EMB -->|text-embedding-ada-002| OAI["OpenAI API"]
        OAI -->|1536-dim vectors| EMBDB["embeddings<br/>(pgvector)"]
        EMBDB -->|Nearest neighbor search| AIS["AI Assistant"]
        AIS -->|Chat completion| OAI
    end

    subgraph TranslationPipeline["Translation Pipeline"]
        MS["Merger Service"] -->|translation.requested| RMQ
        RMQ -->|Consume| TW["Translation Worker"]
        TW -->|Converted files| S3["MinIO / S3"]
        TW -->|Status updates| TJ["translation_jobs<br/>(PostgreSQL)"]
        TW -->|translation.completed| RMQ
    end

    subgraph Audit["Audit Trail"]
        ALL["All Services"] -->|Action events| AL["audit_logs<br/>(PostgreSQL)"]
    end

    subgraph Serving["Data Serving"]
        FM --> MS
        MS -->|Unified feed| GW["API Gateway"]
        AIS -->|Answers + sources| GW
        GW -->|JSON responses| FE["Frontend"]
    end
```

---

## 7. Technology Stack

### Backend

| Layer | Technology | Version | Purpose |
|-------|-----------|---------|---------|
| Runtime | .NET 8 (ASP.NET Core) | 8.0 | All backend microservices |
| Database | PostgreSQL | 16 (Alpine) | Primary relational data store |
| Vector DB | pgvector extension | -- | Semantic search embeddings (1536-dim) |
| Cache | Redis | 7 (Alpine) | Session cache, rate limiting, query caching |
| Message Queue | RabbitMQ | 3 (Management Alpine) | Async job dispatch, event publishing |
| Object Storage | MinIO (S3-compatible) | Latest | Translation output, file cache |
| Containerization | Docker | Multi-stage builds | Service packaging |
| Orchestration | Kubernetes | v1.27+ | Production deployment, scaling |
| Ingress | NGINX Ingress Controller | -- | TLS termination, rate limiting, routing |

### Frontend

| Layer | Technology | Version | Purpose |
|-------|-----------|---------|---------|
| Framework | Next.js (App Router) | 14.2.15 | SSR + SPA with file-based routing |
| UI Library | React | 18.3 | Component rendering |
| State Management | Zustand | 5.0 | Client-side global state |
| Data Fetching | TanStack Query | 5.59 | Server state, caching, refetching |
| HTTP Client | Axios | 1.7 | API communication |
| Styling | Tailwind CSS | 3.4 | Utility-first CSS |
| Components | Radix UI | Various | Accessible primitives (Dialog, Dropdown, Tabs, Toast) |
| Icons | Lucide React | 0.451 | SVG icon set |
| Date Utilities | date-fns | 4.1 | Date formatting and manipulation |

### External APIs

| Provider | API | Purpose |
|----------|-----|---------|
| Google | Drive API v3 | File listing, download, export |
| Google | OAuth 2.0 | User authentication |
| Microsoft | Graph API | OneDrive/SharePoint file access |
| Microsoft | OAuth 2.0 (MSAL) | User authentication |
| OpenAI | Chat Completions (GPT-4o) | Natural language Q&A, summarization |
| OpenAI | Embeddings (text-embedding-ada-002) | Semantic vector generation |

---

## 8. Database Schema Overview

The PostgreSQL database uses the following core tables, all scoped to tenant isolation:

| Table | Purpose | Key Fields |
|-------|---------|------------|
| `tenants` | Multi-tenant organization units | id, slug, subscription_tier, settings (JSONB) |
| `users` | User accounts scoped to tenants | id, email, role (admin/user/viewer), tenant_id |
| `oauth_connections` | Encrypted OAuth tokens per provider | user_id, provider, access_token_encrypted, refresh_token_encrypted |
| `files_metadata` | Canonical file records from all providers | provider, provider_file_id, phi_detected, quarantined, tenant_id |
| `translation_jobs` | Document format conversion tracking | source_file_id, source_format, target_format, status, output_storage_path |
| `audit_logs` | Immutable, append-only action log | action, resource_type, resource_id, ip_address, metadata (JSONB) |
| `dlp_violations` | PHI/PII detection records | violation_type, severity, pattern_matched, resolved |
| `embeddings` | pgvector semantic search vectors | file_id, chunk_index, chunk_text, embedding (vector 1536) |

### Key Indexes

- **IVFFlat cosine index** on `embeddings.embedding` for approximate nearest-neighbor search
- **Partial indexes** on `files_metadata.phi_detected` and `files_metadata.quarantined` for fast DLP queries
- **Composite index** on `audit_logs (tenant_id, created_at DESC)` for time-ordered audit trail retrieval
- **Unique constraint** on `(tenant_id, provider, provider_file_id)` preventing duplicate file records

### Extensions

- `pgcrypto` -- UUID generation via `gen_random_uuid()`
- `vector` (pgvector) -- 1536-dimensional vector storage and cosine distance search
