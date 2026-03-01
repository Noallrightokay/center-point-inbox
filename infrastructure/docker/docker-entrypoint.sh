#!/bin/bash
set -e

# =============================================================================
# Center Point Inbox - Backend Service Entrypoint
# Waits for required infrastructure dependencies before starting the service.
# =============================================================================

# ---------------------------------------------------------------------------
# Configuration (set via environment variables)
# ---------------------------------------------------------------------------
WAIT_TIMEOUT="${WAIT_TIMEOUT:-60}"
WAIT_INTERVAL="${WAIT_INTERVAL:-2}"

# ---------------------------------------------------------------------------
# wait_for_host - waits for a TCP connection to host:port
# ---------------------------------------------------------------------------
wait_for_host() {
    local host="$1"
    local port="$2"
    local name="$3"
    local elapsed=0

    echo "[entrypoint] Waiting for ${name} at ${host}:${port} ..."

    while ! bash -c "echo > /dev/tcp/${host}/${port}" 2>/dev/null; do
        elapsed=$((elapsed + WAIT_INTERVAL))
        if [ "$elapsed" -ge "$WAIT_TIMEOUT" ]; then
            echo "[entrypoint] ERROR: Timed out waiting for ${name} at ${host}:${port} after ${WAIT_TIMEOUT}s"
            exit 1
        fi
        echo "[entrypoint]   ${name} not ready, retrying in ${WAIT_INTERVAL}s... (${elapsed}/${WAIT_TIMEOUT}s)"
        sleep "$WAIT_INTERVAL"
    done

    echo "[entrypoint] ${name} at ${host}:${port} is ready."
}

# ---------------------------------------------------------------------------
# Wait for PostgreSQL if configured
# ---------------------------------------------------------------------------
if [ -n "$POSTGRES_HOST" ] && [ -n "$POSTGRES_PORT" ]; then
    wait_for_host "$POSTGRES_HOST" "$POSTGRES_PORT" "PostgreSQL"
fi

# ---------------------------------------------------------------------------
# Wait for RabbitMQ if configured
# ---------------------------------------------------------------------------
if [ -n "$RABBITMQ_HOST" ] && [ -n "$RABBITMQ_PORT" ]; then
    wait_for_host "$RABBITMQ_HOST" "$RABBITMQ_PORT" "RabbitMQ"
fi

# ---------------------------------------------------------------------------
# Wait for Redis if configured
# ---------------------------------------------------------------------------
if [ -n "$REDIS_HOST" ] && [ -n "$REDIS_PORT" ]; then
    wait_for_host "$REDIS_HOST" "$REDIS_PORT" "Redis"
fi

# ---------------------------------------------------------------------------
# Wait for MinIO / S3 if configured
# ---------------------------------------------------------------------------
if [ -n "$MINIO_HOST" ] && [ -n "$MINIO_PORT" ]; then
    wait_for_host "$MINIO_HOST" "$MINIO_PORT" "MinIO"
fi

# ---------------------------------------------------------------------------
# Start the .NET service
# ---------------------------------------------------------------------------
echo "[entrypoint] Starting service: dotnet ${SERVICE_DLL}"
exec dotnet "${SERVICE_DLL}" "$@"
