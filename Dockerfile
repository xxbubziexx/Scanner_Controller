# Production Multi-Arch Dockerfile for Linux / Raspberry Pi / x86_64
FROM python:3.11-slim

WORKDIR /app

# Install curl for container health check
RUN apt-get update && apt-get install -y --no-install-recommends \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Install python dependencies
COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

# Copy application source
COPY . .

# Environment defaults for containerized Linux deployment
ENV PYTHONUNBUFFERED=1 \
    SCANSCRIBE_INBOX_DIR=/data/inbox \
    SCANNER_A_PORT=/dev/ttyACM0 \
    SCANNER_B_PORT=NONE

# Expose Web UI & WebSocket server
EXPOSE 8000

# Health check endpoint
HEALTHCHECK --interval=15s --timeout=3s --retries=3 \
  CMD curl -f http://localhost:8000/api/state || exit 1

# Launch ASGI server
CMD ["python", "-m", "uvicorn", "server:app", "--host", "0.0.0.0", "--port", "8000"]
